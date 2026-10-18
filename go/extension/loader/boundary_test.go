// INPUT: Approved real AEP helper process and native session dispatch.
// OUTPUT: Gateway actual tool/call binding and input budget regressions.
// POS: [L4 extension] Borrowed native Plugin gateway invocations fail before approval or remote execution.
package loader

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestAEPGatewayRequiresActualDispatchBeforeApproval(t *testing.T) {
	f := newPluginFixture(t, "remote", nil)
	var approvals atomic.Int32
	f.p.config.Guard, _ = security.NewGuard(f.workspace, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals.Add(1)
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	model := pluginModel(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	state, _ := abi.NewAgentState(model, f.log, nil)
	runtime := abi.RuntimeContext{State: state, RunID: "run", Workspace: f.workspace}
	if err := f.p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	call := wire.ToolCall{ID: "call", Name: f.p.Tool().Name(), Arguments: json.RawMessage(`{"action":"catalog"}`)}
	base := abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.workspace, ActualTool: f.p.Tool()}
	tests := []struct {
		name string
		edit func(*abi.ToolExecutionContext, *wire.ToolCall)
	}{
		{"missing_tool", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.ActualTool = nil }},
		{"copied_tool", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.ActualTool = &pluginGateway{plugin: f.p} }},
		{"wrong_name", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { c.Name = "other" }},
		{"wrong_call", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.CallID = "other" }},
		{"empty_call", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.CallID = ""; c.ID = "" }},
		{"oversize", func(e *abi.ToolExecutionContext, c *wire.ToolCall) {
			c.Arguments = []byte(`{"action":"catalog","query":"` + strings.Repeat("q", 256<<10) + `"}`)
		}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e, c := base, call
			tt.edit(&e, &c)
			if _, err := f.p.Tool().Execute(context.Background(), e, c); err == nil {
				t.Fatal("borrowed/invalid invocation accepted")
			}
			if approvals.Load() != 0 {
				t.Fatal("invalid invocation reached approval")
			}
		})
	}
	if _, err := f.p.Tool().Execute(context.Background(), base, call); err != nil {
		t.Fatal(err)
	}
}

func TestAEPCloseCancelsAndJoinsApproval(t *testing.T) {
	f := newPluginFixture(t, "remote", nil)
	entered, release, observed := make(chan struct{}), make(chan struct{}), make(chan struct{})
	f.p.config.Guard, _ = security.NewGuard(f.workspace, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(ctx context.Context, _ security.ApprovalRequest) (security.ApprovalDecision, error) {
		close(entered)
		<-release
		select {
		case <-ctx.Done():
			close(observed)
		default:
		}
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	model := pluginModel(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	state, _ := abi.NewAgentState(model, f.log, nil)
	if err := f.p.OnRunStart(context.Background(), abi.RuntimeContext{State: state, RunID: "run", Workspace: f.workspace}); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() {
		_, err := f.p.Tool().Execute(context.Background(), abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.workspace, ActualTool: f.p.Tool()}, wire.ToolCall{ID: "call", Name: f.p.Tool().Name(), Arguments: []byte(`{"action":"call","tool":"remote_echo","arguments":{}}`)})
		done <- err
	}()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("approval not entered")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	err := f.p.Close(ctx)
	cancel()
	complete := f.p.CleanupComplete()
	close(release)
	callErr := <-done
	if !errors.Is(err, context.DeadlineExceeded) || complete {
		t.Error("pending approval lost owned cleanup", err, complete)
	}
	if !errors.Is(callErr, context.Canceled) {
		t.Error("owner cancellation lost", callErr)
	}
	select {
	case <-observed:
	default:
		t.Error("approval did not receive owner cancellation")
	}
	if err := f.p.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if !f.p.CleanupComplete() {
		t.Fatal("cleanup retry incomplete")
	}
}
