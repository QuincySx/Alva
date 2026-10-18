// INPUT: Explicit native shell owner, blocked approval/activity callbacks and real background cat job.
// OUTPUT: Owner Close joins every admitted Tool callback and propagates cancellation before fact-store release.
// POS: [L4 extension] Approval and job management retain cleanup ownership independently of process reap.
package shell

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestShellCloseJoinsApprovalAndManagement(t *testing.T) {
	for _, stage := range []string{"approval", "stdin_admission"} {
		t.Run(stage, func(t *testing.T) {
			f := newFixture(t)
			tool := "execute_shell"
			args := map[string]any{"command": "printf should-not-start"}
			if stage == "stdin_admission" {
				result, err := f.call(t, "execute_shell", map[string]any{"command": "cat", "run_in_background": true})
				if err != nil {
					t.Fatal(err)
				}
				tool = "write_shell_stdin"
				args = map[string]any{"job_id": jobID(t, result), "stdin": "should-not-write"}
			}
			entered, release, observed := make(chan struct{}), make(chan struct{}), make(chan struct{})
			block := func(ctx context.Context) error {
				close(entered)
				<-release
				select {
				case <-ctx.Done():
					close(observed)
					return ctx.Err()
				default:
					return nil
				}
			}
			if stage == "approval" {
				f.p.config.Guard, _ = security.NewGuard(f.cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(ctx context.Context, _ security.ApprovalRequest) (security.ApprovalDecision, error) {
					err := block(ctx)
					return security.ApprovalDecision{Allowed: true, Scope: security.Once}, err
				})))
			} else {
				f.p.config.Report = func(ctx context.Context, _ Activity) error { return block(ctx) }
			}
			done := make(chan error, 1)
			go func() { _, err := f.call(t, tool, args); done <- err }()
			select {
			case <-entered:
			case <-time.After(time.Second):
				t.Fatal("callback did not enter")
			}
			ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
			err := f.p.Close(ctx)
			cancel()
			complete := f.p.CleanupComplete()
			close(release)
			callErr := <-done
			if !errors.Is(err, context.DeadlineExceeded) || complete {
				t.Error("active Tool callback lost owner", err, complete)
			}
			if !errors.Is(callErr, context.Canceled) {
				t.Error("owner cancellation did not reach Tool", callErr)
			}
			select {
			case <-observed:
			default:
				t.Error("callback did not see owner cancellation")
			}
			if err := f.p.Close(context.Background()); err != nil {
				t.Fatal(err)
			}
			if !f.p.CleanupComplete() {
				t.Fatal("cleanup retry incomplete")
			}
		})
	}
}

func TestShellRejectsAmbiguousInvocation(t *testing.T) {
	f := newFixture(t)
	tool := f.p.Tools()[0]
	for _, raw := range []string{`{"command":"one","command":"two"}`, `{"command":"\ud800"}`} {
		e := abi.ToolExecutionContext{ActualTool: tool, Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd}
		if _, err := tool.Execute(context.Background(), e, wire.ToolCall{ID: "call", Name: tool.Name(), Arguments: []byte(raw)}); err == nil {
			t.Fatal("ambiguous command accepted")
		}
	}
	e := abi.ToolExecutionContext{ActualTool: tool, Session: f.log, SessionID: "native", RunID: "run", Workspace: f.cwd}
	if _, err := tool.Execute(context.Background(), e, wire.ToolCall{Name: tool.Name(), Arguments: []byte(`{"command":"printf no"}`)}); err == nil {
		t.Fatal("empty call admitted")
	}
	if len(f.p.jobs) != 0 {
		t.Fatal("invalid invocation started a process")
	}
}
