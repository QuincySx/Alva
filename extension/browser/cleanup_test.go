// INPUT: Native browser recipe, a deliberately blocked prior-activity callback and explicit owner Close.
// OUTPUT: Close cancellation/join and cleanup deadline ownership regressions without starting Chrome.
// POS: [L4 extension] Browser physical completion includes outstanding Tool callbacks and fact writes.
package browser

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestBrowserCloseWaitsForActiveToolAndCancelsAdmission(t *testing.T) {
	for _, stage := range []string{"owner_close", "run_end"} {
		t.Run(stage, func(t *testing.T) {
			f := browserFixture(t, true)
			entered, release, observed := make(chan struct{}), make(chan struct{}), make(chan struct{})
			p := f.plugin(t, func(ctx context.Context, _ Activity) error {
				close(entered)
				<-release
				select {
				case <-ctx.Done():
					close(observed)
					return ctx.Err()
				default:
					return nil
				}
			})
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			state, _ := abi.NewAgentState(model, f.log, nil)
			runtime := abi.RuntimeContext{State: state, RunID: "run", Workspace: f.cwd}
			if err := p.OnRunStart(context.Background(), runtime); err != nil {
				t.Fatal(err)
			}
			tool := p.tools[6] // status still commits intent/outcome; no Chrome startup.
			execution := abi.ToolExecutionContext{Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: tool}
			done := make(chan error, 1)
			go func() {
				_, err := tool.Execute(context.Background(), execution, wire.ToolCall{ID: "call", Name: tool.Name(), Arguments: []byte(`{}`)})
				done <- err
			}()
			select {
			case <-entered:
			case <-time.After(time.Second):
				t.Fatal("callback did not enter")
			}
			cleanup, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
			var err error
			if stage == "owner_close" {
				err = p.Close(cleanup)
			} else {
				err = p.OnRunEnd(cleanup, runtime, nil)
			}
			cancel()
			complete := p.CleanupComplete()
			close(release)
			toolErr := <-done
			if !errors.Is(err, context.DeadlineExceeded) {
				t.Error("Close reported completion while Tool callback was running", err)
			}
			if complete {
				t.Error("active Tool/journal owner reported physical cleanup complete")
			}
			if !errors.Is(toolErr, context.Canceled) {
				t.Error("owner stop did not cancel active admission", toolErr)
			}
			select {
			case <-observed:
			default:
				t.Error("callback never observed owner cancellation")
			}
			if err := p.Close(context.Background()); err != nil {
				t.Fatal(err)
			}
			if !p.CleanupComplete() {
				t.Fatal("cleanup retry did not finish")
			}
		})
	}
}

func TestBrowserRejectsAmbiguousArguments(t *testing.T) {
	tool := &browserTool{name: "browser_action"}
	for _, raw := range []string{`{"action":"press","key":"Enter","action":"type","text":"side effect"}`, `{"action":"type","text":"\ud800"}`} {
		if _, err := tool.decode([]byte(raw)); err == nil {
			t.Fatal("ambiguous browser action accepted")
		}
	}
}
