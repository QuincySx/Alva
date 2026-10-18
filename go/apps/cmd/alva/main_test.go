// INPUT: bytes, context, strings, testing.
// OUTPUT: Safe-default and explicit-live configuration CLI regressions.
// POS: [L6 application] Reference entry tests that never contact a paid endpoint or read credentials.
package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/host"
	"github.com/QuincySx/Alva/go/platform/storage/sqlite"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestCancelledStdinAPIKeyReadReturnsWithoutPipeEOF(t *testing.T) {
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
		done <- run(ctx, []string{"--live", "--endpoint", "http://127.0.0.1:1/v1/chat/completions", "--model", "fixture", "--prompt", "never send", "--api-key-stdin"}, reader, io.Discard, io.Discard)
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
		t.Fatal("cancelled API key read waited for pipe EOF")
	}
}

func TestPortableExplicitPrivateKeyFileLeavesREPLInputAndFileUnchanged(t *testing.T) {
	directory, err := sqlite.EnsurePrivateDir(filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "key-fixture")
	secret := "portable-private-key-fixture"
	if err := os.WriteFile(path, []byte(secret), 0600); err != nil {
		t.Fatal(err)
	}
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if r.Header.Get("Authorization") != "Bearer "+secret {
			t.Error("explicit file credential lost")
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"portable local fixture\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	var output, diagnostics bytes.Buffer
	args := []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--repl", "--api-key-file", path}
	if err := run(t.Context(), args, strings.NewReader("explicit input remains a prompt\n/quit\n"), &output, &diagnostics); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 || !strings.Contains(output.String(), "portable local fixture") || strings.Contains(output.String()+diagnostics.String(), secret) {
		t.Fatal("portable credential displaced input or leaked")
	}
	raw, err := os.ReadFile(path)
	if err != nil || string(raw) != secret {
		t.Fatal("explicit readonly file changed", err)
	}
	if err := run(t.Context(), append(args, "--api-key-stdin"), nil, io.Discard, io.Discard); err == nil {
		t.Fatal("multiple secret sources accepted")
	}
}

func TestDemoIsSafeDefaultAndRunsRealTool(t *testing.T) {
	var output, diagnostics bytes.Buffer
	if err := run(context.Background(), []string{"--demo", "--prompt", "fixture input"}, strings.NewReader(""), &output, &diagnostics); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "Tool result: fixture input") || !strings.Contains(diagnostics.String(), "no network") {
		t.Fatalf("wrong demo output: %q / %q", output.String(), diagnostics.String())
	}
}

func TestDirectModelTerminalControlsAreVisibleTextRatherThanTerminalActions(t *testing.T) {
	var output, diagnostic bytes.Buffer
	input := "untrusted \x1b]52;c;clipboard\a\r\u009b31m"
	if err := run(t.Context(), []string{"--demo", "--prompt", input}, strings.NewReader(""), &output, &diagnostic); err != nil {
		t.Fatal(err)
	}
	if strings.ContainsAny(output.String(), "\x1b\a\r\u009b") || !strings.Contains(output.String(), "\\u001b]52;c;clipboard") {
		t.Fatal("model control sequence reached terminal", output.String())
	}
}

type cliRunnerFunc func(context.Context, []wire.Message, func(abi.Event) error) error

func (f cliRunnerFunc) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	return f(ctx, input, emit)
}

type blockedWriter struct {
	bytes.Buffer
	entered, release chan struct{}
	once             sync.Once
}

func (w *blockedWriter) Write(data []byte) (int, error) {
	w.once.Do(func() { close(w.entered); <-w.release })
	return w.Buffer.Write(data)
}
func (w *blockedWriter) WriteString(text string) (int, error) { return w.Write([]byte(text)) }

func TestFastStreamWithSlowWriterRecoversSnapshotTailWithoutReplay(t *testing.T) {
	writer := &blockedWriter{entered: make(chan struct{}), release: make(chan struct{})}
	runnerDone := make(chan struct{})
	requests := 0
	runner := cliRunnerFunc(func(ctx context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		requests++
		defer close(runnerDone)
		if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: "start-"}); err != nil {
			return err
		}
		<-writer.entered
		for index := 0; index < 1000; index++ {
			if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: "🙂"}); err != nil {
				return err
			}
		}
		return nil
	})
	controller := host.NewController(runner, 256)
	id, err := controller.StartPrompt(context.Background(), "fixture")
	if err != nil {
		t.Fatal(err)
	}
	observed := make(chan error, 1)
	go func() { observed <- observeRun(context.Background(), controller, id, writer) }()
	select {
	case <-runnerDone:
	case <-time.After(time.Second):
		t.Fatal("observer blocked producer")
	}
	if err := controller.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	close(writer.release)
	if err := <-observed; err != nil {
		t.Fatal(err)
	}
	want := "start-" + strings.Repeat("🙂", 1000) + "\n"
	if writer.String() != want || requests != 1 {
		t.Fatalf("successful output truncated/replayed: bytes=%d want=%d requests=%d", writer.Len(), len(want), requests)
	}
}

func TestInitiallyExpiredSubscriptionRecoversAlreadyCompletedSnapshot(t *testing.T) {
	finished := make(chan struct{})
	controller := host.NewController(cliRunnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		defer close(finished)
		for index := 0; index < 1000; index++ {
			if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: "x"}); err != nil {
				return err
			}
		}
		return nil
	}), 256)
	id, _ := controller.StartPrompt(context.Background(), "fixture")
	<-finished
	if err := controller.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, _, err := controller.Subscribe(id, 0); !errors.Is(err, host.ErrHistoryExpired) {
		t.Fatalf("fixture did not expire initial replay: %v", err)
	}
	var output bytes.Buffer
	if err := observeRun(context.Background(), controller, id, &output); err != nil {
		t.Fatal(err)
	}
	if output.String() != strings.Repeat("x", 1000)+"\n" {
		t.Fatalf("initial snapshot output truncated: %d", output.Len())
	}
}

type shortWriter struct{}

func (shortWriter) Write(data []byte) (int, error) { return len(data) - 1, nil }
func TestShortOutputWriteFails(t *testing.T) {
	if err := writeString(shortWriter{}, "fixture"); !errors.Is(err, io.ErrShortWrite) {
		t.Fatalf("short write accepted: %v", err)
	}
}

func TestExplicitLiveUsesOnlyProvidedStdinKey(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer explicit-fixture-key" {
			t.Error("explicit stdin key not used")
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"local HTTP fixture\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	var output, diagnostics bytes.Buffer
	err := run(context.Background(), []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--prompt", "hello", "--api-key-stdin"}, strings.NewReader("explicit-fixture-key\n"), &output, &diagnostics)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "local HTTP fixture") || strings.Contains(output.String()+diagnostics.String(), "explicit-fixture-key") {
		t.Fatalf("wrong/unsafe output: %q / %q", output.String(), diagnostics.String())
	}
}
func TestUnsupportedOrImplicitRemoteSettingsFail(t *testing.T) {
	for _, args := range [][]string{{"--resume", "old-session"}, {"--endpoint", "https://example.com/chat"}, {"--api-key", "secret"}, {"--live"}, {"--live", "--demo"}} {
		var output, diagnostics bytes.Buffer
		if err := run(context.Background(), args, strings.NewReader(""), &output, &diagnostics); err == nil {
			t.Fatalf("invalid invocation accepted: %v", args)
		}
	}
}
