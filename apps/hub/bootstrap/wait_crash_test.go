// INPUT: Actual restartable Hub bootstrap, a killed owned test process and local model/file effects.
// OUTPUT: Interrupted continuation never replays input/tools, expired requests fail closed and review can reset state.
// POS: [L7 crash acceptance] Exercises real process loss and supplied journals without credentials or paid endpoints.
package bootstrap

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestHubWaitCrashHelper(t *testing.T) {
	path := os.Getenv("ALVA_TEST_HUB_WAIT_DIRECTORY")
	if path == "" {
		t.Skip("owned helper process only")
	}
	config := waitConfig(t, os.Getenv("ALVA_TEST_HUB_WAIT_ENDPOINT"))
	config.DataDirectory = path
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	id := os.Getenv("ALVA_TEST_HUB_WAIT_SESSION")
	v, err := a.Service.Waits(t.Context(), id)
	if err != nil || !v.CanResume {
		t.Fatal(v, err)
	}
	if _, err = a.Service.ResumeWait(t.Context(), id, waitTarget(v)); err != nil {
		t.Fatal(err)
	}
	<-t.Context().Done() // The parent kills this process after the actual write and before its next response.
}

func TestHubWaitKilledContinuationDoesNotReplayEffects(t *testing.T) {
	var calls atomic.Int32
	reached := make(chan struct{})
	release := make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		if n <= 2 {
			waitReply(w, "write_file", `{"path":"once.txt","content":"actual authorized write"}`, "")
			return
		}
		_, _ = io.Copy(io.Discard, r.Body)
		if n == 3 {
			close(reached)
		}
		select {
		case <-r.Context().Done():
		case <-release:
		}
	}))
	defer upstream.Close()
	defer close(release)
	config := waitConfig(t, upstream.URL)
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	workspace := t.TempDir()
	s, err := a.Service.Create(t.Context(), "crash continuation", workspace)
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Service.EnableDurableWaits(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	turn, err := a.Service.Submit(t.Context(), s.ID, "one original mutation request")
	if err != nil {
		t.Fatal(err)
	}
	v := waitStatus(t, a, s.ID, engine.Waiting)
	resolveFirst(t, a, s.ID, v, `{"allow":true}`)
	closeApp(t, a)
	command := exec.CommandContext(t.Context(), os.Args[0], "-test.run=^TestHubWaitCrashHelper$", "-test.timeout=30s")
	command.Env = []string{"ALVA_TEST_HUB_WAIT_DIRECTORY=" + config.DataDirectory, "ALVA_TEST_HUB_WAIT_ENDPOINT=" + upstream.URL, "ALVA_TEST_HUB_WAIT_SESSION=" + s.ID}
	var output bytes.Buffer
	command.Stdout, command.Stderr = &output, &output
	if err = command.Start(); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- command.Wait() }()
	reaped := false
	defer func() {
		if !reaped {
			_ = command.Process.Kill()
			<-done
		}
	}()
	select {
	case <-reached:
	case err = <-done:
		reaped = true
		t.Fatalf("helper exited before effect: %v\n%s", err, output.String())
	case <-time.After(15 * time.Second):
		t.Fatal("actual mutation never reached its next model request")
	}
	path := filepath.Join(workspace, "once.txt")
	data, err := os.ReadFile(path)
	if err != nil || string(data) != "actual authorized write" {
		t.Fatal(string(data), err)
	}
	if err = command.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	err = <-done
	reaped = true
	var exit *exec.ExitError
	if !errors.As(err, &exit) {
		t.Fatal("expected killed process", err)
	}
	// A replay of write_file would overwrite this independently changed content.
	if err = os.WriteFile(path, []byte("independent content after crash"), 0600); err != nil {
		t.Fatal(err)
	}
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	v, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil || v.Status != engine.Interrupted || v.Snapshot.State.Status != durable.Running || v.CanResume || v.TurnID != turn {
		t.Fatal(v, err)
	}
	if _, err = a.Service.ResumeWait(t.Context(), s.ID, waitTarget(v)); err == nil {
		t.Fatal("unknown continuation was replayed")
	}
	if calls.Load() != 3 {
		t.Fatal("restart or inspection executed a request", calls.Load())
	}
	data, err = os.ReadFile(path)
	if err != nil || string(data) != "independent content after crash" {
		t.Fatal("mutation replayed", string(data), err)
	}
	if err = a.Service.AbandonWait(t.Context(), s.ID, waitTarget(v)); err != nil {
		t.Fatal(err)
	}
	v, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil || v.Snapshot.State.Status != durable.Idle || v.Status != engine.Interrupted || calls.Load() != 3 {
		t.Fatal(v, calls.Load(), err)
	}
	history, err := a.Store.History(t.Context(), s.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	inputs, abandoned := 0, false
	for _, event := range history {
		if event.Input != "" {
			inputs++
		}
		if event.Timeline != nil && event.Timeline.Kind == "wait_abandoned" {
			abandoned = true
		}
	}
	if inputs != 1 || !abandoned {
		t.Fatal("original input replayed or explicit review missing", inputs, abandoned)
	}
}

func TestHubWaitExpiredCheckpointRequiresReviewWithoutExecution(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		waitReply(w, "await_external", `{"request":{"job":"expired"}}`, "")
	}))
	defer upstream.Close()
	a, err := Open(t.Context(), waitConfig(t, upstream.URL))
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	s, err := a.Service.Create(t.Context(), "expired wait", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Service.EnableDurableWaits(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(t.Context(), s.ID, "wait explicitly"); err != nil {
		t.Fatal(err)
	}
	v := waitStatus(t, a, s.ID, engine.Waiting)
	d, err := a.Store.Detail(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	b, err := product.ActiveBranch(d)
	if err != nil {
		t.Fatal(err)
	}
	backend, err := journal.Open(t.Context(), a.Repository, s.ID, b.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	_, releaseClaim, err := a.Repository.BeginRun(t.Context(), s.ID, b.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	snapshot, err := backend.Load(t.Context())
	if err == nil {
		snapshot.State.Requests[0].ExpiresAt = time.Now().Add(-time.Hour)
		_, err = backend.Commit(t.Context(), snapshot.Revision, snapshot.State, nil)
	}
	releaseClaim()
	if err != nil {
		t.Fatal(err)
	}
	v, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil || v.CanResume {
		t.Fatal(v, err)
	}
	r := v.Snapshot.State.Requests[0]
	if err = a.Service.ResolveWait(t.Context(), s.ID, product.WaitResolution{Target: waitTarget(v), RequestID: r.ID, Binding: r.Binding, Result: []byte(`{"expired":"answer"}`)}); !errors.Is(err, durable.ErrExpired) {
		t.Fatal(err)
	}
	if _, err = a.Service.ResumeWait(t.Context(), s.ID, waitTarget(v)); err == nil {
		t.Fatal("expired wait resumed")
	}
	if err = a.Service.AbandonWait(context.Background(), s.ID, waitTarget(v)); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 {
		t.Fatal("expiry handling called a model", calls.Load())
	}
}
