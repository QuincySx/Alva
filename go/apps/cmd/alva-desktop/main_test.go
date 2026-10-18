// INPUT: Real pending credential pipes, local HTTP fixture, cancellable native startup and testing
// OUTPUT: Desktop startup cancellation, cold assembly and grammar regressions without opening native windows
// POS: [L6/L7 application] Verify the actual command startup path before GUI and execution ownership transfer
//go:build desktop && !smoke

package main

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/platform/system/loopback"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestCredentialPipeCancellationPrecedesWindowStoreAndModel(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); w.WriteHeader(500) }))
	defer upstream.Close()
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	args := []string{"--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-stdin"}
	done := make(chan error, 1)
	go func() {
		startup, err := prepareDesktop(ctx, args, reader, io.Discard)
		if startup != nil {
			err = errors.Join(err, startup.controller.Close(t.Context()), errors.New("cancelled startup constructed an application"))
		}
		done <- err
	}()
	time.AfterFunc(20*time.Millisecond, cancel)
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		writer.Close()
		<-done
		t.Fatal("credential read waited for pipe EOF after cancellation")
	}
	if _, err := reader.Stat(); err != nil {
		t.Fatalf("borrowed input was closed: %v", err)
	}
	if calls.Load() != 0 {
		t.Fatal("credential startup contacted the model")
	}
}

func TestDurableDirectStartupListsAndSelectsWithoutAutoExecution(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "data")
	startup, err := prepareDesktop(t.Context(), []string{"--demo", "--data-dir", directory}, nil, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	if startup.controller != nil || startup.service == nil || startup.service.HasActive() {
		t.Fatal("durable startup used another runtime or executed")
	}
	page, err := startup.service.List(t.Context(), "")
	if err != nil || len(page.Sessions) != 0 {
		t.Fatal("startup created a session", err)
	}
	id, err := startup.service.Create(t.Context(), "actual direct startup")
	if err != nil {
		t.Fatal(err)
	}
	if err := startup.close(t.Context()); err != nil {
		t.Fatal(err)
	}
	reopened, err := prepareDesktop(t.Context(), []string{"--demo", "--data-dir", directory, "--session", id}, nil, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.close(t.Context())
	if reopened.initialSession != id || reopened.service.HasActive() {
		t.Fatal("selection resumed execution")
	}
	if _, exists := reopened.service.CurrentRun(id); exists {
		t.Fatal("cold selection constructed Controller")
	}
	if _, err := prepareDesktop(t.Context(), []string{"--session", id}, nil, io.Discard); err == nil {
		t.Fatal("implicit data source accepted")
	}
}

func TestDurableDirectCredentialCancellationCreatesNoDataRoot(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "must-not-create")
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan error, 1)
	go func() {
		startup, err := prepareDesktop(ctx, []string{"--live", "--endpoint", "http://127.0.0.1:1/v1/chat/completions", "--model", "fixture", "--api-key-stdin", "--data-dir", directory}, reader, io.Discard)
		if startup != nil {
			startup.close(context.Background())
		}
		done <- err
	}()
	time.AfterFunc(20*time.Millisecond, cancel)
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		writer.Close()
		<-done
		t.Fatal("pending key blocked cancellation")
	}
	if _, err := os.Stat(directory); !os.IsNotExist(err) {
		t.Fatal("cancelled credential startup created native storage", err)
	}
}

func TestIndependentDiagnosticStartupStaysColdAndShutsDownSeparately(t *testing.T) {
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	startup, err := prepareDesktop(t.Context(), []string{"--demo", "--debug-listen", "127.0.0.1:0", "--startup-secrets-stdin", "--debug-controls"}, strings.NewReader(`{"debug_token":"`+token+`"}`), io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer startup.close(t.Context())
	if startup.diagnostics == nil {
		t.Fatal("diagnostic setup changed execution")
	}
	if err := startup.diagnostics.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	id, err := startup.controller.StartPrompt(t.Context(), "explicit demo after diagnostic close")
	if err != nil {
		t.Fatal("diagnostic shutdown closed Agent", err)
	}
	deadline := time.Now().Add(time.Second)
	for {
		snapshot, err := startup.controller.Snapshot(id)
		if err != nil {
			t.Fatal(err)
		}
		if snapshot.Status != "running" {
			if snapshot.Status != "completed" {
				t.Fatal(snapshot.Status)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("actual demo Send did not finish")
		}
		time.Sleep(time.Millisecond)
	}
}

func TestLiveCredentialStartupIsColdAndRejectsOversizeSecret(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); w.WriteHeader(500) }))
	defer upstream.Close()
	args := []string{"--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-stdin"}
	secret := "fixture-secret-value"
	ctx, cancel := context.WithCancel(t.Context())
	startup, err := prepareDesktop(ctx, args, strings.NewReader(secret+"\n"), io.Discard)
	cancel()
	if err != nil {
		t.Fatal(err)
	}
	if err := startup.controller.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 {
		t.Fatal("cold preparation called the model")
	}
	startup, err = prepareDesktop(t.Context(), args, strings.NewReader(strings.Repeat("x", 8193)), io.Discard)
	if err == nil || startup != nil || strings.Contains(err.Error(), secret) || calls.Load() != 0 {
		t.Fatalf("oversize key was not rejected privately: %v", err)
	}
}

func TestInvalidModesFailBeforeCredentialRead(t *testing.T) {
	for _, entry := range []struct {
		flags []string
		want  string
	}{
		{[]string{"--live", "--api-key-stdin"}, "requires --endpoint"},
		{[]string{"--demo", "--api-key-stdin"}, "provider options require --live"},
		{[]string{"--live", "--demo"}, "choose"},
		{[]string{"unexpected"}, "positional"},
		{[]string{"--unknown"}, "flag provided"},
	} {
		args := entry.flags

		startup, err := prepareDesktop(t.Context(), args, nil, io.Discard)
		if err == nil || startup != nil || !strings.Contains(err.Error(), entry.want) {
			t.Fatalf("invalid startup accepted or reached credential read: %v / %v", entry.flags, err)
		}
	}
}

func TestStartupSignalScopeDoesNotCancelLaterExplicitSend(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if r.Header.Get("Authorization") != "Bearer startup-fixture-key" {
			t.Error("explicit key was lost")
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"after startup\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer upstream.Close()
	ctx, cancel := context.WithCancel(t.Context())
	args := []string{"--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-stdin"}
	startup, err := prepareDesktop(ctx, args, strings.NewReader("startup-fixture-key\n"), io.Discard)
	cancel()
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := startup.controller.Close(t.Context()); err != nil {
			t.Error(err)
		}
	}()
	if calls.Load() != 0 {
		t.Fatal("startup submitted a model request")
	}

	id, err := startup.controller.StartPrompt(t.Context(), "explicit send")
	if err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(2 * time.Second)
	for {
		snapshot, err := startup.controller.Snapshot(id)
		if err != nil {
			t.Fatal(err)
		}
		if snapshot.Status != "running" {
			if snapshot.Status != "completed" || snapshot.Text != "after startup" {
				t.Fatalf("startup cancellation poisoned actual Send: %#v", snapshot)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("explicit Send did not finish")
		}
		time.Sleep(time.Millisecond)
	}

	if calls.Load() != 1 {
		t.Fatalf("expected exactly one explicit model request, got %d", calls.Load())
	}
}
