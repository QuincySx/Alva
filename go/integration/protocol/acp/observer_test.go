// INPUT: bytes, context, errors, io, os, strings, sync, testing, time, native/process
// OUTPUT: ACP tail-observation, frame-boundary and cancellation regression tests
// POS: [integration / external protocol] Verifies real owned close retains journal failures without leaking private paths or inventing cancellation errors.
package acp

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
)

func TestACPCloseRetainsPendingTailObserverFailurePrivately(t *testing.T) {
	disk := errors.New("private disk marker")
	c := openFixture(t, "tail-stderr", nil, ObserverFunc(func(ctx context.Context, event IOEvent) error {
		if event.Direction == "stderr" && event.Stage == "received_partial" {
			if err := ctx.Err(); err != nil {
				t.Error("close canceled independent journal context", err)
			}
			if string(event.Data) != "private tail 尾" {
				t.Error("tail bytes changed", string(event.Data))
			}
			return &os.PathError{Op: "append", Path: "/private/secrets/journal", Err: disk}
		}
		return nil
	}))
	// The same helper goroutine writes the tail before reading session/new;
	// wait its response so Close cannot race before the pending bytes exist.
	_ = newFixtureSession(t, c)
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	err := c.Close(ctx)
	if !errors.Is(err, disk) || strings.Contains(err.Error(), "/private/") || strings.Contains(err.Error(), "private disk marker") {
		t.Fatal("private observer evidence lost/leaked", err)
	}
	select {
	case <-c.Done():
	default:
		t.Fatal("close returned before real worker exit")
	}
	if !errors.Is(c.Err(), disk) {
		t.Fatal("terminal error lost late journal failure", c.Err())
	}
}

type byteReader struct{ data []byte }

func (reader *byteReader) Read(out []byte) (int, error) {
	if len(reader.data) == 0 {
		return 0, io.EOF
	}
	out[0] = reader.data[0]
	reader.data = reader.data[1:]
	return 1, nil
}
func TestACPStderrFramesCrossReadsAndUnicodeWithoutLoss(t *testing.T) {
	var events []IOEvent
	c := &Client{ctx: context.Background(), config: Config{ID: "frames", ObserverTimeout: time.Second}, observer: ObserverFunc(func(_ context.Context, event IOEvent) error { events = append(events, event); return nil })}
	raw := []byte("marker=密钥\n尾部")
	if err := c.readStderr(&byteReader{data: raw}); err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 || events[0].Stage != "received" || string(events[0].Data) != "marker=密钥\n" || events[1].Stage != "received_partial" || string(events[1].Data) != "尾部" {
		t.Fatal("raw framing changed", events)
	}
	events = nil
	if err := c.readStderr(bytes.NewReader(bytes.Repeat([]byte("x"), (1<<20)+1))); !errors.Is(err, process.ErrLineLimit) || len(events) != 1 || events[0].Stage != "received_truncated" || len(events[0].Data) != 1<<20 {
		t.Fatal("oversized frame not bounded/fatal", err, len(events))
	}
}

func TestACPCancelDoesNotManufactureObserverCancellation(t *testing.T) {
	var mu sync.Mutex
	var canceled bool
	c := openFixture(t, "hang", nil, ObserverFunc(func(ctx context.Context, _ IOEvent) error {
		if ctx.Err() != nil {
			mu.Lock()
			canceled = true
			mu.Unlock()
		}
		return ctx.Err()
	}))
	c.config.CancelTimeout = 30 * time.Millisecond
	s := newFixtureSession(t, c)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	_, err := s.Prompt(ctx, []ContentBlock{Text("wait")})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("original prompt context cause lost", err)
	}
	var observed *ObserverError
	if errors.As(err, &observed) {
		t.Fatal("ownership cancellation became fake journal failure", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if canceled {
		t.Fatal("observer received owner cancellation")
	}
}
