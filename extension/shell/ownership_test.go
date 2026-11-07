// INPUT: Actual native SQLite facts beyond the 4096-record restore budget, counting/fault-injecting storage wrappers and real shell tools
// OUTPUT: Snapshot-based recovery, one-time paged pre-snapshot rebuild equal to the former full scan, and atomic fact+snapshot regressions
// POS: [L4 extension] Proves first shell binding in long sessions reads one snapshot, never the whole history, and never splits ownership evidence.
package shell

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/storage"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	kernel "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// countingRepository observes whether recovery paged the history.
type countingRepository struct {
	storage.Repository
	pages *atomic.Int32
}

func (r countingRepository) Page(ctx context.Context, owner, id string, after uint64, limit int, highWater uint64) (storage.RecordPage, error) {
	r.pages.Add(1)
	return r.Repository.Page(ctx, owner, id, after, limit, highWater)
}

// faultRepository hands the plugin a session whose snapshot commits can be
// made to fail inside the real native transaction, and which reports any
// ownership fact written outside the snapshot compare-and-append.
type faultRepository struct {
	storage.Repository
	fail   *atomic.Bool
	bypass *atomic.Int32
}

type faultSession struct {
	storage.Session
	fail   *atomic.Bool
	bypass *atomic.Int32
}

func (r faultRepository) OpenSession(ctx context.Context, owner, id string) (storage.Session, error) {
	inner, err := r.Repository.OpenSession(ctx, owner, id)
	if err != nil {
		return nil, err
	}
	return faultSession{Session: inner, fail: r.fail, bypass: r.bypass}, nil
}
func (s faultSession) AppendRecords(ctx context.Context, records []kernel.SessionRecord) ([]kernel.SessionRecord, error) {
	for _, record := range records {
		if record.Namespace == shellNamespace && (ownershipKind(record.Type) || record.Type == pendingJobsKind) {
			s.bypass.Add(1)
		}
	}
	return s.Session.AppendRecords(ctx, records)
}
func (s faultSession) AppendExtensionSnapshot(ctx context.Context, namespace, kind string, expected uint64, payload json.RawMessage, additional ...kernel.SessionRecord) (kernel.SessionRecord, error) {
	if s.fail.Load() {
		// An invalid trailing record makes the store reject the whole batch.
		additional = append(slices.Clone(additional), kernel.SessionRecord{Kind: kernel.RecordExtension, Namespace: "alva.kernel", Type: "injected_failure", SchemaVersion: 1, Payload: json.RawMessage(`{}`)})
	}
	return s.Session.AppendExtensionSnapshot(ctx, namespace, kind, expected, payload, additional...)
}

func extensionRecord(t *testing.T, namespace, kind string, value any) kernel.SessionRecord {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	return kernel.SessionRecord{Kind: kernel.RecordExtension, Namespace: namespace, Type: kind, SchemaVersion: 1, Payload: data}
}

// appendFiller pushes the session past the native 4096-record / full-read budget.
func appendFiller(t *testing.T, log storage.Session, count int) {
	t.Helper()
	for written := 0; written < count; {
		batch := make([]kernel.SessionRecord, 0, 1000)
		for ; written < count && len(batch) < 1000; written++ {
			batch = append(batch, extensionRecord(t, "test.filler", "noise", map[string]int{"n": written}))
		}
		if _, err := log.AppendRecords(t.Context(), batch); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := log.Records(t.Context(), 0); err == nil {
		t.Fatal("fixture session is still within the whole-history read budget")
	}
}

// legacyPendingJobs is the former whole-session recoverInterrupted scan, kept
// only as the equivalence oracle for pre-snapshot sessions.
func legacyPendingJobs(records []kernel.SessionRecord) ([]string, error) {
	pending := map[string]bool{}
	for _, record := range records {
		if record.Kind != kernel.RecordExtension || record.Namespace != "alva.shell" {
			continue
		}
		var value struct {
			Task struct {
				ID string `json:"id"`
			} `json:"task"`
			Status json.RawMessage `json:"status"`
			JobID  string          `json:"job_id"`
		}
		if json.Unmarshal(record.Payload, &value) != nil {
			return nil, errors.New("invalid saved shell ownership fact")
		}
		switch record.Type {
		case "job_intent":
			if value.Task.ID != "" {
				pending[value.Task.ID] = true
			}
		case "job_outcome":
			var status struct {
				Task struct {
					ID string `json:"id"`
				} `json:"task"`
			}
			if json.Unmarshal(value.Status, &status) != nil {
				return nil, errors.New("invalid saved shell outcome")
			}
			delete(pending, status.Task.ID)
		case "job_start_failed", "job_abandoned":
			delete(pending, value.JobID)
		}
	}
	return sortedJobIDs(pending), nil
}

func latestShellPayload(t *testing.T, f *fixture, kind string) (json.RawMessage, bool) {
	t.Helper()
	entry, found, err := f.repo.LatestRecord(t.Context(), "owner", "native", shellNamespace, kind)
	if err != nil {
		t.Fatal(err)
	}
	if !found {
		return nil, false
	}
	_, data, err := f.repo.ReadArtifactFull(t.Context(), "owner", "native", entry.Value.URI, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	var record kernel.SessionRecord
	if err := json.Unmarshal(data, &record); err != nil {
		t.Fatal(err)
	}
	return record.Payload, true
}

func pendingSnapshot(t *testing.T, f *fixture) []string {
	t.Helper()
	payload, found := latestShellPayload(t, f, pendingJobsKind)
	if !found {
		t.Fatal("pending_jobs snapshot missing")
	}
	var ids []string
	if err := json.Unmarshal(payload, &ids); err != nil || ids == nil {
		t.Fatal("invalid pending_jobs payload", string(payload), err)
	}
	return ids
}

func countShellType(t *testing.T, f *fixture, kind string) int {
	t.Helper()
	count := 0
	var after, highWater uint64
	for {
		page, err := f.repo.Page(t.Context(), "owner", "native", after, 100, highWater)
		if err != nil {
			t.Fatal(err)
		}
		highWater = page.HighWater
		for _, entry := range page.Entries {
			if entry.Namespace == shellNamespace && entry.Type == kind {
				count++
			}
		}
		if !page.HasMore {
			return count
		}
		after = page.Next
	}
}

func bindOnce(t *testing.T, config Config) (*Lease, int32) {
	t.Helper()
	pages := &atomic.Int32{}
	config.Repository = countingRepository{Repository: config.Repository.(storage.Repository), pages: pages}
	s := NewSessions()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := s.Close(ctx); err != nil && !kernel.OnlyCancellation(err) {
			t.Error(err)
		}
	})
	lease, err := s.Bind(t.Context(), config)
	if err != nil {
		t.Fatal("first shell binding failed", err)
	}
	if err := lease.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	return lease, pages.Load()
}

func TestLongSessionFirstBindRecoversFromSnapshotWithoutPaging(t *testing.T) {
	f := newFixture(t)
	if err := f.p.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
		t.Fatal(err)
	}
	task, err := agentabi.NewTask(agentabi.TaskLocalBash, "interrupted fixture", "long-call", "")
	if err != nil {
		t.Fatal(err)
	}
	if err := f.p.append(t.Context(), "job_intent", map[string]any{"task": task, "run_id": "interrupted-run"}); err != nil {
		t.Fatal(err)
	}
	if ids := pendingSnapshot(t, f); !slices.Equal(ids, []string{task.ID}) {
		t.Fatal("job intent did not update the snapshot", ids)
	}
	appendFiller(t, f.log, 4200)
	lease, pages := bindOnce(t, f.p.config)
	if pages != 0 {
		t.Fatal("snapshot session still paged its history", pages)
	}
	if !slices.Equal(lease.Plugin.recoveredUnknown, []string{task.ID}) {
		t.Fatal("wrong interrupted jobs recovered", lease.Plugin.recoveredUnknown)
	}
	if ids := pendingSnapshot(t, f); len(ids) != 0 {
		t.Fatal("recovered jobs still pending", ids)
	}
	payload, found := latestShellPayload(t, f, "job_abandoned")
	if !found || !json.Valid(payload) {
		t.Fatal("abandonment fact missing")
	}
	var abandoned struct {
		JobID  string `json:"job_id"`
		Status string `json:"status"`
	}
	if err := json.Unmarshal(payload, &abandoned); err != nil || abandoned.JobID != task.ID || abandoned.Status != "unknown" {
		t.Fatal("abandonment fact is not an unknown outcome for the interrupted job", string(payload))
	}
	again, pages := bindOnce(t, f.p.config)
	if pages != 0 || len(again.Plugin.recoveredUnknown) != 0 || countShellType(t, f, "job_abandoned") != 1 {
		t.Fatal("second recovery paged, re-abandoned or duplicated", pages, again.Plugin.recoveredUnknown)
	}
}

func TestPreSnapshotSessionPagesOnceLikeLegacyScanThenWritesSnapshot(t *testing.T) {
	f := newFixture(t)
	if err := f.p.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
		t.Fatal(err)
	}
	ids := map[string]string{}
	for _, name := range []string{"A", "B", "C", "D", "E"} {
		task, err := agentabi.NewTask(agentabi.TaskLocalBash, "legacy "+name, "legacy-"+name, "")
		if err != nil {
			t.Fatal(err)
		}
		ids[name] = task.ID
		if _, err := f.log.AppendRecords(t.Context(), []kernel.SessionRecord{extensionRecord(t, shellNamespace, "job_intent", map[string]any{"task": task, "run_id": "old-run", "call_id": "legacy-" + name})}); err != nil {
			t.Fatal(err)
		}
	}
	// Ownership facts written directly, exactly as before snapshots existed;
	// unrelated shell facts and other namespaces are interleaved.
	finished := agentabi.Task{ID: ids["B"]}
	for _, record := range []kernel.SessionRecord{
		extensionRecord(t, shellNamespace, "output_frame", map[string]any{"job_id": ids["B"], "frame": map[string]int{"sequence": 1}}),
		extensionRecord(t, shellNamespace, "job_outcome", map[string]any{"run_id": "old-run", "status": Status{Task: finished}}),
		extensionRecord(t, shellNamespace, "job_start_failed", map[string]any{"job_id": ids["C"], "started": false}),
		extensionRecord(t, shellNamespace, "operation_intent", map[string]any{"run_id": "old-run", "tool": "execute_shell"}),
		extensionRecord(t, "other.plugin", "job_abandoned", map[string]any{"job_id": ids["A"]}),
		extensionRecord(t, shellNamespace, "job_abandoned", map[string]any{"job_id": ids["D"], "status": "unknown"}),
		extensionRecord(t, shellNamespace, "stdin_intent", map[string]any{"job_id": ids["E"], "bytes": 1}),
	} {
		if _, err := f.log.AppendRecords(t.Context(), []kernel.SessionRecord{record}); err != nil {
			t.Fatal(err)
		}
	}
	small, err := f.log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	want, err := legacyPendingJobs(small)
	if err != nil {
		t.Fatal(err)
	}
	if expected := sortedJobIDs(map[string]bool{ids["A"]: true, ids["E"]: true}); !slices.Equal(want, expected) {
		t.Fatal("legacy oracle fixture drifted", want, expected)
	}
	if _, found := latestShellPayload(t, f, pendingJobsKind); found {
		t.Fatal("pre-snapshot fixture already has a snapshot")
	}
	appendFiller(t, f.log, 4200)
	lease, pages := bindOnce(t, f.p.config)
	if pages < 40 {
		t.Fatal("pre-snapshot session did not page its whole history once", pages)
	}
	if !slices.Equal(lease.Plugin.recoveredUnknown, want) {
		t.Fatal("paged rebuild differs from the former full scan", lease.Plugin.recoveredUnknown, want)
	}
	if snapshot := pendingSnapshot(t, f); len(snapshot) != 0 {
		t.Fatal("first snapshot does not reflect recovery", snapshot)
	}
	if count := countShellType(t, f, "job_abandoned"); count != 1+len(want) {
		t.Fatal("abandonment facts missing or duplicated", count)
	}
	again, pages := bindOnce(t, f.p.config)
	if pages != 0 || len(again.Plugin.recoveredUnknown) != 0 || countShellType(t, f, "job_abandoned") != 1+len(want) {
		t.Fatal("snapshot written after rebuild was not used", pages, again.Plugin.recoveredUnknown)
	}
}

func TestPreSnapshotSessionWithoutPendingJobsStillWritesFirstSnapshot(t *testing.T) {
	f := newFixture(t)
	if err := f.p.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
		t.Fatal(err)
	}
	lease, pages := bindOnce(t, f.p.config)
	if pages == 0 || len(lease.Plugin.recoveredUnknown) != 0 {
		t.Fatal("fresh session recovery", pages, lease.Plugin.recoveredUnknown)
	}
	if snapshot := pendingSnapshot(t, f); len(snapshot) != 0 || countShellType(t, f, "job_abandoned") != 0 {
		t.Fatal("empty first snapshot missing or invented abandonment", snapshot)
	}
	if _, pages := bindOnce(t, f.p.config); pages != 0 {
		t.Fatal("empty snapshot not used by the next recovery", pages)
	}
}

func TestOwnershipFactAndSnapshotCommitAtomically(t *testing.T) {
	f := newFixture(t)
	fail, bypass := &atomic.Bool{}, &atomic.Int32{}
	config := f.p.config
	config.Repository = faultRepository{Repository: f.repo, fail: fail, bypass: bypass}
	p, err := New(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 6*time.Second)
		defer cancel()
		if err := p.Close(ctx); err != nil && !kernel.OnlyCancellation(err) {
			t.Error(err)
		}
	})
	if err := p.OnRunStart(t.Context(), f.runtime); err != nil {
		t.Fatal(err)
	}
	execute := func(id string) (wire.ToolResult, error) {
		for _, tool := range p.Tools() {
			if tool.Name() == "execute_shell" {
				return tool.Execute(t.Context(), kernel.ToolExecutionContext{ActualTool: tool, Session: f.log, SessionID: "native", RunID: "run", CallID: id, Workspace: f.cwd}, wire.ToolCall{ID: id, Name: "execute_shell", Arguments: json.RawMessage(`{"command":"printf atomic"}`)})
			}
		}
		t.Fatal("execute_shell unavailable")
		return wire.ToolResult{}, nil
	}
	fail.Store(true)
	if _, err := execute("atomic-fail"); err == nil {
		t.Fatal("launch succeeded although its ownership commit failed")
	}
	p.mu.Lock()
	jobs := len(p.jobs)
	p.mu.Unlock()
	if jobs != 0 {
		t.Fatal("process started without committed ownership intent")
	}
	if _, found := latestShellPayload(t, f, "job_intent"); found {
		t.Fatal("job intent committed without its snapshot")
	}
	if _, found := latestShellPayload(t, f, pendingJobsKind); found {
		t.Fatal("snapshot committed without its job intent")
	}
	fail.Store(false)
	result, err := execute("atomic-ok")
	if err != nil {
		t.Fatal(err)
	}
	id := jobID(t, result)
	intents, outcomes := countShellType(t, f, "job_intent"), countShellType(t, f, "job_outcome")
	if intents != 1 || outcomes != 1 {
		t.Fatal("fact counts after recovery from a failed commit", intents, outcomes)
	}
	if snapshot := pendingSnapshot(t, f); len(snapshot) != 0 {
		t.Fatal("completed foreground job still pending", snapshot, id)
	}
	if countShellType(t, f, pendingJobsKind) != 2 {
		t.Fatal("each ownership fact must carry exactly one snapshot")
	}
	if bypass.Load() != 0 {
		t.Fatal(fmt.Sprintf("%d ownership facts bypassed the snapshot compare-and-append", bypass.Load()))
	}
}
