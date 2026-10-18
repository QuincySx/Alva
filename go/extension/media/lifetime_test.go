// INPUT: Actual native media selection, a deliberately delayed model call and explicit owner shutdown.
// OUTPUT: Model invocation/receipt ownership and admission cancellation regressions.
// POS: [L4 extension] Media cleanup cannot finish before selected-image model work and final facts settle.
package media

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestMediaCloseCancelsAndJoinsModelBeforeFactsRelease(t *testing.T) {
	f := newFixture(t, true, true)
	r, e := f.direct(t)
	if _, err := f.invoke(t, e, "process", map[string]any{"action": "process_image", "source_uri": f.source.Reference().URI}); err != nil {
		t.Fatal(err)
	}
	if _, err := f.invoke(t, e, "choose", map[string]any{"action": "select", "manifest_uris": []string{f.manifest(t).URI}, "reason": "explicit owner cancellation fixture"}); err != nil {
		t.Fatal(err)
	}
	entered, release, observed := make(chan struct{}), make(chan struct{}), make(chan struct{})
	done := make(chan error, 1)
	go func() {
		done <- f.plugin.WrapModel(context.Background(), r, abi.ModelRequest{}, func(wire.StreamChunk) error { return nil }, func(ctx context.Context, _ abi.RuntimeContext, request abi.ModelRequest, _ func(wire.StreamChunk) error) error {
			if imageCount(request) != 1 {
				return errors.New("fixture did not submit selected image")
			}
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
	}()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("model invocation did not enter")
	}
	// Lifetime joining does not serialize an in-flight model with read-only
	// metadata Tool consumers; alternative loop scheduling can still compose them.
	if _, err := f.invoke(t, e, "parallel-metadata", map[string]any{"action": "read_manifest", "manifest_uri": f.manifest(t).URI}); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	err := f.plugin.Close(ctx)
	cancel()
	complete := f.plugin.CleanupComplete()
	close(release)
	modelErr := <-done
	if !errors.Is(err, context.DeadlineExceeded) || complete {
		t.Error("active model/receipt owner reported cleanup complete", err, complete)
	}
	if !errors.Is(modelErr, context.Canceled) {
		t.Error("owner cancellation did not reach model", modelErr)
	}
	select {
	case <-observed:
	default:
		t.Error("model did not observe owner cancellation")
	}
	if err := f.plugin.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if !f.plugin.CleanupComplete() {
		t.Fatal("cleanup retry incomplete")
	}
}
