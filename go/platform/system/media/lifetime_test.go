// INPUT: Real owned helper process, active-call barriers and occupied processing slots
// OUTPUT: Terminal Close, retained active ownership, true reap and queued-start cancellation regressions
// POS: [platform / OS primitive] Physical cleanup is never inferred from decoder errors or an early returned timeout
package media

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"image"
	"image/png"
	"io"
	"os"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
)

func TestMediaLifetimeOwnedChild(t *testing.T) {
	if os.Getenv("ALVA_MEDIA_LIFETIME_CHILD") != "1" {
		return
	}
	_, _ = io.WriteString(os.Stdout, "ready\n")
	_, _ = io.Copy(io.Discard, os.Stdin)
	os.Exit(0)
}

func TestProcessorCloseCancelsActualStandardImageQueue(t *testing.T) {
	var source bytes.Buffer
	if err := png.Encode(&source, image.NewRGBA(image.Rect(0, 0, 1, 1))); err != nil {
		t.Fatal(err)
	}
	processingJobs <- struct{}{}
	processingJobs <- struct{}{}
	defer func() { <-processingJobs; <-processingJobs }()
	p := &Processor{}
	done := make(chan error, 1)
	go func() { _, err := p.ProcessImage(context.Background(), source.Bytes(), "image/png"); done <- err }()
	deadline := time.Now().Add(time.Second)
	for {
		p.mu.Lock()
		active := p.active
		p.mu.Unlock()
		if active == 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("actual image call did not enter owner")
		}
		time.Sleep(time.Millisecond)
	}
	cleanup, stop := context.WithTimeout(context.Background(), time.Second)
	defer stop()
	if err := p.Close(cleanup); err != nil {
		t.Fatal("standard image queue ignored owner cancellation", err)
	}
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal("closed queued image returned a derivative", err)
	}
	if !p.CleanupComplete() {
		t.Fatal("actual queued image call remained active")
	}
}

func TestProcessorCloseRetainsActiveOwnerAndActuallyReapsChild(t *testing.T) {
	p := &Processor{}
	end, err := p.enter()
	if err != nil {
		t.Fatal(err)
	}
	child, err := p.startProcess(t.Context(), process.Config{Command: os.Args[0], Args: []string{"-test.run=^TestMediaLifetimeOwnedChild$"}, Cwd: t.TempDir(), Env: map[string]string{"ALVA_MEDIA_LIFETIME_CHILD": "1"}})
	if err != nil {
		end()
		t.Fatal(err)
	}
	if line, err := bufio.NewReader(child.Stdout).ReadString('\n'); err != nil || line != "ready\n" {
		end()
		t.Fatal("real child not running", line, err)
	}
	short, cancel := context.WithTimeout(context.Background(), time.Millisecond)
	err = p.Close(short)
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) || p.CleanupComplete() {
		end()
		t.Fatal("active owner lost on timed Close", err)
	}
	end()
	cleanup, stop := context.WithTimeout(context.Background(), 3*time.Second)
	defer stop()
	if err = p.Close(cleanup); err != nil || !p.CleanupComplete() || !child.CleanupComplete() {
		t.Fatal("real child not reaped/owner not drained", err)
	}
	if _, err = p.enter(); err == nil {
		t.Fatal("closed decoder owner revived")
	}
}

func TestProcessorCloseCancelsCallWaitingForGlobalOperationBudget(t *testing.T) {
	processingJobs <- struct{}{}
	processingJobs <- struct{}{}
	defer func() { <-processingJobs; <-processingJobs }()
	p := &Processor{}
	done := make(chan error, 1)
	go func() {
		end, err := p.begin(context.Background())
		if err == nil {
			end()
		}
		done <- err
	}()
	deadline := time.Now().Add(time.Second)
	for {
		p.mu.Lock()
		active := p.active
		p.mu.Unlock()
		if active == 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("call did not enter owner")
		}
		time.Sleep(time.Millisecond)
	}
	cleanup, stop := context.WithTimeout(context.Background(), time.Second)
	defer stop()
	if err := p.Close(cleanup); err != nil {
		t.Fatal("Close waited for unrelated global jobs", err)
	}
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal("closed queued call admitted", err)
	}
	if !p.CleanupComplete() {
		t.Fatal("cancelled queued call retained phantom work")
	}
}
