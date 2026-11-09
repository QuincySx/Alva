// INPUT: A scripted stub engine (with and without interjection) registered in a real product service with private SQLite stores
// OUTPUT: Queue dispatch after success, pause after failure/stop, interjection delivery/downgrade/requeue, single dispatch under a race and stale-execution regressions
// POS: [L7 Hub product] Proves the product rules around the queue: when it starts the next Turn, when it stays paused, and that a message is sent exactly once
package product

import (
	"context"
	"errors"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/session"
)

// stubEngine runs one scripted execution per Execute call.
type stubEngine struct {
	mu       sync.Mutex
	caps     engine.Capabilities
	prompts  []string
	started  chan string
	gate     chan struct{} // an execution waits for one token; nil means it does not wait
	outcome  engine.Outcome
	deliver  bool
	pending  []engine.Interjection
	active   map[string]bool
	executed int
	forks    []*engine.ForkPoint
	natives  int
	turnSeq  int
	// failNext makes the next execution fail before it reports a native session.
	failNext bool
	// unreachable makes an execution that asks to fork report that the recorded
	// cut cannot be reached, before it emits anything.
	unreachable bool
	// checkErr is what CheckFork answers (the engine's own reachability check).
	checkErr error
}

func (s *stubEngine) CheckFork(context.Context, engine.ForkPoint) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.checkErr
}

func (s *stubEngine) Capabilities() engine.Capabilities { return s.caps }
func (s *stubEngine) Cancel(context.Context, string) error {
	return nil
}
func (s *stubEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (s *stubEngine) Interject(_ context.Context, id string, in engine.Interjection) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.active[id] {
		return engine.ErrSessionNotFound
	}
	s.pending = append(s.pending, in)
	return nil
}
func (s *stubEngine) Execute(ctx context.Context, request engine.Request, sink func(engine.Event) error) error {
	native := request.ResumeSession
	s.mu.Lock()
	if request.Fork != nil && s.unreachable {
		s.prompts = append(s.prompts, request.Prompt)
		s.forks = append(s.forks, request.Fork)
		s.executed++
		s.mu.Unlock()
		return engine.ErrForkUnreachable
	}
	if s.failNext {
		s.failNext = false
		s.prompts = append(s.prompts, request.Prompt)
		s.forks = append(s.forks, request.Fork)
		s.executed++
		s.mu.Unlock()
		return errors.New("scripted startup failure")
	}
	if native == "" {
		s.natives++
		native = "stub-native-" + strconv.Itoa(s.natives)
	}
	s.forks = append(s.forks, request.Fork)
	s.prompts = append(s.prompts, request.Prompt)
	s.executed++
	if s.active == nil {
		s.active = map[string]bool{}
	}
	s.active[native] = true
	gate, started, deliver, outcome := s.gate, s.started, s.deliver, s.outcome
	s.mu.Unlock()
	defer func() {
		s.mu.Lock()
		delete(s.active, native)
		s.mu.Unlock()
	}()
	if err := sink(engine.Event{Kind: engine.SessionStarted, SessionID: native}); err != nil {
		return err
	}
	if started != nil {
		started <- request.Prompt
	}
	if gate != nil {
		select {
		case <-gate:
		case <-ctx.Done():
			return sink(engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Cancelled})
		}
	}
	if deliver {
		s.mu.Lock()
		items := s.pending
		s.pending = nil
		s.mu.Unlock()
		for _, item := range items {
			if err := sink(engine.Event{Kind: engine.InputDelivered, SessionID: native, ID: item.ID, Text: item.Text}); err != nil {
				return err
			}
		}
	}
	if outcome == "" {
		outcome = engine.Succeeded
	}
	if outcome == engine.Failed {
		if err := sink(engine.Event{Kind: engine.Error, Text: "scripted failure", Failure: engine.FailureExecution}); err != nil {
			return err
		}
	}
	s.mu.Lock()
	s.turnSeq++
	turn := "stub-turn-" + strconv.Itoa(s.turnSeq)
	s.mu.Unlock()
	return sink(engine.Event{Kind: engine.Completed, SessionID: native, Outcome: outcome, NativeTurn: turn})
}

func (s *stubEngine) promptList() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]string(nil), s.prompts...)
}

func queueFixture(t *testing.T, stub *stubEngine) (*HubService, hub.Session) {
	t.Helper()
	ctx := context.Background()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := productstore.Open(ctx, filepath.Join(directory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	repo, err := session.Open(ctx, filepath.Join(directory, "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	runtime := hub.New(store)
	t.Cleanup(func() {
		closing, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = runtime.Close(closing)
	})
	if err = runtime.Register("stub", stub); err != nil {
		t.Fatal(err)
	}
	target := hub.Target{EngineID: "stub", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "no-billing"}
	service, err := NewHubService(store, runtime, repo, target, directory, true)
	if err != nil {
		t.Fatal(err)
	}
	created, err := service.Create(ctx, "queue", "")
	if err != nil {
		t.Fatal(err)
	}
	return service, created
}

func waitPrompt(t *testing.T, stub *stubEngine, want string) {
	t.Helper()
	select {
	case got := <-stub.started:
		if !strings.HasSuffix(got, want) {
			t.Fatalf("started %q, want %q", got, want)
		}
	case <-time.After(10 * time.Second):
		t.Fatalf("engine never started %q", want)
	}
}

func settled(t *testing.T, s *HubService, id string) engine.Outcome {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		detail, err := s.Store.Detail(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		if _, active := s.Runtime.ActiveExecution(id); !active && detail.Session.Status != hub.Running {
			return detail.Session.Status
		}
		time.Sleep(2 * time.Millisecond)
	}
	t.Fatal("conversation never settled")
	return ""
}

func queuedTexts(t *testing.T, s *HubService, id string) []string {
	t.Helper()
	items, err := s.Turns.Queued(context.Background(), id)
	if err != nil {
		t.Fatal(err)
	}
	var texts []string
	for _, item := range items {
		texts = append(texts, string(item.State)+":"+item.Text)
	}
	return texts
}

func TestQueueStartsTheHeadOnlyAfterASuccessfulTurn(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Streaming: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	if _, err := s.Turns.Send(ctx, c.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	for _, text := range []string{"second", "third"} {
		if _, err := s.Turns.Queue(ctx, c.ID, text); err != nil {
			t.Fatal(err)
		}
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 2 || got[0] != "queued:second" {
		t.Fatal("queue", got)
	}
	stub.gate <- struct{}{}
	waitPrompt(t, stub, "second")
	// The head left the queue in the admission that started it.
	if got := queuedTexts(t, s, c.ID); len(got) != 1 || got[0] != "queued:third" {
		t.Fatal("after dispatch", got)
	}
	stub.gate <- struct{}{}
	waitPrompt(t, stub, "third")
	stub.gate <- struct{}{}
	if settled(t, s, c.ID) != engine.Succeeded {
		t.Fatal("conversation must end successfully")
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 0 {
		t.Fatal("queue must be empty", got)
	}
	var prompts []string
	for _, p := range stub.promptList() {
		prompts = append(prompts, p[strings.LastIndex(p, "\n")+1:])
	}
	if strings.Join(prompts, ",") != "first,second,third" {
		t.Fatal("every message must be sent exactly once, in order", prompts)
	}
}

func TestQueueStaysPausedAfterAFailureOrAStopUntilTheUserResumes(t *testing.T) {
	for _, outcome := range []engine.Outcome{engine.Failed, engine.Cancelled} {
		t.Run(string(outcome), func(t *testing.T) {
			stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8), outcome: outcome}
			s, c := queueFixture(t, stub)
			ctx := context.Background()
			start, err := s.Turns.Send(ctx, c.ID, "first", nil)
			if err != nil {
				t.Fatal(err)
			}
			waitPrompt(t, stub, "first")
			if _, err = s.Turns.Queue(ctx, c.ID, "second"); err != nil {
				t.Fatal(err)
			}
			if outcome == engine.Cancelled {
				if err = s.Turns.Stop(ctx, c.ID, start.ExecutionID); err != nil {
					t.Fatal(err)
				}
			} else {
				stub.gate <- struct{}{}
			}
			if got := settled(t, s, c.ID); got != outcome {
				t.Fatal("outcome", got)
			}
			time.Sleep(150 * time.Millisecond)
			if stub.executed != 1 {
				t.Fatal("a failed or stopped Turn must not start the queue", stub.executed)
			}
			if got := queuedTexts(t, s, c.ID); len(got) != 1 || got[0] != "queued:second" {
				t.Fatal("the paused queue is kept", got)
			}
			stub.mu.Lock()
			stub.outcome = engine.Succeeded
			stub.mu.Unlock()
			if _, err = s.Turns.SendQueued(ctx, c.ID, ""); err != nil {
				t.Fatal(err)
			}
			waitPrompt(t, stub, "second")
			stub.gate <- struct{}{}
			if settled(t, s, c.ID) != engine.Succeeded || len(queuedTexts(t, s, c.ID)) != 0 {
				t.Fatal("explicit resume sends the head once")
			}
		})
	}
}

func TestInterjectDowngradesToTheQueueWhenTheEngineCannotTakeIt(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	if capabilities, err := s.Turns.Capabilities(ctx, c.ID); err != nil || capabilities.Interject {
		t.Fatal("this engine does not interject", capabilities, err)
	}
	start, err := s.Turns.Send(ctx, c.ID, "first", nil)
	if err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	result, err := s.Turns.Interject(ctx, c.ID, start.ExecutionID, "please also do X")
	if err != nil {
		t.Fatal(err)
	}
	if result.Mode != InterjectQueued || result.Notice != "当前引擎不支持运行中插话，已加入队列" || result.Input.State != hub.PendingQueued {
		t.Fatal("an engine without interjection must queue with a notice", result)
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 1 || got[0] != "queued:please also do X" {
		t.Fatal("queued", got)
	}
	stub.gate <- struct{}{}
	waitPrompt(t, stub, "please also do X")
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
	if got := queuedTexts(t, s, c.ID); len(got) != 0 {
		t.Fatal("the downgraded message is sent once and leaves the queue", got)
	}
}

func TestInterjectionIsHandedToTheEngineAndLeavesTheQueueOnItsReceipt(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Interjection: true}, started: make(chan string, 8), gate: make(chan struct{}, 8), deliver: true}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	if capabilities, err := s.Turns.Capabilities(ctx, c.ID); err != nil || !capabilities.Interject {
		t.Fatal("this engine interjects", capabilities, err)
	}
	start, err := s.Turns.Send(ctx, c.ID, "first", nil)
	if err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	result, err := s.Turns.Interject(ctx, c.ID, start.ExecutionID, "look at Y too")
	if err != nil || result.Mode != InterjectSent || result.Notice != "" {
		t.Fatal("interjection must be handed to the engine", result, err)
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 1 || got[0] != "interjecting:look at Y too" {
		t.Fatal("while with the engine it shows as interjecting", got)
	}
	// A stale execution identity never interjects into a different run.
	if _, err = s.Turns.Interject(ctx, c.ID, "attempt-stale", "wrong run"); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("a stale execution identity must be refused", err)
	}
	stub.gate <- struct{}{}
	if settled(t, s, c.ID) != engine.Succeeded {
		t.Fatal("run must succeed")
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 0 {
		t.Fatal("the delivery receipt removes the input from the queue", got)
	}
	if stub.executed != 1 {
		t.Fatal("a delivered interjection must not be sent again", stub.executed)
	}
}

func TestUndeliveredInterjectionReturnsToTheQueueAndIsSentOnce(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Interjection: true}, started: make(chan string, 8), gate: make(chan struct{}, 8), deliver: false}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	start, err := s.Turns.Send(ctx, c.ID, "first", nil)
	if err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	if result, e := s.Turns.Interject(ctx, c.ID, start.ExecutionID, "too late for this run"); e != nil || result.Mode != InterjectSent {
		t.Fatal(result, e)
	}
	stub.gate <- struct{}{}
	// The run ends without a receipt: the message is queued at the head and
	// the successful Turn starts it as the next one.
	waitPrompt(t, stub, "too late for this run")
	stub.mu.Lock()
	stub.deliver = true
	stub.mu.Unlock()
	stub.gate <- struct{}{}
	if settled(t, s, c.ID) != engine.Succeeded {
		t.Fatal("second run must succeed")
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 0 || stub.executed != 2 {
		t.Fatal("the message is sent exactly once, as its own Turn", got, stub.executed)
	}
}

func TestManualSendAndExitDispatchRaceStartsExactlyOneTurn(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	if _, err := s.Turns.Send(ctx, c.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	item, err := s.Turns.Queue(ctx, c.ID, "only once")
	if err != nil {
		t.Fatal(err)
	}
	stub.gate <- struct{}{}
	// Race the exit dispatcher with an explicit send of the same input.
	var wg sync.WaitGroup
	for i := 0; i < 4; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, _ = s.Turns.SendQueued(ctx, c.ID, item.ID)
		}()
	}
	wg.Wait()
	waitPrompt(t, stub, "only once")
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
	time.Sleep(100 * time.Millisecond)
	if stub.executed != 2 {
		t.Fatal("exactly one Turn must start from one queued input", stub.executed)
	}
}

func TestQueueRefusesInvalidAndOverflowingInput(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	if _, err := s.Turns.Send(ctx, c.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	for i := 0; i < hub.MaxPendingInputs; i++ {
		if _, err := s.Turns.Queue(ctx, c.ID, "x"); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.Turns.Queue(ctx, c.ID, "ninth"); !errors.Is(err, hub.ErrPendingFull) {
		t.Fatal("the ninth must be refused", err)
	}
	if _, err := s.Turns.Interject(ctx, c.ID, "", "tenth"); !errors.Is(err, hub.ErrPendingFull) {
		t.Fatal("a full queue refuses a downgraded interjection too", err)
	}
	if _, err := s.Turns.Interject(ctx, c.ID, "", strings.Repeat("x", hub.MaxPendingInputText+1)); err == nil {
		t.Fatal("oversize interjection")
	}
	if _, err := s.Turns.SendQueued(ctx, c.ID, ""); !errors.Is(err, hub.ErrSessionBusy) {
		t.Fatal("explicit send must wait for the running Turn", err)
	}
	stub.gate <- struct{}{}
	for i := 0; i < hub.MaxPendingInputs; i++ {
		waitPrompt(t, stub, "x")
		stub.gate <- struct{}{}
	}
	settled(t, s, c.ID)
}

func TestQueueOnAnIdleConversationStartsAtOnceUnlessPaused(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	// A fresh idle conversation has nothing to wait for.
	if _, err := s.Turns.Queue(ctx, c.ID, "first by queue"); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first by queue")
	stub.gate <- struct{}{}
	if settled(t, s, c.ID) != engine.Succeeded || len(queuedTexts(t, s, c.ID)) != 0 {
		t.Fatal("an idle queue must start its head and empty itself")
	}
	// After a failure the queue is paused: the message waits for an explicit send.
	stub.mu.Lock()
	stub.outcome = engine.Failed
	stub.mu.Unlock()
	if _, err := s.Turns.Send(ctx, c.ID, "will fail", nil); err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "will fail")
	stub.gate <- struct{}{}
	if settled(t, s, c.ID) != engine.Failed {
		t.Fatal("scripted failure expected")
	}
	if _, err := s.Turns.Queue(ctx, c.ID, "stay queued"); err != nil {
		t.Fatal(err)
	}
	time.Sleep(150 * time.Millisecond)
	if got := queuedTexts(t, s, c.ID); len(got) != 1 || got[0] != "queued:stay queued" || stub.executed != 2 {
		t.Fatal("a paused conversation keeps the message queued", got, stub.executed)
	}
	// A message queued behind another waiting one is not the head: nothing starts.
	if _, err := s.Turns.Queue(ctx, c.ID, "behind"); err != nil {
		t.Fatal(err)
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 2 || stub.executed != 2 {
		t.Fatal(got, stub.executed)
	}
}

func TestBlankMessagesAreRefusedBeforeTheyReachTheStore(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true, Interjection: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	ctx := context.Background()
	start, err := s.Turns.Send(ctx, c.ID, "first", nil)
	if err != nil {
		t.Fatal(err)
	}
	waitPrompt(t, stub, "first")
	for _, blank := range []string{"   ", "\n\t ", " "} {
		if _, err = s.Turns.Queue(ctx, c.ID, blank); err == nil {
			t.Fatalf("blank queued text %q was accepted", blank)
		}
		if _, err = s.Turns.Interject(ctx, c.ID, start.ExecutionID, blank); err == nil {
			t.Fatalf("blank interjection %q was accepted", blank)
		}
	}
	if got := queuedTexts(t, s, c.ID); len(got) != 0 {
		t.Fatal("a refused message left a row behind", got)
	}
	// An edited message may be as long as an ordinary prompt; a queued one may not.
	if err = hub.ValidTurnText(strings.Repeat("x", 40<<10)); err != nil {
		t.Fatal("an edit within 64 KiB must be allowed", err)
	}
	if hub.ValidTurnText(strings.Repeat("x", hub.MaxTurnText+1)) == nil || hub.ValidPendingText(strings.Repeat("x", 40<<10)) == nil {
		t.Fatal("limits: edit 64 KiB, queue 16 KiB")
	}
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
}

func TestEditAcceptsAnOrdinaryLongPrompt(t *testing.T) {
	stub := &stubEngine{caps: engine.Capabilities{Resume: true, Cancel: true}, started: make(chan string, 8), gate: make(chan struct{}, 8)}
	s, c := queueFixture(t, stub)
	first := sendOnce(t, stub, s, c.ID, "first")
	if _, err := s.Turns.Edit(context.Background(), c.ID, first.TurnID, strings.Repeat("长", 20<<10)); err != nil {
		t.Fatal("a 60 KiB edit is an ordinary prompt", err)
	}
	<-stub.started
	stub.gate <- struct{}{}
	settled(t, s, c.ID)
}
