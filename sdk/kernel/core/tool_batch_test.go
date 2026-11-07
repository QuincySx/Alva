// INPUT: context, encoding/json, fmt, sync, testing, time, kernel/abi, llm/wire
// OUTPUT: Scheduling conflict/wave boundary and per-call execution time regressions
// POS: [L2 kernel runtime] Pins explicit concurrency opt-in, global barriers, reader/writer conflicts and real per-call start/end facts.
package core

import (
	"context"
	"encoding/json"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type recordingLog struct {
	mu      sync.Mutex
	records []abi.SessionRecord
}

func (*recordingLog) ID() string                                       { return "timing" }
func (*recordingLog) Messages(context.Context) ([]wire.Message, error) { return nil, nil }
func (*recordingLog) Flush(context.Context) error                      { return nil }
func (log *recordingLog) AppendRecords(_ context.Context, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	log.mu.Lock()
	defer log.mu.Unlock()
	committed := abi.CloneRecords(records)
	for i := range committed {
		if err := committed[i].Validate(); err != nil {
			return nil, err
		}
		committed[i].Sequence = uint64(len(log.records) + 1)
		committed[i].ID = fmt.Sprintf("timing:%d", committed[i].Sequence)
		log.records = append(log.records, abi.CloneRecords(committed[i:i+1])...)
	}
	return committed, nil
}
func (log *recordingLog) outcomes() map[string]abi.ToolOutcome {
	log.mu.Lock()
	defer log.mu.Unlock()
	out := map[string]abi.ToolOutcome{}
	for _, record := range log.records {
		if record.ToolOutcome != nil {
			out[record.ToolOutcome.CallID] = *record.ToolOutcome
		}
	}
	return out
}

type sleepingTool struct {
	name  string
	delay time.Duration
}

func (tool sleepingTool) Name() string                 { return tool.name }
func (sleepingTool) Description() string               { return "timing fixture" }
func (sleepingTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (sleepingTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (sleepingTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (sleepingTool) IsReadOnly(wire.ToolCall) bool           { return true }
func (tool sleepingTool) Execute(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	select {
	case <-time.After(tool.delay):
	case <-ctx.Done():
		return wire.ToolResult{}, ctx.Err()
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: tool.name}}}, nil
}

func TestParallelWaveRecordsEachCallsActualExecutionTimes(t *testing.T) {
	slow, fast := sleepingTool{"slow", 200 * time.Millisecond}, sleepingTool{"fast", 0}
	tools := map[string]abi.Tool{"slow": slow, "fast": fast}
	calls := []wire.ToolCall{{ID: "slow-call", Name: "slow", Arguments: json.RawMessage(`{}`)}, {ID: "fast-call", Name: "fast", Arguments: json.RawMessage(`{}`)}, {ID: "prepared-call", Name: "fast", Arguments: json.RawMessage(`{}`)}}
	intents := make([]abi.SessionRecord, len(calls))
	for i, call := range calls {
		intents[i] = abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: call, Scheduling: abi.Scheduling{ConcurrentSafe: true}, NoSideEffect: call.ID == "prepared-call"}}
	}
	if got := waveLength(intents, 8, "/work"); got != len(calls) {
		t.Fatalf("fixture calls must share one wave, got %d", got)
	}
	log := &recordingLog{}
	prepared := map[string]wire.ToolResult{"prepared-call": {CallID: "prepared-call", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "prepared"}}}}
	before := time.Now()
	if err := (ToolBatchCoordinator{}).Execute(t.Context(), ToolBatchConfig{Session: log, Workspace: "/work", PreparedResults: prepared}, "run", calls, intents, tools, nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	after := time.Now()
	outcomes := log.outcomes()
	slowOutcome, fastOutcome, preparedOutcome := outcomes["slow-call"], outcomes["fast-call"], outcomes["prepared-call"]
	for name, outcome := range map[string]abi.ToolOutcome{"slow": slowOutcome, "fast": fastOutcome} {
		if outcome.StartedAt.IsZero() || outcome.EndedAt.IsZero() || outcome.EndedAt.Before(outcome.StartedAt) || outcome.StartedAt.Before(before) || outcome.EndedAt.After(after) {
			t.Fatalf("%s execution times invalid: %+v", name, outcome)
		}
	}
	if slowOutcome.EndedAt.Sub(slowOutcome.StartedAt) < 200*time.Millisecond {
		t.Fatal("slow call end does not reflect its actual execution", slowOutcome.StartedAt, slowOutcome.EndedAt)
	}
	if !fastOutcome.EndedAt.Before(slowOutcome.EndedAt.Add(-100 * time.Millisecond)) {
		t.Fatal("fast call recorded with the wave settlement time instead of its own end", fastOutcome.EndedAt, slowOutcome.EndedAt)
	}
	if preparedOutcome.Status != abi.OutcomeSucceeded || !preparedOutcome.StartedAt.IsZero() || !preparedOutcome.EndedAt.IsZero() {
		t.Fatal("prepared result invented execution times", preparedOutcome)
	}

	cancelled := &recordingLog{}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err := (ToolBatchCoordinator{}).Execute(ctx, ToolBatchConfig{Session: cancelled, Workspace: "/work"}, "run", calls[:2], intents[:2], tools, nil, func(abi.Event) error { return nil }); err == nil {
		t.Fatal("cancelled batch reported success")
	}
	unexecuted := cancelled.outcomes()
	if len(unexecuted) != 2 {
		t.Fatal("unexecuted calls not settled", unexecuted)
	}
	for id, outcome := range unexecuted {
		if outcome.Status != abi.OutcomeCancelled || !outcome.StartedAt.IsZero() || !outcome.EndedAt.IsZero() {
			t.Fatalf("unexecuted %s invented execution times: %+v", id, outcome)
		}
	}
}

func TestConflictAwareWaveComposition(t *testing.T) {
	read := abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "./a/../file", Access: abi.AccessRead}}}
	write := abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "/work/file", Access: abi.AccessWrite}}}
	barrier := abi.Scheduling{ConcurrentSafe: true, ExecutionMode: abi.SerialGlobal}
	for _, fixture := range []struct {
		name        string
		scheduling  []abi.Scheduling
		limit, want int
	}{{"readers", []abi.Scheduling{read, read}, 8, 2}, {"writer conflict", []abi.Scheduling{read, write}, 8, 1}, {"global", []abi.Scheduling{read, barrier}, 8, 1}, {"undeclared", []abi.Scheduling{{}, read}, 8, 1}, {"cap", []abi.Scheduling{read, read}, 1, 1}, {"coordinator", []abi.Scheduling{{ConcurrentSafe: true, ExecutionMode: abi.Coordinator}, read}, 8, 2}} {
		t.Run(fixture.name, func(t *testing.T) {
			records := make([]abi.SessionRecord, len(fixture.scheduling))
			for i, schedule := range fixture.scheduling {
				records[i].ToolIntent = &abi.ToolIntent{Scheduling: schedule}
			}
			if got := waveLength(records, fixture.limit, "/work"); got != fixture.want {
				t.Fatalf("wave=%d want=%d", got, fixture.want)
			}
		})
	}
}
