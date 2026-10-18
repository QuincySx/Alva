// INPUT: testing, real compiled WASIp1 worker, local HTTP model, private native session and explicit grants
// OUTPUT: Actual CLI sandbox/native reference/model/tool/approval/cancel and durable evidence regressions
// POS: [L6 application] Executes the production CLI path with no paid network, credentials discovery or model replay.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	sandboxhost "github.com/QuincySx/Alva/go/platform/sandbox/host"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	sandboxabi "github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

var wasmFixtureOnce sync.Once
var wasmFixture []byte
var wasmFixtureErr error
var fixtureCallID atomic.Uint64

func TestMain(m *testing.M) {
	if handled, err := runWASMShellTrampoline(os.Args[1:]); handled {
		if err != nil {
			fmt.Fprintln(os.Stderr, "private host shell setup failed")
			os.Exit(125)
		}
		os.Exit(0)
	}
	os.Exit(m.Run())
}

func cliWorker(t *testing.T) string {
	t.Helper()
	wasmFixtureOnce.Do(func() {
		directory, err := os.MkdirTemp("", "alva-cli-worker-")
		if err != nil {
			wasmFixtureErr = err
			return
		}
		defer os.RemoveAll(directory)
		target := filepath.Join(directory, "worker.wasm")
		cmd := exec.Command(filepath.Join(runtime.GOROOT(), "bin", "go"), "build", "-mod=readonly", "-o", target, "./apps/cmd/alva-worker-wasm")
		cmd.Dir = "../../.."
		cmd.Env = append(os.Environ(), "GOOS=wasip1", "GOARCH=wasm", "GOPROXY=off")
		if data, err := cmd.CombinedOutput(); err != nil {
			wasmFixtureErr = fmt.Errorf("guest build: %w: %s", err, data)
			return
		}
		wasmFixture, wasmFixtureErr = os.ReadFile(target)
	})
	if wasmFixtureErr != nil {
		t.Fatal(wasmFixtureErr)
	}
	target := filepath.Join(t.TempDir(), "worker.wasm")
	if err := os.WriteFile(target, wasmFixture, 0600); err != nil {
		t.Fatal(err)
	}
	return target
}
func wasmCLIArgs(module, workspace string) []string {
	// The job budget includes native cold compilation of the worker and, for
	// fuel-enabled scripts, both QuickJS variants; race builds need headroom.
	return []string{"--sandbox", "wasm", "--wasm-module", module, "--workspace", workspace, "--grant", workspace + "=/work", "--wasm-timeout", "3m"}
}
func modelCall(w http.ResponseWriter, name string, args any) {
	data, _ := json.Marshal(args)
	delta := map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": fmt.Sprintf("call-%s-%d", name, fixtureCallID.Add(1)), "type": "function", "function": map[string]any{"name": name, "arguments": string(data)}}}}
	payload, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": "tool_calls"}}})
	w.Header().Set("Content-Type", "text/event-stream")
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", payload)
}
func modelText(w http.ResponseWriter, text string) {
	payload, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]string{"content": text}, "finish_reason": "stop"}}})
	w.Header().Set("Content-Type", "text/event-stream")
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", payload)
}

func TestWASMCLIRealNativeAttachmentHistoryAndHostOnlyKey(t *testing.T) {
	module := cliWorker(t)
	workspace := t.TempDir()
	storeDir := filepath.Join(t.TempDir(), "private")
	attachment := filepath.Join(workspace, "log.txt")
	const original = "attachment-explicit-read-marker"
	if err := os.WriteFile(attachment, []byte(strings.Repeat("large unrelated log\n", 2048)+original), 0600); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	repo, err := session.Open(ctx, storeDir)
	if err != nil {
		t.Fatal(err)
	}
	native, err := openCLISession(ctx, repo, "")
	if err != nil {
		t.Fatal(err)
	}
	old := wire.TextMessage(wire.RoleAssistant, "historical-explicit-query-marker")
	if _, err := native.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &old})}); err != nil {
		t.Fatal(err)
	}
	history, err := repo.Page(ctx, cliOwner, native.ID(), 0, 1, 0)
	if err != nil {
		t.Fatal(err)
	}
	oldURI := history.Entries[0].Value.URI
	id := native.ID()
	if err := repo.Close(); err != nil {
		t.Fatal(err)
	}
	const key = "host-transport-only-fixture-key"
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer "+key {
			t.Error("host-only key not used")
		}
		body, _ := io.ReadAll(r.Body)
		if bytes.Contains(body, []byte(key)) || bytes.Contains(body, []byte(workspace)) {
			t.Error("host credentials/path crossed guest model context")
		}
		switch calls.Add(1) {
		case 1:
			if bytes.Contains(body, []byte(original)) || bytes.Contains(body, []byte("historical-explicit-query-marker")) {
				t.Error("original/history automatically injected")
			}
			uri := regexp.MustCompile(`alva-artifact://[0-9a-f]{32}`).FindString(string(body))
			if uri == "" {
				t.Error("missing attachment index")
			}
			modelCall(w, "context_search", map[string]any{"uri": uri, "query": original, "limit": 1})
		case 2:
			if !bytes.Contains(body, []byte(original)) {
				t.Error("explicit attachment search result missing")
			}
			modelCall(w, "native_history", map[string]any{"query": "historical-explicit-query-marker", "limit": 1})
		case 3:
			if !bytes.Contains(body, []byte(oldURI)) {
				t.Error("actual native history record reference missing")
			}
			modelCall(w, "context_read", map[string]any{"uri": oldURI, "offset": 0, "limit": 1024})
		case 4:
			if !bytes.Contains(body, []byte("historical-explicit-query-marker")) {
				t.Error("selected original history unavailable")
			}
			modelText(w, "native references inspected explicitly")
		default:
			t.Error("automatic model replay")
			modelText(w, "unexpected")
		}
	}))
	defer server.Close()
	args := append(wasmCLIArgs(module, workspace), "--live", "--endpoint", server.URL, "--model", "fixture", "--api-key-stdin", "--data-dir", storeDir, "--session", id, "--attach", attachment, "--prompt", "inspect selected references")
	var output, diagnostics bytes.Buffer
	if err := run(ctx, args, strings.NewReader(key+"\n"), &output, &diagnostics); err != nil {
		t.Fatal(err, diagnostics.String())
	}
	if calls.Load() != 4 || output.String() != "native references inspected explicitly\n" {
		t.Fatal("wrong actual result", calls.Load(), output.String())
	}
	repo, err = session.Open(ctx, storeDir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	native, err = repo.OpenSession(ctx, cliOwner, id)
	if err != nil {
		t.Fatal(err)
	}
	records, err := native.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	kinds := map[string]int{}
	for _, r := range records {
		if r.Namespace == "alva.sandbox" {
			kinds[r.Type]++
		}
	}
	if kinds["job_intent"] != 1 || kinds["job_outcome"] != 1 || kinds["io_frame"] < 8 {
		t.Fatal("missing durable job/audit facts", kinds)
	}
	page, err := repo.PageArtifacts(ctx, cliOwner, id, "wasm-io-frame", "", 0, 0, 64)
	if err != nil {
		t.Fatal(err)
	}
	for _, a := range page.Entries {
		_, data, err := repo.ReadArtifactFull(ctx, cliOwner, id, a.Info.Reference().URI, session.MaxArtifactBytes)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(data, []byte(key)) || bytes.Contains(data, []byte(base64Key(key))) {
			t.Fatal("host key leaked into full IO journal")
		}
	}
	foreign, err := repo.Create(ctx, cliOwner, "foreign-native", "")
	if err != nil {
		t.Fatal(err)
	}
	backend, err := repo.BoundReferences(ctx, cliOwner, foreign.ID())
	if err != nil {
		t.Fatal(err)
	}
	reader := wasmNativeReader{repository: repo, sessionID: foreign.ID(), backend: backend}
	if _, err := reader.Read(ctx, sandboxabi.ReferenceRead{Version: 1, URI: oldURI, Limit: 512}); err == nil {
		t.Fatal("foreign native record readable")
	}
}
func base64Key(key string) string { // keep the assertion independent from frame text encoding
	return base64.StdEncoding.EncodeToString([]byte(key))
}

func TestWASMCLIAskIsActuallyReadonly(t *testing.T) {
	module := cliWorker(t)
	workspace := t.TempDir()
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		if calls.Add(1) == 1 {
			if bytes.Contains(body, []byte(`write_file`)) {
				t.Error("Ask advertised guest writes")
			}
			modelCall(w, "write_file", map[string]any{"path": "denied.txt", "content": "must not exist"})
		} else {
			modelText(w, "write was denied; no elevation occurred")
		}
	}))
	defer server.Close()
	args := append(wasmCLIArgs(module, workspace), "--live", "--endpoint", server.URL, "--model", "fixture", "--prompt", "try denied write")
	var output, diagnostics bytes.Buffer
	if err := run(context.Background(), args, strings.NewReader(""), &output, &diagnostics); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(workspace, "denied.txt")); !os.IsNotExist(err) {
		t.Fatal("Ask allowed actual write", err)
	}
	if calls.Load() != 2 || !strings.Contains(diagnostics.String(), "READ-ONLY") {
		t.Fatal("wrong policy evidence", calls.Load(), diagnostics.String())
	}
}

func TestWASMCLIExplicitApprovalEachInvocationAndActualShell(t *testing.T) {
	if runtime.GOOS != "darwin" && runtime.GOOS != "linux" {
		t.Skip("pinned shell supported only darwin/linux")
	}
	module := cliWorker(t)
	workspace := t.TempDir()
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	diagnostics := &challengeWriter{reply: writer}
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		io.Copy(io.Discard, r.Body)
		switch calls.Add(1) {
		case 1:
			modelCall(w, "request_escalation", map[string]any{"command": "printf first > approved.txt", "cwd": "/work", "timeout_ms": 5000})
		case 2:
			modelCall(w, "request_escalation", map[string]any{"command": "printf second >> approved.txt", "cwd": "/work", "timeout_ms": 5000})
		default:
			modelText(w, "two separately approved host commands completed")
		}
	}))
	defer server.Close()
	args := append(wasmCLIArgs(module, workspace), "--live", "--endpoint", server.URL, "--model", "fixture", "--prompt", "explicit host shell requests", "--dangerously-allow-host-shell", "--approval-stdin")
	var output bytes.Buffer
	if err := run(context.Background(), args, reader, &output, diagnostics); err != nil {
		t.Fatal(err, diagnostics.String())
	}
	data, err := os.ReadFile(filepath.Join(workspace, "approved.txt"))
	if err != nil || string(data) != "firstsecond" || diagnostics.approvals != 2 || calls.Load() != 3 {
		t.Fatal("not actual separately approved commands", string(data), err, diagnostics.approvals, calls.Load())
	}
}

type challengeWriter struct {
	bytes.Buffer
	reply     io.Writer
	approvals int
	seen      map[string]bool
}

func (w *challengeWriter) Write(data []byte) (int, error) {
	n, err := w.Buffer.Write(data)
	if err != nil {
		return n, err
	}
	if w.seen == nil {
		w.seen = map[string]bool{}
	}
	for _, challenge := range regexp.MustCompile(`approve [0-9a-f]{16}`).FindAllString(w.Buffer.String(), -1) {
		if !w.seen[challenge] {
			w.seen[challenge] = true
			w.approvals++
			if _, err := io.WriteString(w.reply, challenge+"\n"); err != nil {
				return n, err
			}
		}
	}
	return n, nil
}
func (w *challengeWriter) WriteString(text string) (int, error) { return w.Write([]byte(text)) }

func TestWASMCLIUnsupportedAndConflictingCapabilitiesFailBeforeIO(t *testing.T) {
	for _, args := range [][]string{{"--wasm-fuel", "1"}, {"--sandbox", "other"}, {"--sandbox", "wasm"}, {"--dangerously-allow-host-shell"}, {"--live", "--api-key-stdin", "--approval-stdin"}, {"--live", "--provider", "missing", "--endpoint", "https://invalid.invalid", "--model", "x", "--prompt", "x"}} {
		var output bytes.Buffer
		if err := run(context.Background(), args, strings.NewReader("not-read"), &output, &output); err == nil {
			t.Fatal("invalid authority accepted", args)
		}
	}
}
func TestWASMJournalFramesBeforeSanitizingAndMarksTail(t *testing.T) {
	j := wasmJournal{jobID: "test", pending: map[string][]byte{}}
	ctx := context.Background()
	for _, data := range []string{"prefix api_", "key=known-sensitive\n", "plain-", "尾"} {
		if err := j.Observe(ctx, sandboxhost.IOEvent{Kind: "stderr", Data: []byte(data)}); err != nil {
			t.Fatal(err)
		}
	}
	if err := j.finish(ctx); err != nil {
		t.Fatal(err)
	}
	if len(j.memory) != 2 {
		t.Fatal("chunk not framed", len(j.memory))
	}
	data := strings.Join(func() []string {
		var out []string
		for _, frame := range j.memory {
			out = append(out, string(frame))
		}
		return out
	}(), "\n")
	if strings.Contains(data, "known-sensitive") || strings.Contains(data, base64Key("prefix api_key=known-sensitive\n")) || !strings.Contains(data, "eof_partial") {
		t.Fatal("split marker leaked or tail not identified", data)
	}
}

func TestNativeCLICommittedReplacementIsExplicitNotReplayed(t *testing.T) {
	var calls atomic.Int32
	runner := cliRunnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		calls.Add(1)
		if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: "draft must not be mistaken for final"}); err != nil {
			return err
		}
		time.Sleep(20 * time.Millisecond)
		m := wire.TextMessage(wire.RoleAssistant, "actual committed answer")
		return emit(abi.Event{Kind: abi.EventMessageCommitted, Message: &m, Record: &abi.RecordReference{SessionID: "s", RecordID: "r", Sequence: 1}})
	})
	var output bytes.Buffer
	if err := runRunner(context.Background(), runner, "fixture", &output); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(output.String(), "actual committed answer") || calls.Load() != 1 {
		t.Fatal("wrong committed answer/replay", output.String(), calls.Load())
	}
	if strings.Contains(output.String(), "draft must") && !strings.Contains(output.String(), "replaces the streamed draft") {
		t.Fatal("draft silently passed as final", output.String())
	}
}

func TestWASMApprovalWaitHonorsCancellationWithoutDetachedReader(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { _, err := readApprovalFile(ctx, reader); done <- err }()
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("approval reader ignored cancel")
	}
}

func TestPinnedShellUsesOriginalDirectoryAfterRename(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "original")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	if err := root.WriteFile("marker", []byte("original-pinned"), 0600); err != nil {
		t.Fatal(err)
	}
	moved := directory + "-moved"
	if err := os.Rename(directory, moved); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "marker"), []byte("replacement"), 0600); err != nil {
		t.Fatal(err)
	}
	result, err := executeWASMShell(context.Background(), sandboxhost.EscalationTarget{Directory: root, HostPath: directory, Request: sandboxabi.EscalationRequest{Version: 1, Command: "cat marker", Cwd: "/work", TimeoutMS: 5000}})
	if err != nil {
		t.Fatal(err)
	}
	if result.Stdout != "original-pinned" {
		t.Fatal("shell followed replaced display path", result.Stdout)
	}
}

func TestWASMCLIModelCancellationWaitsActualHTTPAndDoesNotReplay(t *testing.T) {
	module := cliWorker(t)
	workspace := t.TempDir()
	entered := make(chan struct{})
	left := make(chan struct{})
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		io.Copy(io.Discard, r.Body)
		close(entered)
		<-r.Context().Done()
		close(left)
	}))
	defer server.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	var output, diagnostics bytes.Buffer
	args := append(wasmCLIArgs(module, workspace), "--live", "--endpoint", server.URL, "--model", "fixture", "--prompt", "wait for cancellation")
	go func() { done <- run(ctx, args, strings.NewReader(""), &output, &diagnostics) }()
	select {
	case <-entered:
	case <-time.After(time.Minute):
		t.Fatal("actual guest never invoked HTTP model")
	}
	cancel()
	select {
	case err := <-done:
		if err == nil || !errors.Is(err, context.Canceled) {
			t.Fatal("actual cancellation evidence missing", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("CLI failed to cancel actual guest/model")
	}
	select {
	case <-left:
	case <-time.After(time.Second):
		t.Fatal("actual HTTP callback still alive after run returned")
	}
	if calls.Load() != 1 || output.Len() != 0 {
		t.Fatal("cancelled execution replayed or printed success", calls.Load(), output.String())
	}
	if !strings.Contains(diagnostics.String(), "status: cancelled") {
		t.Fatal("pure cancellation misclassified as independent failure", diagnostics.String())
	}
}

func TestNativeCLIUsesAllFourCanonicalProviderProtocols(t *testing.T) {
	streams := map[string]string{
		"openai_chat":      "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"canonical provider\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
		"openai_responses": "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"m\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"canonical provider\"}]}}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"r\",\"status\":\"completed\"}}\n\n",
		"anthropic":        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"r\",\"role\":\"assistant\"}}\n\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"canonical provider\"}}\n\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n",
		"gemini":           "data: {\"responseId\":\"r\",\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"canonical provider\"}]},\"finishReason\":\"STOP\"}]}\n\n",
	}
	for kind, stream := range streams {
		t.Run(kind, func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				io.Copy(io.Discard, r.Body)
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprint(w, stream)
			}))
			defer server.Close()
			var output, diagnostics bytes.Buffer
			if err := run(context.Background(), []string{"--live", "--provider", kind, "--endpoint", server.URL, "--model", "fixture", "--prompt", "selected protocol"}, strings.NewReader(""), &output, &diagnostics); err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(output.String(), "canonical provider") || calls.Load() != 1 {
				t.Fatal("selected protocol ignored/replayed", kind, output.String(), calls.Load())
			}
		})
	}
}
