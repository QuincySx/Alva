// INPUT: Real native repositories, managed gateway, local transport fixtures and cancellable lifecycle.
// OUTPUT: Actual authority, strict invocation and physical cleanup ownership regressions.
// POS: [L4 extension] MCP boundaries reject borrowed identities before approval or journal activity.
package managedmcp

import (
	"context"
	"encoding/json"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestMCPRunRequiresActualNativeAuthority(t *testing.T) {
	p, _, runtime := fixture(t, security.ApprovalFunc(allow))
	state, _ := abi.NewAgentState(fixtureModel{}, core.NewMemorySession(p.id), nil)
	foreign := runtime
	foreign.State = state
	if err := p.OnRunStart(context.Background(), foreign); err == nil {
		t.Fatal("same ID memory session acquired native worker authority")
	}
	if p.runID != "" || p.Stats().Records != 0 {
		t.Fatal("rejected run changed ownership")
	}
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
}

func TestMCPGatewayRejectsBorrowedInvocationBeforeApproval(t *testing.T) {
	var approvals atomic.Int32
	p, _, runtime := fixture(t, security.ApprovalFunc(func(ctx context.Context, req security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals.Add(1)
		return allow(ctx, req)
	}))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	call := wire.ToolCall{ID: "call", Name: p.gateway.Name(), Arguments: json.RawMessage(`{"action":"list_servers"}`)}
	base := abi.ToolExecutionContext{Session: p.journal.log, SessionID: p.id, RunID: runtime.RunID, CallID: call.ID, Workspace: p.servers[0].config.Process.Cwd, ActualTool: p.gateway}
	tests := []struct {
		name string
		edit func(*abi.ToolExecutionContext, *wire.ToolCall)
	}{
		{"same_id_memory", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.Session = core.NewMemorySession(p.id) }},
		{"missing_session", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.Session = nil }},
		{"missing_tool", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.ActualTool = nil }},
		{"copied_tool", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { e.ActualTool = &gateway{plugin: p} }},
		{"wrong_name", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { c.Name = "another" }},
		{"empty_call", func(e *abi.ToolExecutionContext, c *wire.ToolCall) { c.ID = ""; e.CallID = "" }},
		{"duplicate_json", func(e *abi.ToolExecutionContext, c *wire.ToolCall) {
			c.Arguments = []byte(`{"action":"call_tool","action":"list_servers"}`)
		}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e, c := base, call
			tt.edit(&e, &c)
			before := p.Stats()
			if _, err := p.gateway.Execute(context.Background(), e, c); err == nil {
				t.Fatal("borrowed/ambiguous invocation accepted")
			}
			if approvals.Load() != 0 || p.Stats() != before {
				t.Fatal("invalid invocation reached approval/journal")
			}
		})
	}
	if _, err := p.gateway.Execute(context.Background(), base, call); err != nil {
		t.Fatal(err)
	}
	if approvals.Load() != 1 {
		t.Fatal("actual invocation denied")
	}
}

type completedTransport struct{ failingTransport }

func (*completedTransport) CleanupComplete() bool { return true }
func TestMCPRetainsCloseErrorWithoutRetainingReapedWorker(t *testing.T) {
	p, _, runtime := fixture(t, security.ApprovalFunc(allow))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	cause := errors.Join(ErrJournal, errors.New("retained observer failure"))
	worker := &completedTransport{failingTransport{closeErr: cause}}
	p.servers[0].client = worker
	if err := p.OnRunEnd(context.Background(), runtime, nil); !errors.Is(err, cause) {
		t.Fatal("lost worker failure", err)
	}
	if p.servers[0].client != nil {
		t.Fatal("physically reaped worker still retained")
	}
	if err := p.Close(context.Background()); !errors.Is(err, cause) {
		t.Fatal("owner retry erased prior cleanup error", err)
	}
}

func TestMCPCloseCancelsAndJoinsApproval(t *testing.T) {
	entered, release, observed := make(chan struct{}), make(chan struct{}), make(chan struct{})
	p, _, runtime := fixture(t, security.ApprovalFunc(func(ctx context.Context, _ security.ApprovalRequest) (security.ApprovalDecision, error) {
		close(entered)
		<-release
		select {
		case <-ctx.Done():
			close(observed)
		default:
		}
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	}))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { _, err := invoke(context.Background(), p, `{"action":"list_servers"}`); done <- err }()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("approval not entered")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	err := p.Close(ctx)
	cancel()
	complete := p.CleanupComplete()
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
	if err := p.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if !p.CleanupComplete() {
		t.Fatal("cleanup retry incomplete")
	}
}
