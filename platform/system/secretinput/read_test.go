// INPUT: Finite secret buffers, real private OS pipes and cancellation barriers
// OUTPUT: Exact byte bounds and actual blocked read cancellation without an orphan goroutine
// POS: [platform / OS primitive] Native stdin lifecycle regression tests do not inspect user secrets or global files
package secretinput

import (
	"bytes"
	"context"
	"errors"
	"os"
	"strings"
	"testing"
	"time"
)

func TestBoundsAndCanceledRead(t *testing.T) {
	data, err := Read(context.Background(), strings.NewReader("secret"), 6)
	if err != nil || string(data) != "secret" {
		t.Fatalf("bounded read failed: %q %v", data, err)
	}
	if _, err = Read(context.Background(), strings.NewReader("secret!"), 6); err == nil {
		t.Fatal("oversized input accepted")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if data, err = Read(ctx, strings.NewReader("secret"), 6); !errors.Is(err, context.Canceled) || data != nil {
		t.Fatal("canceled input disclosed")
	}
}

func TestActualPipeCancellationSettlesReaderAndRetainsCallerFile(t *testing.T) {
	read, write, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer read.Close()
	defer write.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { _, err := Read(ctx, read, 1<<20); done <- err }()
	// Larger than the default native pipe buffers: completion requires a real
	// reader to drain data. Producer stays open, so the bounded read must still
	// wait for EOF (or one byte beyond its exact limit) after this write.
	wrote := make(chan error, 1)
	go func() { _, err := write.Write(bytes.Repeat([]byte("s"), 1<<20)); wrote <- err }()
	select {
	case err = <-wrote:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("secret input never began draining the pipe")
	}
	select {
	case err = <-done:
		t.Fatalf("open producer was not actually waiting for EOF: %v", err)
	default:
	}
	cancel()
	select {
	case err = <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("not canceled: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("secret pipe read survived cancellation")
	}
	// The cancellation callback is settled before a fresh caller reuses stdin.
	reused := make(chan error, 1)
	go func() { _, err := Read(context.Background(), read, 1<<20); reused <- err }()
	if err = write.SetWriteDeadline(time.Now().Add(3 * time.Second)); err != nil {
		t.Fatal(err)
	}
	if _, err = write.Write([]byte("next")); err != nil {
		t.Fatal(err)
	}
	write.Close()
	select {
	case err = <-reused:
		if err != nil {
			t.Fatalf("borrowed file closed or deadline retained: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("borrowed input did not settle after producer EOF")
	}
}
