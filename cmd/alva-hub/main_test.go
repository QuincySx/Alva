// INPUT: Real local SDK/provider and test-binary external engine, owned private stores/stdin and CLI arguments
// OUTPUT: Cold/REPL/resume/handoff, full native final answer, deny/cancel and stdout-failure regressions
// POS: [L7 CLI entry] Actual execution fixtures use no paid model, installed vendor CLI, ambient credentials or historical user data.
package main

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/platform/system/terminaltext"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
	privatefs "github.com/QuincySx/Alva/platform/system/privatefs"
	"github.com/ncruces/go-sqlite3"
)

func TestMain(m *testing.M) {
	if len(os.Args) > 1 && os.Args[1] == "app-server" {
		runCodexAppServerCLIFixture()
		os.Exit(0)
	}
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

func runCodexAppServerCLIFixture() {
	if strings.Join(os.Args[1:], " ") != "app-server --listen stdio://" {
		os.Exit(90)
	}
	write := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	notify := func(method string, params map[string]any) {
		params["threadId"] = "hub-cli-rpc"
		params["turnId"] = "cli-turn"
		write(map[string]any{"method": method, "params": params})
	}
	finish := func() {
		notify("item/started", map[string]any{"item": map[string]string{"type": "agentMessage", "id": "answer"}})
		notify("item/completed", map[string]any{"item": map[string]string{"type": "agentMessage", "id": "answer", "text": "app-server fixture answer"}})
		notify("turn/completed", map[string]any{"turn": map[string]string{"id": "cli-turn", "status": "completed"}})
	}
	scan := bufio.NewScanner(os.Stdin)
	wantDecision := "decline"
	for scan.Scan() {
		var frame struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
			Result json.RawMessage `json:"result"`
		}
		if json.Unmarshal(scan.Bytes(), &frame) != nil {
			os.Exit(91)
		}
		reply := func(value any) { write(map[string]any{"id": frame.ID, "result": value}) }
		switch frame.Method {
		case "initialize":
			reply(map[string]string{"userAgent": "cli-fixture"})
		case "initialized":
		case "thread/start", "thread/resume":
			var params struct {
				Sandbox  string `json:"sandbox"`
				Approval string `json:"approvalPolicy"`
			}
			json.Unmarshal(frame.Params, &params)
			if params.Sandbox != "workspace-write" || params.Approval != "on-request" {
				os.Exit(92)
			}
			cwd, _ := os.Getwd()
			reply(map[string]any{"thread": map[string]string{"id": "hub-cli-rpc", "cwd": cwd}})
		case "turn/start":
			var params struct {
				Input []struct {
					Text string `json:"text"`
				} `json:"input"`
			}
			json.Unmarshal(frame.Params, &params)
			reply(map[string]any{"turn": map[string]string{"id": "cli-turn", "status": "inProgress"}})
			if len(params.Input) > 0 && strings.Contains(params.Input[0].Text, "ask permission fixture") {
				wantDecision = "accept"
			}
			if len(params.Input) > 0 && strings.Contains(params.Input[0].Text, "permission fixture") {
				notify("item/started", map[string]any{"item": map[string]string{"id": "cmd", "type": "commandExecution", "status": "inProgress", "command": "echo fixture"}})
				write(map[string]any{"id": 7, "method": "item/commandExecution/requestApproval", "params": map[string]string{"threadId": "hub-cli-rpc", "turnId": "cli-turn", "itemId": "cmd", "command": "echo fixture"}})
			} else if len(params.Input) > 0 && strings.Contains(params.Input[0].Text, "question fixture") {
				write(map[string]any{"id": 8, "method": "item/tool/requestUserInput", "params": map[string]any{"threadId": "hub-cli-rpc", "turnId": "cli-turn", "itemId": "q", "questions": []any{map[string]any{"id": "clarify", "header": "Clarify", "question": "Choose", "isOther": true, "isSecret": false, "options": nil}}}})
			} else {
				finish()
			}
		case "turn/interrupt":
			reply(map[string]any{})
			notify("turn/completed", map[string]any{"turn": map[string]string{"id": "cli-turn", "status": "interrupted"}})
		case "":
			var decision struct {
				Decision string `json:"decision"`
			}
			json.Unmarshal(frame.Result, &decision)
			if decision.Decision != wantDecision {
				os.Exit(93)
			}
			notify("serverRequest/resolved", map[string]any{"requestId": 7})
			notify("item/completed", map[string]any{"item": map[string]string{"id": "cmd", "type": "commandExecution", "status": "declined"}})
			finish()
		default:
			os.Exit(94)
		}
	}
}

func TestPortableExplicitPrivateKeyFileKeepsHubCommandInputIndependent(t *testing.T) {
	directory, err := privatefs.EnsureDir(filepath.Join(t.TempDir(), "private"))
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
	out, _, err = invoke(t, []string{"--data-dir", dir, "--repl", "--codex-command", exe, "--codex-transport", "exec", "--external-host-approved"}, "first native turn\n/engine codex\nexternal turn\n/engine alva\nreturn native turn\n/session\n/quit\n")
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

func TestExplicitCLIConfigurationFlagsRequireTheirOwnExecutable(t *testing.T) {
	for _, vendor := range []string{"codex", "claude", "pi"} {
		directory := filepath.Join(t.TempDir(), "not-created")
		_, _, err := invoke(t, []string{"--data-dir", directory, "--prompt", "must not run", "--" + vendor + "-config-dir", t.TempDir()}, "")
		if err == nil || !strings.Contains(err.Error(), "corresponding") {
			t.Fatal("CLI configuration without corresponding executable accepted", vendor, err)
		}
		if _, err := os.Stat(directory); !os.IsNotExist(err) {
			t.Fatal("invalid CLI configuration flags created storage", err)
		}
	}
}

func TestExplicitCodexConfigurationFlagBindsPersistedRoute(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	profile, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(t.TempDir(), "data")
	_, _, err = invoke(t, []string{"--data-dir", directory, "--prompt", "explicit fixture profile", "--engine", "codex", "--codex-command", binary, "--codex-transport", "exec", "--codex-config-dir", profile, "--external-host-approved"}, "")
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256([]byte(profile))
	want := "explicit CLI configuration sha256:" + hex.EncodeToString(digest[:])
	inspect(t, directory, func(store *productstore.Store) {
		sessions, err := store.ListSessions(t.Context(), 8)
		if err != nil || len(sessions) != 1 {
			t.Fatal(sessions, err)
		}
		detail, err := store.Detail(t.Context(), sessions[0].ID)
		if err != nil {
			t.Fatal(err)
		}
		for _, branch := range detail.Branches {
			if branch.ID == detail.Session.ActiveBranchID && branch.Target.AccountSource == want {
				return
			}
		}
		t.Fatal("CLI profile flag did not bind the actual persisted route")
	})
}

func TestExplicitCodexCommandDefaultsToActualAppServerWire(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	out, _, err := invoke(t, []string{"--data-dir", filepath.Join(t.TempDir(), "data"), "--prompt", "explicit app-server fixture", "--engine", "codex", "--codex-command", binary, "--external-host-approved"}, "")
	if err != nil || out != "app-server fixture answer\n" {
		t.Fatal("stock Codex command did not use actual app-server argv/wire", out, err)
	}
}

func TestAppServerCLIDefaultDeniesPermissionAndQuestionsFailClearly(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	for _, prompt := range []string{"permission fixture", "question fixture"} {
		out, _, err := invoke(t, []string{"--data-dir", filepath.Join(t.TempDir(), "data"), "--prompt", prompt, "--engine", "codex", "--codex-command", binary, "--external-host-approved", "--timeout", "5s"}, "")
		if prompt == "permission fixture" {
			if err != nil || out != "app-server fixture answer\n" {
				t.Fatal("native permission was not denied through owned CLI callback", out, err)
			}
		} else if err == nil || !strings.Contains(err.Error(), "native human questions require Hub desktop") || out != "" {
			t.Fatal("native question hung or silently answered", out, err)
		}
	}
}

type nativeConsentDiagnostic struct {
	input *io.PipeWriter
	asked bool
	done  chan error
}

func (w *nativeConsentDiagnostic) Write(data []byte) (int, error) {
	text := string(data)
	if index := strings.Index(text, "Type allow "); index >= 0 {
		fields := strings.Fields(text[index:])
		if len(fields) < 3 {
			return 0, errors.New("native consent prompt missing handle")
		}
		w.asked = true
		go func() {
			_, err := io.WriteString(w.input, "allow "+fields[2]+"\n/quit\n")
			w.done <- errors.Join(err, w.input.Close())
		}()
	}
	return len(data), nil
}

func TestAppServerCLIExplicitAskConfirmsCapturedOpaqueHandle(t *testing.T) {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	input, writer := io.Pipe()
	defer input.Close()
	defer writer.Close()
	diagnostic := &nativeConsentDiagnostic{input: writer, done: make(chan error, 1)}
	defer func() {
		_ = input.Close()
		_ = writer.Close()
		if diagnostic.asked {
			if err := <-diagnostic.done; err != nil {
				t.Error(err)
			}
		}
	}()
	var output bytes.Buffer
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	err = run(ctx, []string{"--data-dir", filepath.Join(t.TempDir(), "data"), "--repl", "--permission", "ask", "--prompt", "ask permission fixture", "--engine", "codex", "--codex-command", binary, "--external-host-approved"}, input, &output, diagnostic)
	if err != nil || !diagnostic.asked || output.String() != "app-server fixture answer\n" {
		t.Fatal("native ask did not confirm actual captured callback", output.String(), err)
	}
}

func TestCodexTransportPolicyFlagsRejectBeforeStoreIO(t *testing.T) {
	for _, flags := range [][]string{{"--codex-transport", "exec"}, {"--codex-command", "not-started", "--codex-transport", "unknown"}, {"--codex-command", "not-started", "--codex-sandbox", "danger-full-access"}, {"--codex-command", "not-started", "--codex-approval-policy", "auto"}, {"--codex-command", "not-started", "--codex-transport", "exec", "--codex-sandbox", "read-only"}} {
		dir := filepath.Join(t.TempDir(), "not-created")
		args := append([]string{"--data-dir", dir, "--prompt", "must not run"}, flags...)
		if _, _, err := invoke(t, args, ""); err == nil {
			t.Fatal("invalid Codex native flags accepted", flags)
		}
		if _, err := os.Stat(dir); !errors.Is(err, os.ErrNotExist) {
			t.Fatal("invalid native flags created store", err)
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

func TestPiConfigurationAndTransportColdCLIAdmission(t *testing.T) {
	for _, transport := range []string{"rpc", "exec"} {
		directory := filepath.Join(t.TempDir(), "product")
		_, _, err := invoke(t, []string{"--data-dir", directory, "--sessions", "--pi-command", "not-started", "--pi-config-dir", t.TempDir(), "--pi-transport", transport, "--external-host-approved"}, "")
		if err != nil {
			t.Fatal("explicit Pi source rejected on cold startup", transport, err)
		}
	}
	for _, flags := range [][]string{{"--pi-transport", "rpc"}, {"--pi-command", "not-started", "--pi-transport", "unknown"}, {"--pi-command", "not-started", "--pi-config-dir", filepath.Join(t.TempDir(), "missing"), "--external-host-approved"}} {
		directory := filepath.Join(t.TempDir(), "not-created")
		_, _, err := invoke(t, append([]string{"--data-dir", directory, "--sessions"}, flags...), "")
		if err == nil {
			t.Fatal("invalid Pi source/transport accepted", flags)
		}
		if _, err := os.Stat(directory); !os.IsNotExist(err) {
			t.Fatal("invalid Pi source/transport created storage", err)
		}
	}
}

func TestTurnDeadlineKeepsIdentityOverBareSQLiteInterrupt(t *testing.T) {
	expired, cancel := context.WithTimeout(t.Context(), time.Nanosecond)
	defer cancel()
	<-expired.Done()
	for _, effect := range []error{sqlite3.INTERRUPT, sql.ErrTxDone} {
		if err := ownTurnCancellation(expired, fmt.Errorf("pending approvals: %w", effect)); !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("%v lost the deadline identity: %v", effect, err)
		}
	}
	disk := errors.New("disk full")
	if err := ownTurnCancellation(expired, disk); errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("an unrelated failure was relabelled as a timeout", err)
	}
	if err := ownTurnCancellation(t.Context(), sqlite3.INTERRUPT); errors.Is(err, context.Canceled) || err == nil {
		t.Fatal("a live context must not rewrite the error", err)
	}
}

// The CLI answers approvals inline and reads the answer when the run ends, so
// its own sessions must not park a Turn in a durable wait (the product default).
func TestCLICreatedSessionDisablesDurableWaits(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	if _, _, err := invoke(t, []string{"--data-dir", dir, "--prompt", "one-shot"}, ""); err != nil {
		t.Fatal(err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 {
			t.Fatal(all, err)
		}
		settings, err := s.Settings(t.Context(), all[0].ID)
		if err != nil || settings.DurableWaits {
			t.Fatal("CLI session kept the parking default", settings.DurableWaits, err)
		}
	})
}

func TestREPLRenamePinDeleteManageSessions(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "manage")
	var id string
	out, _, err := invoke(t, []string{"--data-dir", dir, "--repl"}, "  first line of the first prompt \nsecond\n/rename 重命名后\n/pin\n/session\n/quit\n")
	if err != nil {
		t.Fatal(out, err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 || all[0].Title != "重命名后" || !all[0].Pinned {
			t.Fatal("rename/pin not stored", all, err)
		}
		id = all[0].ID
	})
	if _, _, err = invoke(t, []string{"--data-dir", dir, "--repl"}, "/delete "+id+"\n/quit\n"); err != nil {
		t.Fatal(err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 0 {
			t.Fatal("deleted session still listed", all, err)
		}
		detail, err := s.DetailIncludingDeleted(t.Context(), id)
		if err != nil || detail.Session.DeletedAt == 0 {
			t.Fatal("soft delete must keep the record", err)
		}
	})
}

func TestCLITitleComesFromTheSharedRule(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "title")
	if _, _, err := invoke(t, []string{"--data-dir", dir, "--prompt", "  标题来自首行  \n后面的行不进标题"}, ""); err != nil {
		t.Fatal(err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 || all[0].Title != "标题来自首行" {
			t.Fatal("CLI did not use the product title rule", all, err)
		}
	})
}

func TestREPLForkContinuesFromAnExplicitTurnInANewSession(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "fork")
	if _, _, err := invoke(t, []string{"--data-dir", dir, "--prompt", "first prompt to fork from"}, ""); err != nil {
		t.Fatal(err)
	}
	var id, turn string
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 1 {
			t.Fatal("one session expected", all, err)
		}
		id = all[0].ID
		latest, err := s.LatestTurn(t.Context(), id)
		if err != nil {
			t.Fatal(err)
		}
		turn = latest.ID
	})
	_, diagnostic, err := invoke(t, []string{"--data-dir", dir, "--session", id, "--repl"}, "/fork "+turn+"\n/fork turn-missing\n/quit\n")
	if err == nil || !strings.Contains(diagnostic, "Forked session: hub-") {
		t.Fatal("/fork must print the new session id and refuse an unknown Turn", diagnostic, err)
	}
	inspect(t, dir, func(s *productstore.Store) {
		all, err := s.ListSessions(t.Context(), 10)
		if err != nil || len(all) != 2 {
			t.Fatal("the fork is a second session", all, err)
		}
		for _, session := range all {
			if session.ID != id && (session.ForkedFrom == nil || session.ForkedFrom.TurnID != turn) {
				t.Fatal("fork origin", session.ForkedFrom)
			}
		}
	})
}
