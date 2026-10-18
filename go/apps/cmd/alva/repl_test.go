// INPUT: Actual direct harness/session, finite/native command streams and local HTTP tool/approval fixtures
// OUTPUT: No-autosend direct REPL, same-native multi-turn history, explicit HITL and polled cancellation regressions
// POS: [L6 application] Tests never import Hub/engine or discover credentials, paid models or ambient shell access.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestDirectREPLRunsOnlyExplicitPromptsInSameNativeSession(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	var out, diagnostic bytes.Buffer
	if err := run(t.Context(), []string{"--repl", "--data-dir", dir}, strings.NewReader("first direct turn\nsecond direct turn\n/session\n/quit\n"), &out, &diagnostic); err != nil {
		t.Fatal(err)
	}
	if strings.Count(out.String(), "Tool result:") != 2 || strings.Contains(out.String(), "Hello Alva") || !strings.Contains(diagnostic.String(), "Native session:") {
		t.Fatal("REPL auto-ran or lost turns", out.String(), diagnostic.String())
	}
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	metadata, err := repo.List(t.Context(), cliOwner, "", 100)
	if err != nil || len(metadata.Sessions) != 1 {
		t.Fatal(metadata, err)
	}
	log, err := repo.OpenSession(t.Context(), cliOwner, metadata.Sessions[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	messages, err := log.Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, m := range messages {
		if m.Role == wire.RoleUser {
			count++
		}
	}
	if count != 2 {
		t.Fatal("direct REPL lost native history or replayed input", count)
	}
}
func TestDirectLiveREPLQuitDoesNotCallProviderAndRejectsInputConflicts(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); http.Error(w, "must not call", 500) }))
	defer upstream.Close()
	if err := run(t.Context(), []string{"--repl", "--live", "--endpoint", upstream.URL, "--model", "fixture"}, strings.NewReader("/quit\n"), io.Discard, io.Discard); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 {
		t.Fatal("empty REPL contacted model")
	}
	for _, args := range [][]string{{"--repl", "--api-key-stdin"}, {"--repl", "--sessions"}, {"--repl", "--sandbox", "wasm"}, {"--repl", "--live", "--endpoint", upstream.URL, "--model", "fixture", "--api-key-fd", "2"}} {
		if err := run(t.Context(), args, strings.NewReader("/quit\n"), io.Discard, io.Discard); err == nil {
			t.Fatal("conflicting input accepted", args)
		}
	}
}
func TestDirectREPLNativeFileInputCancellationKeepsBorrowedDescriptor(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- run(ctx, []string{"--repl"}, reader, io.Discard, io.Discard) }()
	time.AfterFunc(20*time.Millisecond, cancel)
	select {
	case err := <-done:
		if err != context.Canceled {
			t.Fatal(err)
		}
	case <-time.After(2 * time.Second):
		writer.Close()
		<-done
		t.Fatal("direct REPL input ignored cancellation")
	}
	if _, err := reader.Stat(); err != nil {
		t.Fatal("borrowed REPL file was closed", err)
	}
}

type directReplyOutput struct {
	bytes.Buffer
	writer  *os.File
	replied bool
	err     error
}

func (w *directReplyOutput) WriteString(value string) (int, error) { return w.Write([]byte(value)) }

func (w *directReplyOutput) Write(data []byte) (int, error) {
	n, err := w.Buffer.Write(data)
	if strings.HasPrefix(string(data), "Permission ") && !w.replied {
		w.replied = true
		fields := strings.Fields(string(data))
		if len(fields) < 2 {
			return n, fmt.Errorf("missing request ID")
		}
		_, w.err = fmt.Fprintf(w.writer, "allow %s\n/quit\n", fields[1])
	}
	if w.err != nil {
		return n, w.err
	}
	return n, err
}
func TestDirectREPLAskApprovesOnlyActualGuardedWriteAndRetainsRunner(t *testing.T) {
	workspace := t.TempDir()
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		finish := "stop"
		delta := map[string]any{"content": "actual direct write completed"}
		if calls.Add(1) == 1 {
			args, _ := json.Marshal(map[string]string{"path": "approved.txt", "content": "approved direct bytes"})
			finish = "tool_calls"
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "direct-approved-call", "type": "function", "function": map[string]string{"name": "write_file", "arguments": string(args)}}}}
		}
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	diagnostic := &directReplyOutput{writer: writer}
	var output bytes.Buffer
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	if err := run(ctx, []string{"--repl", "--live", "--endpoint", upstream.URL, "--model", "fixture", "--workspace", workspace, "--prompt", "one approved direct action"}, reader, &output, diagnostic); err != nil {
		t.Fatal(err)
	}
	if !diagnostic.replied || !strings.Contains(output.String(), "actual direct write completed") {
		t.Fatal(output.String(), diagnostic.String())
	}
	if data, err := os.ReadFile(filepath.Join(workspace, "approved.txt")); err != nil || string(data) != "approved direct bytes" {
		t.Fatal(string(data), err)
	}
}
