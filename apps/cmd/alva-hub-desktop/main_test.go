// INPUT: Real pending credential pipes, local HTTP fixture, cancellable native startup and testing
// OUTPUT: Desktop startup cancellation, cold assembly and grammar regressions without opening native windows
// POS: [L6/L7 application] Verify the actual command startup path before GUI and execution ownership transfer
//go:build desktop && !smoke

package main

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/platform/system/loopback"
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
	directory := filepath.Join(t.TempDir(), "new-data")
	args := []string{"--data-dir", directory, "--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-stdin"}
	done := make(chan error, 1)
	go func() {
		startup, err := prepareDesktop(ctx, args, reader, io.Discard)
		if startup != nil {
			err = errors.Join(err, startup.close(t.Context()), errors.New("cancelled startup constructed an application"))
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
	if _, err := os.Stat(directory); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("cancelled credential startup created storage: %v", err)
	}
}

func TestIndependentDiagnosticStartupStaysColdAndShutsDownSeparately(t *testing.T) {
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	startup, err := prepareDesktop(t.Context(), []string{"--demo", "--data-dir", filepath.Join(t.TempDir(), "data"), "--debug-listen", "127.0.0.1:0", "--startup-secrets-stdin", "--debug-controls"}, strings.NewReader(`{"debug_token":"`+token+`"}`), io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer startup.close(t.Context())
	if startup.diagnostics == nil || startup.application.Runtime.HasActive() {
		t.Fatal("diagnostic setup changed execution")
	}
	sessions, err := startup.application.Service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 {
		t.Fatal("diagnostic setup created a product session", err)
	}
	if err := startup.diagnostics.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	session, err := startup.application.Service.Create(t.Context(), "after diagnostic close", "")
	if err != nil {
		t.Fatal(err)
	}
	_, done, err := startup.application.Runtime.Start(t.Context(), session.ID, "explicit demo Send")
	if err != nil {
		t.Fatal("diagnostic shutdown closed Agent", err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("actual demo Send did not finish")
	}
}

func TestLiveCredentialStartupIsColdAndRejectsOversizeSecret(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); w.WriteHeader(500) }))
	defer upstream.Close()
	args := []string{"--data-dir", filepath.Join(t.TempDir(), "data"), "--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-stdin"}
	secret := "fixture-secret-value"
	ctx, cancel := context.WithCancel(t.Context())
	startup, err := prepareDesktop(ctx, args, strings.NewReader(secret+"\n"), io.Discard)
	cancel()
	if err != nil {
		t.Fatal(err)
	}
	if err := startup.close(t.Context()); err != nil {
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
		args = append([]string{"--data-dir", filepath.Join(t.TempDir(), "data")}, args...)
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
	args := []string{"--data-dir", filepath.Join(t.TempDir(), "data"), "--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-stdin"}
	startup, err := prepareDesktop(ctx, args, strings.NewReader("startup-fixture-key\n"), io.Discard)
	cancel()
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := startup.close(t.Context()); err != nil {
			t.Error(err)
		}
	}()
	if calls.Load() != 0 {
		t.Fatal("startup submitted a model request")
	}

	session, err := startup.application.Service.Create(t.Context(), "startup ownership", "")
	if err != nil {
		t.Fatal(err)
	}
	_, done, err := startup.application.Runtime.Start(t.Context(), session.ID, "explicit send")
	if err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("explicit Turn did not finish")
	}

	if calls.Load() != 1 {
		t.Fatalf("expected exactly one explicit model request, got %d", calls.Load())
	}
}
