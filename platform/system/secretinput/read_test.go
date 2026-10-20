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
	"sync"
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
	ctx, cancel := context.WithCancel(context.Background())
	reuseCtx, reuseCancel := context.WithCancel(context.Background())
	var workers sync.WaitGroup
	defer func() {
		cancel()
		reuseCancel()
		read.Close()
		write.Close()
		workers.Wait()
	}()
	done := make(chan error, 1)
	workers.Go(func() { _, err := Read(ctx, read, 1<<20); done <- err })
	// Larger than the default native pipe buffers: completion requires a real
	// reader to drain data. Producer stays open, so the bounded read must still
	// wait for EOF (or one byte beyond its exact limit) after this write.
	wrote := make(chan error, 1)
	workers.Go(func() { _, err := write.Write(bytes.Repeat([]byte("s"), 1<<20)); wrote <- err })
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
	workers.Go(func() {
		data, err := Read(reuseCtx, read, 1<<20)
		// Cancellation may leave unread producer bytes in the pipe. A new
		// borrower must still receive the subsequent suffix exactly once.
		if err == nil && (!bytes.HasSuffix(data, []byte("next")) || len(bytes.Trim(bytes.TrimSuffix(data, []byte("next")), "s")) != 0) {
			err = errors.New("reused pipe lost or changed producer bytes")
		}
		reused <- err
	})
	// Anonymous Windows pipes do not implement write deadlines. Keep a real
	// producer, bound the observation, and close/join both sides on failure.
	wroteNext := make(chan error, 1)
	workers.Go(func() {
		_, err := write.Write([]byte("next"))
		write.Close()
		wroteNext <- err
	})
	select {
	case err = <-wroteNext:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("borrowed pipe did not accept the next producer")
	}
	select {
	case err = <-reused:
		if err != nil {
			t.Fatalf("borrowed file closed or deadline retained: %v", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("borrowed input did not settle after producer EOF")
	}
}
