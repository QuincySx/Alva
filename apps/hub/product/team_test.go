// INPUT: Actual durable product/native stores, local Alva harness and controlled execution owners
// OUTPUT: Explicit Team dispatch, exact-owner Stop/join, observer detach and real FIFO terminal regressions
// POS: [L7 Hub product] Product Worker pumps are not native SDK subagents or automatic startup replay
package product

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"strings"
	"sync"
	"testing"
	"time"
)

type workerResumeCleanupEngine struct {
	engine.Runtime
	started chan struct{}
	cleanup chan struct{}
	release chan struct{}
}

type workerSequenceCall struct {
	ctx    context.Context
	finish func()
}
type workerSequenceEngine struct {
	engine.Runtime
	calls    chan workerSequenceCall
	cancels  chan string
	shutdown chan struct{}
}

func (e workerSequenceEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true}
}
func (e workerSequenceEngine) Cancel(_ context.Context, id string) error { e.cancels <- id; return nil }
func (e workerSequenceEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "ordered-worker-native"}); err != nil {
		return err
	}
	release := make(chan struct{})
	e.calls <- workerSequenceCall{ctx: ctx, finish: sync.OnceFunc(func() { close(release) })}
	select {
	case <-e.shutdown:
		return context.Canceled
	case <-release:
		// A true success can arrive while Stop is joining. The pump must not
		// publish the next claim before the Stop operation finishes its review.
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	}
}

func workerSequenceFixture(t *testing.T) (*HubService, hub.Session, hub.TeamDetail, hub.Worker, workerSequenceEngine) {
	t.Helper()
	s, _ := fixture(t)
	e := workerSequenceEngine{calls: make(chan workerSequenceCall, 3), cancels: make(chan string, 3), shutdown: make(chan struct{})}
	t.Cleanup(func() { close(e.shutdown) })
	if err := s.Runtime.Register("ordered-worker", e); err != nil {
		t.Fatal(err)
	}
	route := s.DefaultTarget
	route.EngineID = "ordered-worker"
	s.Targets = append(s.Targets, route)
	lead, err := s.Create(t.Context(), "ordered Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(t.Context(), lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w, err := s.AddWorker(t.Context(), lead.ID, team.Team.ID, "ordered worker", route, true)
	if err != nil {
		t.Fatal(err)
	}
	for _, prompt := range []string{"first", "second", "third"} {
		if _, err = s.Teams.Store.EnqueueWorker(t.Context(), lead.ID, team.Team.ID, w.ID, prompt, nil); err != nil {
			t.Fatal(err)
		}
	}
	return s, lead, team, w, e
}

func workerReceive[T any](t *testing.T, ch <-chan T) T {
	t.Helper()
	select {
	case out := <-ch:
		return out
	case <-time.After(3 * time.Second):
		t.Fatal("Worker execution barrier timed out")
		var zero T
		return zero
	}
}

func TestWorkerExecutionStopFreezesNextClaimThroughActualSuccess(t *testing.T) {
	s, lead, team, w, e := workerSequenceFixture(t)
	if err := s.Teams.Resume(t.Context(), lead.ID, team.Team.ID, w.ID); err != nil {
		t.Fatal(err)
	}
	first := workerReceive(t, e.calls)
	execution, active := s.Runtime.ActiveExecution(w.SessionID)
	if !active || execution == "" {
		t.Fatal("missing actual first execution")
	}
	stopped := make(chan error, 1)
	go func() { stopped <- s.Teams.StopExecution(t.Context(), lead.ID, team.Team.ID, w.ID, execution) }()
	workerReceive(t, first.ctx.Done())
	first.finish()
	if err := workerReceive(t, stopped); err != nil {
		t.Fatal(err)
	}
	tasks, err := s.Teams.Store.WorkerTasks(t.Context(), lead.ID, team.Team.ID, w.ID, 0, 8)
	if err != nil || tasks.Tasks[0].Status != engine.Succeeded || tasks.Tasks[1].Status != hub.TaskQueued || tasks.Tasks[2].Status != hub.TaskQueued || len(e.calls) != 0 || s.Teams.Status(w.ID).Active {
		t.Fatal("Stop admitted a successor while joining the original success", tasks, err)
	}
}

type workerTeamReadBarrier struct {
	hub.TeamStore
	entered chan struct{}
	release chan struct{}
}

func (s workerTeamReadBarrier) Team(ctx context.Context, lead, team string) (hub.TeamDetail, error) {
	close(s.entered)
	<-s.release
	return s.TeamStore.Team(ctx, lead, team)
}

func TestWorkerLateExecutionStopCannotCancelReplacementOrHaltFIFO(t *testing.T) {
	s, lead, team, w, e := workerSequenceFixture(t)
	if err := s.Teams.Resume(t.Context(), lead.ID, team.Team.ID, w.ID); err != nil {
		t.Fatal(err)
	}
	first := workerReceive(t, e.calls)
	old, _ := s.Runtime.ActiveExecution(w.SessionID)
	barrier := workerTeamReadBarrier{TeamStore: s.Teams.Store, entered: make(chan struct{}), release: make(chan struct{})}
	s.Teams.Store = barrier
	unblock := sync.OnceFunc(func() { close(barrier.release) })
	defer unblock()
	stopped := make(chan error, 1)
	go func() { stopped <- s.Teams.StopExecution(t.Context(), lead.ID, team.Team.ID, w.ID, old) }()
	workerReceive(t, barrier.entered)
	first.finish()
	second := workerReceive(t, e.calls)
	current, active := s.Runtime.ActiveExecution(w.SessionID)
	if !active || current == old {
		t.Fatal("fixture did not replace the original execution")
	}
	unblock()
	if err := workerReceive(t, stopped); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("late Stop accepted a replaced owner", err)
	}
	if second.ctx.Err() != nil || len(e.cancels) != 0 {
		t.Fatal("late Stop cancelled the replacement owner or its pump")
	}
	second.finish()
	third := workerReceive(t, e.calls)
	if third.ctx.Err() != nil {
		t.Fatal("stale Stop halted the remaining FIFO")
	}
	third.finish()
	waitIdle(t, s, w.SessionID)
}

func (e *workerResumeCleanupEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true, DurableWaits: true}
}
func (e *workerResumeCleanupEngine) Cancel(context.Context, string) error { return nil }
func (e *workerResumeCleanupEngine) Execute(ctx context.Context, req engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "worker-wait-native"}); err != nil {
		return err
	}
	if req.WaitRevision == 0 {
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Waiting, Wait: &engine.WaitCheckpoint{Revision: 3, Requests: 1, ExpiresAt: time.Now().Add(time.Hour).UnixMilli()}})
	}
	close(e.started)
	<-ctx.Done()
	close(e.cleanup)
	<-e.release
	return ctx.Err()
}

func TestWorkerStopJoinsIndependentSameTurnResumeOwner(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	store := s.Store.(interface {
		hub.ExecutionStore
		hub.WaitStore
	})
	runtime := hub.NewWithWaits(store)
	t.Cleanup(func() { _ = runtime.Close(context.Background()) })
	s.Runtime, s.Teams.runtime = runtime, runtime
	adapter := &workerResumeCleanupEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
	var release sync.Once
	unblock := func() { release.Do(func() { close(adapter.release) }) }
	defer unblock()
	if err := runtime.Register("worker-wait", adapter); err != nil {
		t.Fatal(err)
	}
	route := s.DefaultTarget
	route.EngineID = "worker-wait"
	s.Targets = append(s.Targets, route)
	lead, err := s.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "waiting worker", route, true)
	if err != nil {
		t.Fatal(err)
	}
	if err = store.SetDurableWaits(ctx, worker.SessionID, true); err != nil {
		t.Fatal(err)
	}
	queued, err := s.Teams.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, "one task across waiting", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Teams.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, "must remain queued", nil); err != nil {
		t.Fatal(err)
	}
	if err = s.Teams.Resume(ctx, lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for s.Teams.Status(worker.ID).Active && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	// Obtain the actual persisted branch, never infer it from a product ID.
	detail, detailErr := s.Store.Detail(ctx, worker.SessionID)
	if detailErr != nil {
		t.Fatal(detailErr)
	}
	turn, err := store.BranchTurn(ctx, worker.SessionID, detail.Session.ActiveBranchID)
	if err != nil || turn.Status != engine.Waiting || s.Teams.Status(worker.ID).Active {
		t.Fatal("Worker did not park its actual Turn", turn, err)
	}
	_, _, done, err := runtime.ResumeWaitOwned(ctx, worker.SessionID, detail.Session.ActiveBranchID, turn.ID, 4)
	if err != nil {
		t.Fatal(err)
	}
	<-adapter.started
	stopCtx, cancel := context.WithTimeout(ctx, 40*time.Millisecond)
	defer cancel()
	stopped := make(chan error, 1)
	go func() { stopped <- s.Teams.Stop(stopCtx, lead.ID, team.Team.ID, worker.ID) }()
	<-adapter.cleanup
	if err = s.Teams.Resume(ctx, lead.ID, team.Team.ID, worker.ID); err == nil {
		t.Fatal("Resume reopened the queue while Stop was joining its owner")
	}
	if err = <-stopped; !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("Stop returned before resumed owner cleanup", err)
	}
	if !runtime.SessionActive(worker.SessionID) {
		t.Fatal("Stop dropped the still-cleaning physical owner")
	}
	page, err := s.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, worker.ID, 0, 8)
	if err != nil || len(page.Tasks) != 2 || page.Tasks[0].ID != queued.ID || page.Tasks[0].TurnID != turn.ID || page.Tasks[0].Status != hub.Running || page.Tasks[1].Status != hub.TaskQueued {
		t.Fatal("Stop fabricated terminal task facts before cleanup", page, err)
	}
	unblock()
	<-done
	if err = s.Teams.Stop(ctx, lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal("Stop could not acknowledge the actual exited owner", err)
	}
	page, err = s.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, worker.ID, 0, 8)
	if err != nil || page.Tasks[0].Status != engine.Cancelled || page.Tasks[1].Status != hub.TaskQueued || runtime.SessionActive(worker.SessionID) {
		t.Fatal("Stop replayed or failed to settle the actual task", page, err)
	}
}

func TestTeamActualWorkerQueueAndAdmissionContextDetach(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	lead, err := s.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.AddWorker(ctx, lead.ID, team.Team.ID, "unapproved", s.DefaultTarget, false); err == nil {
		t.Fatal("Worker billing route silently approved")
	}
	worker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "worker", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	for _, prompt := range []string{"first queued", "second queued"} {
		if _, err = s.Teams.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, prompt, nil); err != nil {
			t.Fatal(err)
		}
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("queueing started execution")
	}
	admission, cancel := context.WithCancel(ctx)
	if err = s.Teams.Resume(admission, lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	cancel()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		page, err := s.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, worker.ID, 0, 32)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Tasks) == 2 && page.Tasks[1].Status == engine.Succeeded && !s.Runtime.HasActive() {
			if page.Tasks[0].Outcome == nil || page.Tasks[0].Outcome.TurnID == page.Tasks[1].Outcome.TurnID {
				t.Fatal("not separate actual Turns")
			}
			first := <-requests
			second := <-requests
			if first.OwnerID != worker.SessionID || second.OwnerID != worker.SessionID || second.ResumeSession == "" {
				t.Fatal("Worker owner/native continuity lost")
			}
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("actual Worker FIFO did not terminate after observer detached")
}
func TestUploadReaderIsByteOnlyBoundedAndDoesNotAssertProcessing(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "bytes", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"../private", "a/b", "a\\b", "nul\x00name"} {
		if _, err = s.UploadReader(ctx, p.ID, name, strings.NewReader("bytes")); err == nil {
			t.Fatalf("unsafe multipart name accepted: %q", name)
		}
	}
	a, err := s.UploadReader(ctx, p.ID, "xfile0.txt", strings.NewReader("original input"))
	if err != nil || a.Info.Bytes != 14 || !strings.Contains(a.Info.Processing, "No compression") {
		t.Fatal("byte uploader changed original or processing", err)
	}
	if _, err = s.UploadReader(ctx, p.ID, "too-large.txt", strings.NewReader(strings.Repeat("x", (20<<20)+1))); err == nil {
		t.Fatal("reader exceeded 20 MiB")
	}
	if len(s.DraftAttachments(p.ID)) != 1 || s.Runtime.HasActive() {
		t.Fatal("failed upload altered draft or ran model")
	}
}
