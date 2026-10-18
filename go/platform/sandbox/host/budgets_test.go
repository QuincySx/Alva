// INPUT: context, errors, testing, time, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: Model callback lease, resource delegation and actual runner ownership regressions
// POS: [platform / WASM host or guest] Prevents swallowed callback errors, late mutation and false completed runner shutdown.
package host

import (
	"context"
	"errors"
	"testing"
	"time"

	kernelabi "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

func TestModelProxyDoesNotSwallowCallbackFailureOrAcceptLateEmission(t *testing.T) {
	request := requestFor(t, false, "")
	if err := normalize(&request); err != nil {
		t.Fatal(err)
	}
	encoded, _ := abi.Encode(abi.ModelRequest{Version: abi.Version}, abi.MaxModelRequestBytes)
	ctx, cancel := context.WithCancel(context.Background())
	j := &job{request: request, caps: Capabilities{Model: modelFunc(func(ctx context.Context, _ kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		_ = emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		cancel()
		_ = emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "ignored failure"})
		return nil
	})}}
	if _, err := j.dispatch(ctx, abi.OpModel, encoded); !errors.Is(err, context.Canceled) {
		t.Fatal("model swallowed callback failure", err)
	}
	var saved func(wire.StreamChunk) error
	j.caps.Model = modelFunc(func(_ context.Context, actual kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if actual.Config.MaxTokens == nil || *actual.Config.MaxTokens != request.Limits.MaxOutputTokens {
			t.Error("host token budget not enforced")
		}
		saved = emit
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	if _, err := j.dispatch(context.Background(), abi.OpModel, encoded); err != nil {
		t.Fatal(err)
	}
	if err := saved(wire.StreamChunk{Kind: wire.ChunkText, Text: "late"}); err == nil {
		t.Fatal("late model emission admitted after return")
	}
	tooMany := request.Limits.MaxOutputTokens + 1
	encoded, _ = abi.Encode(abi.ModelRequest{Version: abi.Version, Config: wire.ModelConfig{MaxTokens: &tooMany}}, abi.MaxModelRequestBytes)
	if _, err := j.dispatch(context.Background(), abi.OpModel, encoded); err == nil {
		t.Fatal("guest enlarged host output budget")
	}
}

func TestWASIRunnerCloseWaitsForActualJobExit(t *testing.T) {
	request := requestFor(t, false, "")
	runner := NewRunner()
	defer runner.Close(context.Background())
	entered := make(chan struct{})
	release := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		_, err := runner.Run(context.Background(), request, Capabilities{Model: modelFunc(func(_ context.Context, _ kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
			close(entered)
			<-release
			return emitText(emit, "completed")
		})})
		done <- err
	}()
	select {
	case <-entered:
	case <-time.After(45 * time.Second):
		t.Fatal("actual job did not reach host model")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err := runner.Close(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("runner falsely closed live job", err)
	}
	if _, err := runner.Run(context.Background(), request, Capabilities{}); err == nil {
		t.Fatal("closing runner admitted new job")
	}
	select {
	case err := <-done:
		t.Fatal("Close secretly terminated running job", err)
	default:
	}
	close(release)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if err := runner.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
}

func TestWASIExchangeAndTotalIOBudgetsFailClosed(t *testing.T) {
	request := requestFor(t, false, "")
	request.Limits.MaxExchanges = 1
	calls := 0
	_, err := runFixture(context.Background(), request, Capabilities{Model: modelFunc(func(_ context.Context, _ kernelabi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		return emitText(emit, "never")
	})})
	if err == nil || calls != 0 {
		t.Fatal("exchange limit reached actual model", err, calls)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	request.Limits.MaxIOBytes = 1 << 20
	j := &job{request: request, ctx: ctx, stop: cancel}
	if err := j.observe("request", abi.OpModel, make([]byte, (1<<20)+1)); err == nil || ctx.Err() == nil {
		t.Fatal("nil observer bypassed global byte budget")
	}
}
