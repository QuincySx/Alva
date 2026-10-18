// INPUT: Real local SDK/provider and test-binary external engine, owned private stores/stdin and CLI arguments
// OUTPUT: Cold/REPL/resume/handoff, full native final answer, deny/cancel and stdout-failure regressions
// POS: [L7 CLI entry] Actual execution fixtures use no paid model, installed vendor CLI, ambient credentials or historical user data.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/platform/system/terminaltext"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	productstore "github.com/QuincySx/Alva/go/apps/hub/sqlite"
	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/platform/storage/sqlite"
)

func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "exec" {
		if _, err := io.ReadAll(os.Stdin); err != nil {
			os.Exit(80)
		}
		fmt.Println(`{"type":"thread.started","thread_id":"hub-cli-fixture-native"}`)
		fmt.Println(`{"type":"item.completed","item":{"type":"agent_message","id":"answer","text":"external fixture answer"}}`)
		fmt.Println(`{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}`)
		os.Exit(0)
	}
	os.Exit(m.Run())
}

func TestPortableExplicitPrivateKeyFileKeepsHubCommandInputIndependent(t *testing.T) {
	directory, err := sqlite.EnsurePrivateDir(filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "key-fixture")
	secret := "portable-hub-private-key-fixture"
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
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"portable Hub fixture\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	args := []string{"--data-dir", filepath.Join(directory, "data"), "--live", "--endpoint", server.URL, "--model", "fixture", "--repl", "--api-key-file", path}
	out, diagnostic, err := invoke(t, args, "explicit Hub prompt\n/quit\n")
	if err != nil || calls.Load() != 1 || !strings.Contains(out, "portable Hub fixture") || strings.Contains(out+diagnostic, secret) {
		t.Fatal("portable Hub secret/input invalid", err)
	}
	raw, err := os.ReadFile(path)
	if err != nil || string(raw) != secret {
		t.Fatal("explicit readonly file changed", err)
	}
	if _, _, err := invoke(t, append(args, "--api-key-fd", "3"), ""); err == nil {
		t.Fatal("multiple secret sources accepted")
	}
}
func invoke(t *testing.T, args []string, input string) (string, string, error) {
	t.Helper()
	var out, diagnostic bytes.Buffer
	err := run(t.Context(), args, io.NopCloser(strings.NewReader(input)), &out, &diagnostic)
	return out.String(), diagnostic.String(), err
}
func inspect(t *testing.T, directory string, fn func(*productstore.Store)) {
	t.Helper()
	store, err := productstore.Open(t.Context(), filepath.Join(directory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	fn(store)
}

func TestOneShotNativeResumeAndColdCommandsNeverStartAnotherTurn(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	out, diagnostic, err := invoke(t, []string{"--data-dir", dir, "--prompt", "first actual CLI turn"}, "")
	if err != nil || !strings.Contains(out, "Tool result: first actual CLI turn") || strings.Contains(out, "session-") || !strings.Contains(diagnostic, "Hub session:") {
		t.Fatal(out, diagnostic, err)
	}
	var id, native string
	inspect(t, dir, func(s *productstore.Store) {
		page, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(page) != 1 {
			t.Fatal(page, err)
		}
		id = page[0].ID
		d, err := s.Detail(t.Context(), id)
		if err != nil {
			t.Fatal(err)
		}
		native = d.Branches[0].NativeSessionID
		if native == "" {
			t.Fatal("actual native identity absent")
		}
	})
	out, _, err = invoke(t, []string{"--data-dir", dir, "--session", id, "--prompt", "second actual CLI turn"}, "")
	if err != nil || !strings.Contains(out, "Tool result: second actual CLI turn") {
		t.Fatal(out, err)
	}
	for _, args := range [][]string{{"--sessions"}, {"--session", id, "--history"}} {
		out, _, err = invoke(t, append([]string{"--data-dir", dir}, args...), "")
		if err != nil || !json.Valid([]byte(out)) {
			t.Fatal("cold output invalid", out, err)
		}
	}
	inspect(t, dir, func(s *productstore.Store) {
		d, err := s.Detail(t.Context(), id)
		if err != nil || d.Branches[0].NativeSessionID != native {
			t.Fatal("resume replaced native identity", err)
		}
		history, err := s.History(t.Context(), id, 0, 128)
		if err != nil {
			t.Fatal(err)
		}
		inputs := 0
		for _, h := range history {
			if h.Input != "" {
				inputs++
			}
		}
		if inputs != 2 {
			t.Fatal("cold read executed or input replayed", inputs)
		}
	})
}
func TestREPLIsLazyAndManualEngineRoundTripRetainsBothNativeBranches(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "lazy")
	out, _, err := invoke(t, []string{"--data-dir", dir, "--repl"}, "/sessions\n/quit\n")
	if err != nil || out != "" {
		t.Fatal(out, err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 0 {
			t.Fatal("cold REPL created work", all, err)
		}
	})
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	dir = filepath.Join(t.TempDir(), "switch")
	out, _, err = invoke(t, []string{"--data-dir", dir, "--repl", "--codex-command", exe, "--external-host-approved"}, "first native turn\n/engine codex\nexternal turn\n/engine alva\nreturn native turn\n/session\n/quit\n")
	if err != nil || !strings.Contains(out, "external fixture answer") || !strings.Contains(out, "return native turn") {
		t.Fatal(out, err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 {
			t.Fatal(all, err)
		}
		d, err := s.Detail(t.Context(), all[0].ID)
		if err != nil || len(d.Branches) != 2 {
			t.Fatal(d, err)
		}
		nativeCount := 0
		for _, branch := range d.Branches {
			if branch.NativeSessionID != "" {
				nativeCount++
			}
		}
		if nativeCount != 2 {
			t.Fatal("manual switch lost native branches", d)
		}
	})
}
func TestNativeFinalOutputReadsExactCommittedRecordBeyondProductPreview(t *testing.T) {
	answer := strings.Repeat("完整答复🙂", 12000)
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		w.Header().Set("Content-Type", "text/event-stream")
		for offset := 0; offset < len(answer); {
			end := min(offset+12000, len(answer))
			for end < len(answer) && answer[end]&0xc0 == 0x80 {
				end--
			}
			data, _ := json.Marshal(map[string]any{"id": "long-native-answer", "choices": []any{map[string]any{"index": 0, "delta": map[string]string{"content": answer[offset:end]}}}})
			fmt.Fprintf(w, "data: %s\n\n", data)
			offset = end
		}
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer upstream.Close()
	out, _, err := invoke(t, []string{"--data-dir", filepath.Join(t.TempDir(), "private"), "--live", "--endpoint", upstream.URL, "--model", "fixture", "--prompt", "return actual long final answer"}, "")
	if err != nil || out != answer+"\n" || calls.Load() != 1 {
		t.Fatal("native answer was truncated/replayed", len(out), len(answer), calls.Load(), err)
	}
}
func TestDefaultPermissionDeniesRealWriteAndFailedTurnPrintsNoAnswer(t *testing.T) {
	workspace := t.TempDir()
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		args, _ := json.Marshal(map[string]string{"path": "must-not-exist", "content": "unapproved"})
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "unapproved-write", "type": "function", "function": map[string]string{"name": "write_file", "arguments": string(args)}}}}, "finish_reason": "tool_calls"}}})
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	out, _, err := invoke(t, []string{"--data-dir", filepath.Join(t.TempDir(), "private"), "--live", "--endpoint", upstream.URL, "--model", "fixture", "--workspace", workspace, "--prompt", "try a denied fixture write"}, "")
	if err == nil || out != "" {
		t.Fatal("denial fabricated final success", out, err)
	}
	if _, err := os.Stat(filepath.Join(workspace, "must-not-exist")); !os.IsNotExist(err) {
		t.Fatal("default permission granted host write", err)
	}
}
func TestCancelledOpenREPLPipeReturnsAndDoesNotCreateSession(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer writer.Close()
	ctx, cancel := context.WithCancel(t.Context())
	dir := filepath.Join(t.TempDir(), "private")
	done := make(chan error, 1)
	go func() { done <- run(ctx, []string{"--data-dir", dir, "--repl"}, reader, io.Discard, io.Discard) }()
	time.AfterFunc(50*time.Millisecond, cancel)
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		reader.Close()
		t.Fatal("cancel left a blocked REPL reader/owner")
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 0 {
			t.Fatal("cancelled cold REPL created task", all, err)
		}
	})
}
func TestBadCLIInvocationFailsBeforeStoreCreation(t *testing.T) {
	for _, args := range [][]string{{}, {"--prompt", "x", "--timeout", "0s"}, {"--repl", "--permission", "allow"}, {"--prompt", "x", "--permission", "ask"}, {"--prompt", "x", "--sessions"}, {"--history"}, {"--prompt", "x", "--engine", "unknown"}, {"--prompt", "x", "--live", "--endpoint", "http://127.0.0.1:1", "--model", "fixture", "--api-key-fd", "2"}} {
		dir := filepath.Join(t.TempDir(), "not-created")
		_, _, err := invoke(t, append([]string{"--data-dir", dir}, args...), "")
		if err == nil {
			t.Fatal("invalid invocation accepted", args)
		}
		if _, err := os.Stat(dir); !os.IsNotExist(err) {
			t.Fatal("invalid grammar created stores", args, err)
		}
	}
}

type failedOutput struct{}

func (failedOutput) Write([]byte) (int, error) { return 0, errors.New("fixture stdout unavailable") }
func TestFailedFinalOutputIsNotASecondExecutionAndApplicationCloses(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	err := run(t.Context(), []string{"--data-dir", dir, "--prompt", "one successful execution only"}, io.NopCloser(strings.NewReader("")), failedOutput{}, io.Discard)
	if err == nil {
		t.Fatal("stdout failure hidden")
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 || all[0].Status != engine.Succeeded {
			t.Fatal("actual result overwritten by display failure", all, err)
		}
		history, err := s.History(t.Context(), all[0].ID, 0, 128)
		if err != nil {
			t.Fatal(err)
		}
		turns := map[string]bool{}
		for _, h := range history {
			if h.TurnID != "" {
				turns[h.TurnID] = true
			}
		}
		if len(turns) != 1 {
			t.Fatal("display failure replayed task", len(turns))
		}
	})
}
func TestTerminalEscapesAndBoundedInputRejectControlInjectionAndLongLines(t *testing.T) {
	if value := terminaltext.Escape("safe\x1b[2J\r\u009b31m\n"); value != "safe\\u001b[2J\\u000d\\u009b31m\n" {
		t.Fatal("terminal control retained", value)
	}
	input := newInput(io.NopCloser(strings.NewReader(strings.Repeat("x", 65537) + "\n")))
	defer input.close()
	if _, err := input.line(t.Context()); err == nil {
		t.Fatal("overlong command accepted")
	}
}

type replyDiagnostics struct {
	bytes.Buffer
	reply   *os.File
	replied bool
	err     error
}

func (w *replyDiagnostics) Write(data []byte) (int, error) {
	n, err := w.Buffer.Write(data)
	if strings.HasPrefix(string(data), "Permission ") && !w.replied {
		fields := strings.Fields(string(data))
		w.replied = true
		if len(fields) < 2 {
			w.err = errors.New("missing actual approval ID")
		} else {
			_, w.err = fmt.Fprintf(w.reply, "allow %s\n/quit\n", fields[1])
		}
	}
	return n, errors.Join(err, w.err)
}
func TestREPLAskBindsActualRequestIDAndWaitsForApprovedRealWrite(t *testing.T) {
	workspace := t.TempDir()
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		finish := "stop"
		delta := map[string]any{"content": "approved actual write completed"}
		if calls.Add(1) == 1 {
			args, _ := json.Marshal(map[string]string{"path": "approved-file", "content": "actual approved bytes"})
			finish = "tool_calls"
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "approved-write", "type": "function", "function": map[string]string{"name": "write_file", "arguments": string(args)}}}}
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
	defer writer.Close()
	diagnostic := &replyDiagnostics{reply: writer}
	var output bytes.Buffer
	err = run(t.Context(), []string{"--data-dir", filepath.Join(t.TempDir(), "private"), "--live", "--endpoint", upstream.URL, "--model", "fixture", "--workspace", workspace, "--prompt", "request one explicit write", "--repl", "--permission", "ask"}, reader, &output, diagnostic)
	if err != nil || !diagnostic.replied || !strings.Contains(output.String(), "approved actual write completed") {
		t.Fatal(output.String(), diagnostic.String(), err)
	}
	if data, err := os.ReadFile(filepath.Join(workspace, "approved-file")); err != nil || string(data) != "actual approved bytes" {
		t.Fatal("actual authorized effect absent", string(data), err)
	}
}
func TestTurnDeadlineCancelsActualStreamAndNeverPrintsPartialAnswer(t *testing.T) {
	ended := make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer close(ended)
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"PARTIAL_MUST_NOT_PRINT\"}}]}\n\n")
		w.(http.Flusher).Flush()
		<-r.Context().Done()
	}))
	defer upstream.Close()
	dir := filepath.Join(t.TempDir(), "private")
	out, _, err := invoke(t, []string{"--data-dir", dir, "--live", "--endpoint", upstream.URL, "--model", "fixture", "--prompt", "actual deadline", "--timeout", "1s"}, "")
	if !errors.Is(err, context.DeadlineExceeded) || out != "" {
		t.Fatal("timeout printed partial success", out, err)
	}
	select {
	case <-ended:
	case <-time.After(time.Second):
		t.Fatal("actual HTTP stream owner remained active")
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 || all[0].Status == engine.Succeeded || all[0].Status == "running" {
			t.Fatal("deadline invented terminal success or lost cleanup", all, err)
		}
	})
}
