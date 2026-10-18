// INPUT: context, encoding/json, errors, sync/atomic, testing, time, agent/core/context/security, kernel/abi.
// OUTPUT: Strict grants, cancellation/settlement, callbacks, output boundaries and bounded child-model regressions.
// POS: [L4 extension] SubAgent contract acceptance with explicit local child factories and no paid models or process startup.
package subagent

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type localModel struct{}

func (localModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

type runFunc func(context.Context, []wire.Message, func(abi.Event) error) error

func (fn runFunc) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	return fn(ctx, input, emit)
}
func setup(t *testing.T, run runFunc, closeChild func(context.Context) error) (*Plugin, *agentcontext.MemoryReferences, abi.ToolExecutionContext) {
	t.Helper()
	ctx := context.Background()
	log := core.NewMemorySession("parent")
	refs, err := agentcontext.NewMemoryReferences("parent", 2<<20)
	if err != nil {
		t.Fatal(err)
	}
	guard, err := security.NewGuard(t.TempDir(), nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	plugin, err := New(Config{SessionID: log.ID(), References: refs, Guard: guard, Timeout: time.Second, MaxIterations: 5, MaxDepth: 2, Factory: ChildFactoryFunc(func(_ context.Context, request ChildRequest) (Child, error) {
		return Child{Runner: run, Session: core.NewMemorySession(request.Scope.SessionID()), Link: abi.RecordReference{SessionID: "parent", RecordID: "parent:1", Sequence: 1}, Close: closeChild}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	state, err := abi.NewAgentState(localModel{}, log, []abi.Tool{plugin.Tool()})
	if err != nil {
		t.Fatal(err)
	}
	if err = plugin.OnRunStart(ctx, abi.RuntimeContext{RunID: "run", State: state}); err != nil {
		t.Fatal(err)
	}
	return plugin, refs, abi.ToolExecutionContext{SessionID: "parent", Session: log, RunID: "run", CallID: "call", Workspace: t.TempDir(), ActualTool: plugin.Tool()}
}
func execute(t *testing.T, p *Plugin, execution abi.ToolExecutionContext, args string) (wire.ToolResult, error) {
	t.Helper()
	return p.Tool().Execute(context.Background(), execution, wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(args)})
}

func TestChildTimeoutOnlyRecoverableWithoutSettlementOrCleanupFailure(t *testing.T) {
	for _, test := range []struct {
		name             string
		runErr, closeErr error
		wantFatal        bool
	}{
		{"pure timeout", context.DeadlineExceeded, nil, false},
		{"cancel and commit failure", errors.Join(context.Canceled, &abi.SessionError{Operation: "append", SessionID: "actual-child", Err: errors.New("disk failed")}), nil, true},
		{"typed commit timeout", &abi.SessionError{Operation: "flush", SessionID: "actual-child", Err: context.DeadlineExceeded}, nil, true},
		{"cleanup deadline", context.DeadlineExceeded, context.DeadlineExceeded, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			p, _, execution := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error { return test.runErr }, func(ctx context.Context) error {
				if ctx.Err() != nil {
					t.Fatal("cleanup reused cancelled child context")
				}
				return test.closeErr
			})
			result, err := execute(t, p, execution, `{"task":"work"}`)
			if (err != nil) != test.wantFatal || !result.IsError {
				t.Fatalf("failure classification: result=%+v err=%v", result, err)
			}
		})
	}
}
func TestParentCancellationWaitsInlineForActualChildExit(t *testing.T) {
	started, observed, release := make(chan struct{}), make(chan struct{}), make(chan struct{})
	p, _, execution := setup(t, func(ctx context.Context, _ []wire.Message, _ func(abi.Event) error) error {
		close(started)
		<-ctx.Done()
		close(observed)
		<-release
		return ctx.Err()
	}, nil)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		_, err := p.Tool().Execute(ctx, execution, wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(`{"task":"work"}`)})
		done <- err
	}()
	<-started
	cancel()
	<-observed
	select {
	case err := <-done:
		t.Fatal("reported termination before child exit", err)
	default:
	}
	close(release)
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
func TestSelectedOutputBoundariesRefusalAndForgedSource(t *testing.T) {
	p, refs, execution := setup(t, func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		for _, event := range []abi.Event{{Kind: abi.EventTextDelta, Text: "pre-tool commentary"}, {Kind: abi.EventToolStarted}, {Kind: abi.EventReasoningDelta, Text: "private reasoning"}, {Kind: abi.EventContentBlock, ContentBlock: &wire.ContentBlock{Kind: wire.ContentRefusal, Refusal: &wire.Refusal{Text: "cannot comply🙂"}}}} {
			if err := emit(event); err != nil {
				return err
			}
		}
		return nil
	}, nil)
	result, err := execute(t, p, execution, `{"task":"work"}`)
	if err != nil || !result.IsError {
		t.Fatal(result, err)
	}
	var payload struct {
		Selected *wire.Reference `json:"selected_output"`
		Refused  bool            `json:"refused"`
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &payload); err != nil {
		t.Fatal(err)
	}
	if !payload.Refused || payload.Selected == nil {
		t.Fatal("refusal falsely reported as empty success")
	}
	page, err := refs.ReadText(context.Background(), payload.Selected.URI, 0, 4096)
	if err != nil || page.Text != "[Refusal] cannot comply🙂" {
		t.Fatal(page, err)
	}
	p, _, execution = setup(t, func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		return emit(abi.Event{Kind: abi.EventToolCompleted, Record: &abi.RecordReference{SessionID: "parent"}})
	}, nil)
	if _, err = execute(t, p, execution, `{"task":"work"}`); err == nil {
		t.Fatal("foreign source accepted")
	}
}
func TestMalformedInputAndUnknownGrantsNeverPrepareChild(t *testing.T) {
	p, _, execution := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error {
		return errors.New("should not run")
	}, nil)
	var prepared atomic.Int32
	p.config.Factory = ChildFactoryFunc(func(context.Context, ChildRequest) (Child, error) {
		prepared.Add(1)
		return Child{}, errors.New("should not prepare")
	})
	for _, input := range []string{`null`, `{"task":"work","tools":null}`, `{"task":"work","tools":["write_file"]}`, `{"task":"work","tools":["agent"]}`, `{"task":"work","unknown":1}`, `{"task":"work","comms":[{"kind":"unknown","config":{}}]}`, `{"task":"work","timeout_ms":1001}`, `{"task":"work","max_iterations":6}`} {
		if _, err := execute(t, p, execution, input); err == nil {
			t.Fatalf("malformed/widened request accepted: %s", input)
		}
	}
	if prepared.Load() != 0 {
		t.Fatal("invalid request started child")
	}
	foreign := execution
	foreign.Session = core.NewMemorySession("foreign")
	if _, err := execute(t, p, foreign, `{"task":"work"}`); err == nil {
		t.Fatal("foreign parent session accepted")
	}
}

type completionFunc func(context.Context, agentabi.SpawnResult) error

func (fn completionFunc) Call(ctx context.Context, result agentabi.SpawnResult) error {
	return fn(ctx, result)
}

type completionComm struct{}

func (completionComm) Kind() string                  { return "completion" }
func (completionComm) Description() string           { return "fixture" }
func (completionComm) ConfigSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (completionComm) Attach(_ context.Context, ctx agentabi.SpawnCommContext, _ json.RawMessage) (agentabi.SpawnCommHandle, error) {
	if ctx.ParentSessionID == ctx.ChildSessionID {
		return agentabi.SpawnCommHandle{}, errors.New("false child identity")
	}
	return agentabi.SpawnCommHandle{OnComplete: completionFunc(func(context.Context, agentabi.SpawnResult) error { return context.DeadlineExceeded })}, nil
}
func TestCompletionFailureNeverMasqueradesAsRecoverableChildTimeout(t *testing.T) {
	p, _, execution := setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error { return nil }, nil)
	registry, err := agentabi.NewCommunicationRegistry(4)
	if err != nil {
		t.Fatal(err)
	}
	if err = registry.Register(completionComm{}); err != nil {
		t.Fatal(err)
	}
	p.config.Communications = registry
	if _, err = execute(t, p, execution, `{"task":"work","comms":[{"kind":"completion","config":{}}]}`); !errors.Is(err, ErrChildCompletion) {
		t.Fatal("callback deadline swallowed", err)
	}
}
func TestModelBudgetBoundsStreamsWithoutHoldingParentCoordinator(t *testing.T) {
	budget, _ := NewBudget(8, 2)
	middleware := ModelBudgetMiddleware{Budget: budget}
	var current, maximum atomic.Int32
	entered := make(chan struct{}, 4)
	release := make(chan struct{})
	var group sync.WaitGroup
	for range 4 {
		group.Go(func() {
			err := middleware.WrapModel(context.Background(), abi.RuntimeContext{}, abi.ModelRequest{}, nil, func(context.Context, abi.RuntimeContext, abi.ModelRequest, func(wire.StreamChunk) error) error {
				n := current.Add(1)
				for {
					old := maximum.Load()
					if n <= old || maximum.CompareAndSwap(old, n) {
						break
					}
				}
				entered <- struct{}{}
				<-release
				current.Add(-1)
				return nil
			})
			if err != nil {
				t.Error(err)
			}
		})
	}
	<-entered
	<-entered
	select {
	case <-entered:
		t.Fatal("model semaphore exceeded")
	default:
	}
	close(release)
	group.Wait()
	if maximum.Load() != 2 {
		t.Fatal(maximum.Load())
	}
	if _, err := NewTemplates([]Template{{Name: "bad", SystemPrompt: strings.Repeat("x", (16<<10)+1)}}); err == nil {
		t.Fatal("oversized profile accepted")
	}
}
