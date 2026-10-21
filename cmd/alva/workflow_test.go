// INPUT: Actual CLI controls, private session storage, local SSE providers and standalone JS/WASIp1 plugin fixtures.
// OUTPUT: Cold review/approval, per-turn lifecycle and restart-safe durable execution acceptance.
// POS: [L6 application] Exercises the shipped entry and real protocol peers without paid models or discovered configuration.
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/harness/durable"
	durablejournal "github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func workflowRun(t *testing.T, args ...string) (string, string, error) {
	t.Helper()
	var out, diag bytes.Buffer
	err := run(t.Context(), args, strings.NewReader(""), &out, &diag)
	return out.String(), diag.String(), err
}
func workflowSession(t *testing.T, dir string) string {
	t.Helper()
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	list, err := repo.List(t.Context(), cliOwner, "", 10)
	if err != nil || len(list.Sessions) != 1 {
		t.Fatal(list, err)
	}
	return list.Sessions[0].ID
}
func workflowJSONFile(t *testing.T, path string, value any) {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
}
func workflowJS(t *testing.T) (string, string, string) {
	t.Helper()
	node, err := exec.LookPath("node")
	if err != nil {
		t.Skip("real Node interpreter unavailable")
	}
	node, err = filepath.Abs(node)
	if err != nil {
		t.Fatal(err)
	}
	source := t.TempDir()
	sdk, err := os.ReadFile("../../integration/protocol/aep/javascript/index.mjs")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(source, "alva-sdk.mjs"), sdk, 0600); err != nil {
		t.Fatal(err)
	}
	manifest := "name = 'cli-fixture'\nversion = '1'\nruntime = 'javascript'\nentry = 'index.mjs'\n"
	if err = os.WriteFile(filepath.Join(source, "alva.toml"), []byte(manifest), 0600); err != nil {
		t.Fatal(err)
	}
	entry := `import {appendFileSync} from 'node:fs';
import {definePlugin, run, textResult} from './alva-sdk.mjs';
appendFileSync(process.env.CLI_START_FILE, 'start\n');
await run(definePlugin({name:'cli-fixture', version:'1', tools:{echo:{description:'Fixture echo',inputSchema:{type:'object'},execute:args=>{
 appendFileSync(process.env.CLI_START_FILE, 'tool\n');
 if(args.hang) return new Promise(()=>{});
 return textResult(args.text ?? 'ok');
}}}}));`
	if err = os.WriteFile(filepath.Join(source, "index.mjs"), []byte(entry), 0600); err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "execution-marker")
	config := filepath.Join(t.TempDir(), "plugins.json")
	workflowJSONFile(t, config, []cliPluginDraft{{ID: "fixture", SourceDirectory: source, Interpreter: node, Env: map[string]string{"CLI_START_FILE": marker, "API_TOKEN": "explicit-test-secret-must-stay-in-source"}}})
	return source, config, marker
}
func workflowInspect(t *testing.T, dir, config string) (string, cliPluginSettings) {
	t.Helper()
	out, diag, err := workflowRun(t, "--data-dir", dir, "--inspect-plugins", config)
	if err != nil {
		t.Fatal(err, diag)
	}
	var state cliPluginSettings
	if err = json.Unmarshal([]byte(out), &state); err != nil || len(state.Plugins) != 1 || state.Plugins[0].Enabled {
		t.Fatal(out, err)
	}
	return workflowSession(t, dir), state
}
func workflowEnable(t *testing.T, dir, id string, state cliPluginSettings) {
	t.Helper()
	_, diag, err := workflowRun(t, "--data-dir", dir, "--session", id, "--enable-plugin", state.Plugins[0].ID, "--approve-plugin-pin", state.Plugins[0].ReviewSHA256)
	if err != nil {
		t.Fatal(err, diag)
	}
}
func workflowReply(w http.ResponseWriter, tool, args, text string) {
	w.Header().Set("Content-Type", "text/event-stream")
	delta := map[string]any{"content": text}
	finish := "stop"
	if tool != "" {
		finish = "tool_calls"
		delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "workflow-call", "type": "function", "function": map[string]string{"name": tool, "arguments": args}}}}
	}
	data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
}
func workflowLive(dir, id, endpoint string) []string {
	return []string{"--data-dir", dir, "--session", id, "--live", "--endpoint", endpoint, "--model", "fixture"}
}

func TestCLIPluginColdPinsFreshTurnsAndTamper(t *testing.T) {
	source, config, marker := workflowJS(t)
	dir := filepath.Join(t.TempDir(), "private")
	id, state := workflowInspect(t, dir, config)
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("inspection executed code", err)
	}
	if _, _, err := workflowRun(t, "--data-dir", dir, "--session", id, "--enable-plugin", "fixture", "--approve-plugin-pin", strings.Repeat("0", 64)); err == nil {
		t.Fatal("unreviewed enable")
	}
	workflowEnable(t, dir, id, state)
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("approval executed code", err)
	}
	settings, err := os.ReadFile(config)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(config, append(append([]byte(nil), settings...), '\n'), 0600); err != nil {
		t.Fatal(err)
	}
	if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--demo", "--prompt", "changed launch settings"); err == nil {
		t.Fatal("changed config bytes accepted")
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("changed config launched code", err)
	}
	if err = os.WriteFile(config, settings, 0600); err != nil {
		t.Fatal(err)
	}
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); workflowReply(w, "", "", "fresh turn") }))
	defer upstream.Close()
	var out, diag bytes.Buffer
	args := append(workflowLive(dir, id, upstream.URL), "--repl")
	if err := run(t.Context(), args, strings.NewReader("/quit\n"), &out, &diag); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("empty REPL executed code", err)
	}
	if err := run(t.Context(), args, strings.NewReader("first\nsecond\n/quit\n"), &out, &diag); err != nil {
		t.Fatal(err, diag.String())
	}
	started, err := os.ReadFile(marker)
	if err != nil || string(started) != "start\nstart\n" || calls.Load() != 2 {
		t.Fatal("peer reused or starts unowned", string(started), calls.Load(), err)
	}
	entries, err := os.ReadDir(filepath.Join(dir, "aep-snapshots"))
	if err != nil || len(entries) != 0 {
		t.Fatal("snapshot not cleaned", entries, err)
	}
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.OpenSession(t.Context(), cliOwner, id)
	if err != nil {
		t.Fatal(err)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(records)
	if bytes.Contains(raw, []byte("explicit-test-secret-must-stay-in-source")) {
		t.Fatal("raw plugin environment entered facts")
	}
	repo.Close()
	file := filepath.Join(source, "index.mjs")
	f, err := os.OpenFile(file, os.O_APPEND|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	_, err = f.WriteString("\n// code changed\n")
	f.Close()
	if err != nil {
		t.Fatal(err)
	}
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--prompt", "must not run")...); err == nil || !strings.Contains(err.Error(), "changed") {
		t.Fatal("changed code executed", err)
	}
	started, _ = os.ReadFile(marker)
	if string(started) != "start\nstart\n" || calls.Load() != 2 {
		t.Fatal("tamper started execution")
	}
	if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--disable-plugin", "fixture"); err != nil {
		t.Fatal(err)
	}
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--prompt", "disabled")...); err != nil {
		t.Fatal(err)
	}
	started, _ = os.ReadFile(marker)
	if string(started) != "start\nstart\n" || calls.Load() != 3 {
		t.Fatal("disabled plugin ran")
	}
	inspected, _, err := workflowRun(t, "--data-dir", dir, "--session", id, "--inspect-plugins", config)
	if err != nil {
		t.Fatal(err)
	}
	var refreshed cliPluginSettings
	if err = json.Unmarshal([]byte(inspected), &refreshed); err != nil || len(refreshed.Plugins) != 1 || refreshed.Plugins[0].Enabled || refreshed.Plugins[0].ReviewSHA256 == state.Plugins[0].ReviewSHA256 {
		t.Fatal("reinspection did not reset approval", inspected, err)
	}

}

func TestCLIDurableExternalRestartBindingAndExplicitAbandon(t *testing.T) {
	workflowRun := workflowProcess
	dir := filepath.Join(t.TempDir(), "private")
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		if n == 1 {
			workflowReply(w, "await_external", `{"request":{"job":"explicit-result-only"}}`, "")
			return
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		raw, _ := json.Marshal(body)
		if !bytes.Contains(raw, []byte("external-answer")) {
			t.Error("resolved delivery absent", string(raw))
		}
		workflowReply(w, "", "", "resumed once")
	}))
	defer upstream.Close()
	out, diag, err := workflowRun(t, "--data-dir", dir, "--live", "--endpoint", upstream.URL, "--model", "fixture", "--durable", "--prompt", "submit")
	if err != nil || !strings.Contains(out, `"status":"waiting"`) || !strings.Contains(diag, "suspended") || calls.Load() != 1 {
		t.Fatal(out, diag, err, calls.Load())
	}
	id := workflowSession(t, dir)
	out, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--waits")
	if err != nil {
		t.Fatal(err)
	}
	var snapshot durable.Snapshot
	if err = json.Unmarshal([]byte(out), &snapshot); err != nil || len(snapshot.State.Requests) != 1 {
		t.Fatal(out, err)
	}
	request := snapshot.State.Requests[0]
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--prompt", "bypass without durable")...); err == nil {
		t.Fatal("normal execution bypassed wait")
	}
	out, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--resume-wait")...)
	if err != nil || !strings.Contains(out, `"status":"waiting"`) || calls.Load() != 1 {
		t.Fatal("unresolved resumed", out, err, calls.Load())
	}
	resultFile := filepath.Join(t.TempDir(), "result.json")
	workflowJSONFile(t, resultFile, map[string]string{"value": "external-answer"})
	resolve := []string{"--data-dir", dir, "--session", id, "--resolve-wait", request.ID, "--wait-binding", request.Binding, "--wait-result-file", resultFile}
	wrong := append([]string(nil), resolve...)
	wrong[7] = strings.Repeat("0", 64)
	if _, _, err = workflowRun(t, wrong...); err == nil {
		t.Fatal("wrong binding accepted")
	}
	if _, _, err = workflowRun(t, resolve...); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 {
		t.Fatal("resolution executed provider")
	}
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--model", "different", "--resume-wait")...); err == nil {
		t.Fatal("changed model consumed checkpoint")
	}
	out, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--resume-wait")...)
	if err != nil || !strings.Contains(out, "resumed once") || calls.Load() != 2 {
		t.Fatal(out, err, calls.Load())
	}
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--resume-wait")...); err == nil || calls.Load() != 2 {
		t.Fatal("checkpoint resumed twice", err)
	}
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	backend, err := durablejournal.Open(t.Context(), repo, cliOwner, id)
	if err != nil {
		t.Fatal(err)
	}
	s, err := backend.Load(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	s, err = backend.Commit(t.Context(), s.Revision, durable.State{Version: 1, Status: durable.Running}, nil)
	if err != nil {
		t.Fatal(err)
	}
	repo.Close()
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--prompt", "unsafe replay")...); err == nil || !strings.Contains(err.Error(), durable.ErrInterrupted.Error()) {
		t.Fatal(err)
	}
	abandon := []string{"--data-dir", dir, "--session", id, "--abandon-wait", "--wait-revision", strconv.FormatUint(s.Revision-1, 10)}
	if _, _, err = workflowRun(t, abandon...); err == nil || !strings.Contains(err.Error(), durable.ErrConflict.Error()) {
		t.Fatal(err)
	}
	abandon[len(abandon)-1] = strconv.FormatUint(s.Revision, 10)
	if _, _, err = workflowRun(t, abandon...); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 2 {
		t.Fatal("abandon replayed execution")
	}
}

func TestCLIDurablePluginApprovalResumesOneExactTool(t *testing.T) {
	_, config, marker := workflowJS(t)
	dir := filepath.Join(t.TempDir(), "private")
	id, state := workflowInspect(t, dir, config)
	workflowEnable(t, dir, id, state)
	tool := "aep_" + cliSHA([]byte("cli-fixture"))[:32]
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) <= 2 {
			workflowReply(w, tool, `{"action":"call","tool":"echo","arguments":{"text":"approved result"}}`, "")
		} else {
			workflowReply(w, "", "", "approved once")
		}
	}))
	defer upstream.Close()
	out, diag, err := workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--durable", "--prompt", "request tool")...)
	if err != nil || !strings.Contains(out, `"status":"waiting"`) {
		t.Fatal(out, diag, err)
	}
	started, _ := os.ReadFile(marker)
	if string(started) != "start\n" {
		t.Fatal("tool executed before approval", string(started))
	}
	var receipt struct {
		Requests []durable.Request `json:"requests"`
	}
	if err = json.Unmarshal([]byte(out), &receipt); err != nil || len(receipt.Requests) != 1 || receipt.Requests[0].Kind != "approval" {
		t.Fatal(out, err)
	}
	if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--disable-plugin", "fixture"); err != nil {
		t.Fatal(err)
	}
	if _, _, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--resume-wait")...); err == nil {
		t.Fatal("changed plugin recipe consumed checkpoint")
	}
	started, _ = os.ReadFile(marker)
	if string(started) != "start\n" {
		t.Fatal("recipe rejection launched code", string(started))
	}
	workflowEnable(t, dir, id, state)
	result := filepath.Join(t.TempDir(), "allow.json")
	workflowJSONFile(t, result, map[string]bool{"allow": true})
	request := receipt.Requests[0]
	if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--resolve-wait", request.ID, "--wait-binding", request.Binding, "--wait-result-file", result); err != nil {
		t.Fatal(err)
	}
	out, diag, err = workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--resume-wait")...)
	if err != nil || !strings.Contains(out, "approved once") || calls.Load() != 3 {
		t.Fatal(out, diag, err, calls.Load())
	}
	started, _ = os.ReadFile(marker)
	if string(started) != "start\nstart\ntool\n" {
		t.Fatal("tool not executed exactly once", string(started))
	}
}

// A failed Close retains both the actual run lease and repository. A later
// physical join allows the same owner to release them, never an orphaned handle.
type workflowResource struct{ complete atomic.Bool }

func (r *workflowResource) Close(context.Context) error {
	if !r.complete.Load() {
		return context.DeadlineExceeded
	}
	return nil
}
func (r *workflowResource) CleanupComplete() bool { return r.complete.Load() }
func TestCLIRetainsLeaseAndStorageUntilActualPluginCleanup(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	log, err := openCLISession(t.Context(), repo, "")
	if err != nil {
		t.Fatal(err)
	}
	_, release, err := repo.BeginRun(t.Context(), cliOwner, log.ID())
	if err != nil {
		t.Fatal(err)
	}
	runtime, err := newCLINative(t.Context(), cliNativeConfig{Repository: repo, Native: log, Diagnostics: io.Discard})
	if err != nil {
		t.Fatal(err)
	}
	resource := &workflowResource{}
	runtime.retain(resource)
	runtime.release = release
	owner := &cliCleanupError{runtime: runtime, repository: repo, cause: context.DeadlineExceeded}
	if err = owner.Close(t.Context()); !errors.Is(err, context.DeadlineExceeded) || owner.CleanupComplete() {
		t.Fatal(err)
	}
	if _, _, err = repo.BeginRun(t.Context(), cliOwner, log.ID()); !errors.Is(err, storage.ErrActive) {
		t.Fatal("live lease released", err)
	}
	if _, err = log.Messages(t.Context()); err != nil {
		t.Fatal("storage closed before join", err)
	}
	resource.complete.Store(true)
	if err = owner.Close(t.Context()); err != nil || !owner.CleanupComplete() {
		t.Fatal(err)
	}
	if _, err = repo.OpenSession(t.Context(), cliOwner, log.ID()); !errors.Is(err, storage.ErrClosed) {
		t.Fatal("storage not closed after join", err)
	}
}

func TestCLIColdControlFlagConflictsDoNotReadCredentials(t *testing.T) {
	for _, args := range [][]string{
		{"--plugins"}, {"--durable"}, {"--waits"}, {"--resume-wait", "--prompt", "unsafe"},
		{"--data-dir", t.TempDir(), "--plugins", "--session", "missing", "--live", "--api-key-stdin"},
		{"--data-dir", t.TempDir(), "--sessions", "--durable"},
	} {
		ctx, cancel := context.WithTimeout(t.Context(), time.Second)
		err := run(ctx, args, nil, io.Discard, io.Discard)
		cancel()
		if err == nil || errors.Is(err, context.DeadlineExceeded) {
			t.Fatal("conflicting operation admitted or read key", args, err)
		}
	}
}

func TestCLIWorkflowProcess(t *testing.T) {
	if os.Getenv("ALVA_CLI_WORKFLOW_HELPER") != "1" {
		t.Skip("subprocess helper needs explicit input")
	}
	index := 0
	for i, arg := range os.Args {
		if arg == "--" {
			index = i + 1
			break
		}
	}
	if index == 0 {
		os.Exit(125)
	}
	if err := run(context.Background(), os.Args[index:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	os.Exit(0)
}
func workflowProcess(t *testing.T, args ...string) (string, string, error) {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.CommandContext(t.Context(), executable, append([]string{"-test.run=^TestCLIWorkflowProcess$", "--"}, args...)...)
	cmd.Env = append(os.Environ(), "ALVA_CLI_WORKFLOW_HELPER=1")
	var out, diag bytes.Buffer
	cmd.Stdout = &out
	cmd.Stderr = &diag
	err = cmd.Run()
	if err != nil {
		err = fmt.Errorf("CLI process: %w: %s", err, diag.String())
	}
	return out.String(), diag.String(), err
}

func TestCLIWASIp1PluginActuallyStartsAndCleansSnapshot(t *testing.T) {
	source := t.TempDir()
	manifest, err := os.ReadFile("../../examples/aep-wasm/alva.toml")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(source, "alva.toml"), manifest, 0600); err != nil {
		t.Fatal(err)
	}
	build := exec.CommandContext(t.Context(), "go", "build", "-mod=readonly", "-o", filepath.Join(source, "plugin.wasm"), ".")
	build.Dir = "../../examples/aep-wasm"
	build.Env = append(os.Environ(), "GOWORK=off", "GOOS=wasip1", "GOARCH=wasm", "CGO_ENABLED=0")
	if out, err := build.CombinedOutput(); err != nil {
		t.Fatalf("build actual plugin: %v %s", err, out)
	}
	file := filepath.Join(t.TempDir(), "plugins.json")
	workflowJSONFile(t, file, []cliPluginDraft{{ID: "wasm", SourceDirectory: source}})
	dir := filepath.Join(t.TempDir(), "private")
	id, state := workflowInspect(t, dir, file)
	workflowEnable(t, dir, id, state)
	out, diag, err := workflowProcess(t, "--data-dir", dir, "--session", id, "--demo", "--prompt", "portable plugin")
	if err != nil || !strings.Contains(out, "Tool result: portable plugin") {
		t.Fatal(out, diag, err)
	}
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	entry, found, err := repo.LatestRecord(t.Context(), cliOwner, id, cliNamespace, "plugin_launch_intent")
	if err != nil || !found || entry.ID == "" {
		t.Fatal("actual embedded startup receipt absent", entry, found, err)
	}
	entries, err := os.ReadDir(filepath.Join(dir, "aep-snapshots"))
	if err != nil || len(entries) != 0 {
		t.Fatal("embedded snapshot cleanup incomplete", entries, err)
	}
}

func TestCLIDurablePluginCancellationJoinsBeforeStoreClose(t *testing.T) {
	_, config, marker := workflowJS(t)
	dir := filepath.Join(t.TempDir(), "private")
	id, state := workflowInspect(t, dir, config)
	workflowEnable(t, dir, id, state)
	tool := "aep_" + cliSHA([]byte("cli-fixture"))[:32]
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		workflowReply(w, tool, `{"action":"call","tool":"echo","arguments":{"hang":true}}`, "")
	}))
	defer upstream.Close()
	out, diag, err := workflowRun(t, append(workflowLive(dir, id, upstream.URL), "--durable", "--prompt", "cancel test")...)
	if err != nil {
		t.Fatal(out, diag, err)
	}
	var receipt struct {
		Requests []durable.Request `json:"requests"`
	}
	if err = json.Unmarshal([]byte(out), &receipt); err != nil || len(receipt.Requests) != 1 {
		t.Fatal(out, err)
	}
	file := filepath.Join(t.TempDir(), "allow.json")
	workflowJSONFile(t, file, map[string]bool{"allow": true})
	request := receipt.Requests[0]
	if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--resolve-wait", request.ID, "--wait-binding", request.Binding, "--wait-result-file", file); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan error, 1)
	go func() {
		done <- run(ctx, append(workflowLive(dir, id, upstream.URL), "--resume-wait"), strings.NewReader(""), io.Discard, io.Discard)
	}()
	deadline := time.NewTimer(10 * time.Second)
	defer deadline.Stop()
	tick := time.NewTicker(10 * time.Millisecond)
	defer tick.Stop()
	entered := false
	for !entered {
		select {
		case err := <-done:
			t.Fatal("returned before actual remote tool", err)
		case <-deadline.C:
			cancel()
			<-done
			t.Fatal("actual remote tool not entered")
		case <-tick.C:
			data, _ := os.ReadFile(marker)
			entered = strings.Contains(string(data), "tool\n")
		}
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal("cancellation lost", err)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("execution did not join")
	}
	entries, err := os.ReadDir(filepath.Join(dir, "aep-snapshots"))
	if err != nil || len(entries) != 0 {
		t.Fatal("peer/snapshot retained after cancellation", entries, err)
	}
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal("repository lock retained", err)
	}
	defer repo.Close()
	_, release, err := repo.BeginRun(t.Context(), cliOwner, id)
	if err != nil {
		t.Fatal("run lease retained after join", err)
	}
	release()
	backend, err := durablejournal.Open(t.Context(), repo, cliOwner, id)
	if err != nil {
		t.Fatal(err)
	}
	snapshot, err := backend.Load(t.Context())
	if err != nil || snapshot.State.Status != durable.Interrupted {
		t.Fatal("cancelled effect mislabeled as safely resumable", snapshot, err)
	}
}

func TestCLIUnsettledPluginStartupNeedsReviewedAbandon(t *testing.T) {
	for _, tryRun := range []bool{false, true} {
		t.Run(strconv.FormatBool(tryRun), func(t *testing.T) {
			dir := filepath.Join(t.TempDir(), "private")
			repo, err := session.Open(t.Context(), dir)
			if err != nil {
				t.Fatal(err)
			}
			log, err := openCLISession(t.Context(), repo, "")
			if err != nil {
				t.Fatal(err)
			}
			id := log.ID()
			if _, err = commitCLILaunch(t.Context(), log, cliPluginPin{ID: "unknown", Runtime: "wasm", ReviewSHA256: strings.Repeat("0", 64)}); err != nil {
				t.Fatal(err)
			}
			repo.Close()
			if tryRun {
				if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--demo", "--prompt", "must not run"); err == nil || !strings.Contains(err.Error(), durable.ErrInterrupted.Error()) {
					t.Fatal("startup automatically replayed", err)
				}
			}
			out, _, err := workflowRun(t, "--data-dir", dir, "--session", id, "--waits")
			if err != nil {
				t.Fatal(err)
			}
			var snapshot durable.Snapshot
			if err = json.Unmarshal([]byte(out), &snapshot); err != nil || !strings.Contains(out, `"plugin_startup_unknown":true`) {
				t.Fatal(out, err)
			}
			if _, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--abandon-wait", "--wait-revision", strconv.FormatUint(snapshot.Revision, 10)); err != nil {
				t.Fatal(err)
			}
			out, _, err = workflowRun(t, "--data-dir", dir, "--session", id, "--demo", "--prompt", "explicit new turn")
			if err != nil || !strings.Contains(out, "Tool result: explicit new turn") {
				t.Fatal(out, err)
			}
		})
	}
}

type workflowModel func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f workflowModel) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, request, emit)
}

func TestCLIVolatileReferencesSurviveFreshAgentTurns(t *testing.T) {
	workspace := t.TempDir()
	file := filepath.Join(workspace, "retained.txt")
	if err := os.WriteFile(file, []byte("retained-across-agent-assembly"), 0600); err != nil {
		t.Fatal(err)
	}
	round := 0
	uri := ""
	readBack := false
	model := workflowModel(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		round++
		var call *wire.ToolCall
		switch round {
		case 1:
			call = &wire.ToolCall{ID: "retain-file", Name: "read_file", Arguments: json.RawMessage(`{"path":"retained.txt"}`)}
		case 2:
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult != nil && block.ToolResult.CallID == "retain-file" {
						for _, result := range block.ToolResult.Content {
							if result.Reference != nil {
								uri = result.Reference.URI
							}
						}
					}
				}
			}
			if uri == "" {
				return errors.New("actual file reference absent")
			}
		case 3:
			args, _ := json.Marshal(map[string]any{"uri": uri, "limit": 256})
			call = &wire.ToolCall{ID: "reuse-reference", Name: "context_read", Arguments: args}
		case 4:
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult != nil && block.ToolResult.CallID == "reuse-reference" && !block.ToolResult.IsError {
						raw, _ := json.Marshal(block.ToolResult)
						readBack = bytes.Contains(raw, []byte("retained-across-agent-assembly"))
					}
				}
			}
			if !readBack {
				return errors.New("second Agent lost the first turn's reference")
			}
		default:
			return errors.New("unexpected model request")
		}
		if call != nil {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: call}); err != nil {
				return err
			}
		} else if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "turn complete"}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	owner, err := newCLINative(t.Context(), cliNativeConfig{Model: model, Workspace: workspace, Mode: security.Ask, Diagnostics: io.Discard})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close(t.Context())
	if err = owner.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "retain file")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(file); err != nil {
		t.Fatal(err)
	}
	if err = owner.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "retrieve prior reference")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if round != 4 || !readBack {
		t.Fatal("volatile session reference did not survive new Agent", round, readBack)
	}
}
