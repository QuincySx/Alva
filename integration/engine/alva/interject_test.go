// INPUT: Actual harness tool waves, an in-memory SDK session, a gated fixture Tool and recorded model requests
// OUTPUT: Interjection mailbox, BeforeModel commit after the tool result, delivery receipt order, late/unsupported interjection and fork-capability regressions
// POS: [L7 Alva product adapter] Proves an interjected message reaches the model only at a model boundary, after the tool result, and is reported delivered only after it is durable
package alva

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// gatedTool blocks until released, so a test can interject while a tool runs.
type gatedTool struct {
	started chan struct{}
	release chan struct{}
}

func (*gatedTool) Name() string                      { return "gated_tool" }
func (*gatedTool) Description() string               { return "Blocks until the test releases it." }
func (*gatedTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (g *gatedTool) Execute(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	close(g.started)
	select {
	case <-g.release:
	case <-ctx.Done():
		return wire.ToolResult{}, ctx.Err()
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "tool-output"}}}, nil
}
func (*gatedTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (*gatedTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Blocks."}
}
func (*gatedTool) IsReadOnly(wire.ToolCall) bool { return true }

type interjectRun struct {
	adapter  *Adapter
	log      *core.MemorySession
	requests [][]wire.Message
	mu       sync.Mutex
	tool     *gatedTool
}

func newInterjectRun(t *testing.T, options FactoryOptions) *interjectRun {
	t.Helper()
	run := &interjectRun{tool: &gatedTool{started: make(chan struct{}), release: make(chan struct{})}}
	run.adapter = NewConfigured(func(ctx context.Context, id string, request engine.Request, _ bool) (harness.Runner, error) {
		run.log = core.NewMemorySession(id)
		turn := 0
		model := modelFunc(func(_ context.Context, req abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			run.mu.Lock()
			run.requests = append(run.requests, wire.CloneMessages(req.Messages))
			turn++
			current := turn
			run.mu.Unlock()
			if current == 1 {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call-1", Name: "gated_tool", Arguments: json.RawMessage(`{}`)}}); err != nil {
					return err
				}
			} else if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "final"}); err != nil {
				return err
			}
			return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		})
		builder := harness.NewBuilder(model, run.log).Tool(run.tool)
		if request.TakeInterjections != nil {
			builder.Plugin(InterjectionPlugin(request.TakeInterjections, request.ReportDelivered))
		}
		return builder.Build(ctx)
	}, nil, options)
	return run
}

func lastRoles(messages []wire.Message) string {
	var roles []string
	for _, m := range messages {
		roles = append(roles, string(m.Role))
	}
	return strings.Join(roles, ",")
}

func TestInterjectionIsCommittedAfterTheToolResultThenReportedDelivered(t *testing.T) {
	run := newInterjectRun(t, FactoryOptions{Interjection: true})
	if !run.adapter.Capabilities().Interjection || run.adapter.Capabilities().Fork {
		t.Fatal("capabilities must report exactly what the factory declared", run.adapter.Capabilities())
	}
	var events []engine.Event
	var native string
	var mu sync.Mutex
	done := make(chan error, 1)
	go func() {
		var contract engine.StreamContract
		done <- run.adapter.Execute(context.Background(), engine.Request{Prompt: "start"}, func(event engine.Event) error {
			mu.Lock()
			defer mu.Unlock()
			if event.Kind == engine.SessionStarted {
				native = event.SessionID
			}
			events = append(events, event)
			return contract.Accept(event)
		})
	}()
	select {
	case <-run.tool.started:
	case <-time.After(10 * time.Second):
		t.Fatal("tool never started")
	}
	mu.Lock()
	id := native
	mu.Unlock()
	if err := run.adapter.Interject(context.Background(), id, engine.Interjection{ID: "input-1", Text: "also check the second file"}); err != nil {
		t.Fatal(err)
	}
	close(run.tool.release)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if len(run.requests) != 2 {
		t.Fatal("two model requests expected", len(run.requests))
	}
	first, second := run.requests[0], run.requests[1]
	for _, m := range first {
		if strings.Contains(m.Text(), "second file") {
			t.Fatal("the interjection must not reach the request that was already sent")
		}
	}
	// The interjection comes after the tool result in the second request, as
	// the last message, and is the only user message after the prompt.
	if got := lastRoles(second); !strings.HasSuffix(got, "assistant,tool,user") {
		t.Fatal("interjection must follow the tool result", got)
	}
	if second[len(second)-1].Text() != "also check the second file" || second[len(second)-1].ID != InterjectionMessagePrefix+"input-1" {
		t.Fatal("interjected message", second[len(second)-1])
	}
	// It is durable in the native log, and the receipt came after it.
	records, err := run.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	logged := false
	for _, r := range records {
		logged = logged || r.Message != nil && r.Message.ID == InterjectionMessagePrefix+"input-1"
	}
	if !logged {
		t.Fatal("the interjected message must be in the native log")
	}
	mu.Lock()
	defer mu.Unlock()
	delivered := 0
	for _, e := range events {
		if e.Kind == engine.InputDelivered {
			delivered++
			if e.ID != "input-1" || e.Text != "also check the second file" || e.SessionID != id {
				t.Fatal("receipt identity", e)
			}
		}
	}
	if delivered != 1 {
		t.Fatal("exactly one delivery receipt expected", delivered)
	}
	// Nothing may be interjected into a finished execution.
	if err := run.adapter.Interject(context.Background(), id, engine.Interjection{ID: "late", Text: "late"}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("a finished execution has no mailbox", err)
	}
}

func TestInterjectionWithoutDeclaredCapabilityIsUnsupportedAndLateOnesAreNeverDelivered(t *testing.T) {
	plain := newInterjectRun(t, FactoryOptions{})
	if plain.adapter.Capabilities().Interjection {
		t.Fatal("a factory that did not declare interjection must not advertise it")
	}
	if err := plain.adapter.Interject(context.Background(), "native-x", engine.Interjection{ID: "i", Text: "t"}); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal(err)
	}
	// An interjection queued after the last model boundary is simply not
	// delivered: no receipt is emitted and the run still completes.
	run := newInterjectRun(t, FactoryOptions{Interjection: true})
	var native string
	var mu sync.Mutex
	var receipts int
	done := make(chan error, 1)
	go func() {
		done <- run.adapter.Execute(context.Background(), engine.Request{Prompt: "start"}, func(event engine.Event) error {
			mu.Lock()
			defer mu.Unlock()
			if event.Kind == engine.SessionStarted {
				native = event.SessionID
			}
			if event.Kind == engine.InputDelivered {
				receipts++
			}
			return nil
		})
	}()
	<-run.tool.started
	close(run.tool.release)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	id := native
	mu.Unlock()
	if err := run.adapter.Interject(context.Background(), id, engine.Interjection{ID: "late", Text: "late"}); err == nil {
		t.Fatal("a finished execution must refuse")
	}
	if receipts != 0 {
		t.Fatal("no receipt without delivery", receipts)
	}
	for _, bad := range []engine.Interjection{{ID: "", Text: "x"}, {ID: "i", Text: "  "}, {ID: "i", Text: strings.Repeat("x", engine.MaxInterjectionText+1)}} {
		if err := run.adapter.Interject(context.Background(), "native-x", bad); err == nil || errors.Is(err, engine.ErrSessionNotFound) {
			t.Fatal("an invalid interjection must be rejected before lookup", bad.ID, err)
		}
	}
}

func TestInboxIsBoundedAndSeveralInterjectionsKeepTheirOrder(t *testing.T) {
	box := &inbox{}
	for i := 0; i < maxInbox; i++ {
		if err := box.put(engine.Interjection{ID: string(rune('a' + i)), Text: "t"}); err != nil {
			t.Fatal(err)
		}
	}
	if err := box.put(engine.Interjection{ID: "z", Text: "t"}); err == nil {
		t.Fatal("the mailbox is bounded")
	}
	items := box.take()
	if len(items) != maxInbox || items[0].ID != "a" || items[maxInbox-1].ID != string(rune('a'+maxInbox-1)) {
		t.Fatal("order", items)
	}
	box.close()
	if err := box.put(engine.Interjection{ID: "y", Text: "t"}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("a closed mailbox refuses", err)
	}
}

func TestForkRequestNeedsTheDeclaredCapabilityAndExcludesResume(t *testing.T) {
	plain := New(func(context.Context, string, engine.Request, bool) (harness.Runner, error) {
		t.Fatal("factory must not run for an unsupported fork")
		return nil, nil
	})
	err := plain.Execute(context.Background(), engine.Request{Prompt: "p", Fork: &engine.ForkPoint{Session: "native-a", DropTurns: 1}}, func(engine.Event) error { return nil })
	if !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("fork without the capability", err)
	}
	forking := NewConfigured(func(context.Context, string, engine.Request, bool) (harness.Runner, error) {
		t.Fatal("factory must not run for an invalid fork")
		return nil, nil
	}, nil, FactoryOptions{Fork: true})
	if !forking.Capabilities().Fork {
		t.Fatal("declared fork")
	}
	if err = forking.Execute(context.Background(), engine.Request{Prompt: "p", ResumeSession: "native-b", Fork: &engine.ForkPoint{Session: "native-a"}}, func(engine.Event) error { return nil }); err == nil {
		t.Fatal("fork and resume are exclusive")
	}
	if err = forking.Execute(context.Background(), engine.Request{Prompt: "p", Fork: &engine.ForkPoint{Session: "bad id!"}}, func(engine.Event) error { return nil }); err == nil {
		t.Fatal("fork source must be a bounded identity")
	}
}

func TestAnUnreachableForkReportsNothingSoTheProductCanDegradeInPlace(t *testing.T) {
	a := NewConfigured(func(context.Context, string, engine.Request, bool) (harness.Runner, error) {
		return nil, engine.ErrForkUnreachable
	}, nil, FactoryOptions{Fork: true})
	var events []engine.Event
	err := a.Execute(context.Background(), engine.Request{Prompt: "p", Fork: &engine.ForkPoint{Session: "native-a", BeforeSequence: 4}}, func(event engine.Event) error {
		events = append(events, event)
		return nil
	})
	if !errors.Is(err, engine.ErrForkUnreachable) || len(events) != 0 {
		t.Fatal("an unreachable fork must fail before reporting anything", err, events)
	}
	// Any other factory failure is still reported as an Error and a failed completion.
	other := NewConfigured(func(context.Context, string, engine.Request, bool) (harness.Runner, error) {
		return nil, errors.New("boom")
	}, nil, FactoryOptions{Fork: true})
	events = nil
	_ = other.Execute(context.Background(), engine.Request{Prompt: "p", Fork: &engine.ForkPoint{Session: "native-a", BeforeSequence: 4}}, func(event engine.Event) error {
		events = append(events, event)
		return nil
	})
	if len(events) != 2 || events[1].Outcome != engine.Failed {
		t.Fatal("ordinary failures are still reported", events)
	}
}
