// INPUT: Explicit local HTTP server, actual native session and deliberately delayed admission callbacks.
// OUTPUT: Owner cancellation across approval/risk and strict invocation regressions.
// POS: [L4 extension] Web network authority cannot outlive its run or accept ambiguous arguments.
package web

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestWebCloseCancelsAdmissionBeforeNetwork(t *testing.T) {
	for _, stage := range []string{"approval", "risk"} {
		t.Run(stage, func(t *testing.T) {
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { requests.Add(1); w.Write([]byte("unexpected request")) }))
			defer server.Close()
			f := newFixture(t, true)
			p := f.plugin(t, server.URL, false)
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
			if stage == "risk" {
				p.config.Report = func(ctx context.Context, _ Activity) error { return block(ctx) }
			} else {
				p.config.Guard, _ = security.NewGuard(f.cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(ctx context.Context, _ security.ApprovalRequest) (security.ApprovalDecision, error) {
					err := block(ctx)
					return security.ApprovalDecision{Allowed: true, Scope: security.Once}, err
				})))
			}
			state, _ := abi.NewAgentState(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil }), f.log, p.Tools())
			if err := p.OnRunStart(context.Background(), abi.RuntimeContext{State: state, RunID: "run", Workspace: f.cwd}); err != nil {
				t.Fatal(err)
			}
			tool := p.Tools()[0]
			done := make(chan error, 1)
			go func() {
				_, err := tool.Execute(context.Background(), abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: tool}, wire.ToolCall{ID: "call", Name: tool.Name(), Arguments: []byte(`{"url":"` + server.URL + `"}`)})
				done <- err
			}()
			select {
			case <-entered:
			case <-time.After(time.Second):
				t.Fatal("admission did not enter")
			}
			ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
			err := p.Close(ctx)
			cancel()
			complete := p.CleanupComplete()
			close(release)
			callErr := <-done
			if !errors.Is(err, context.DeadlineExceeded) || complete {
				t.Error("outstanding admission lost ownership", err, complete)
			}
			if !errors.Is(callErr, context.Canceled) {
				t.Error("owner cancellation did not reach admission", callErr)
			}
			select {
			case <-observed:
			default:
				t.Error("callback did not observe owner cancellation")
			}
			if requests.Load() != 0 {
				t.Fatal("cancelled admission reached network")
			}
			if err := p.Close(context.Background()); err != nil {
				t.Fatal(err)
			}
		})
	}
}
