// INPUT: testing, local helper subprocess, core SDK, security, protocol/mcp, storage/session.
// OUTPUT: Lazy ownership, approval, complete redacted journals, references, budgets and cancellation regressions.
// POS: [L4 extension] Managed MCP acceptance without external servers, inherited credentials or paid model accounts.
package managedmcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	protocol "github.com/QuincySx/Alva/go/integration/protocol/mcp"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/platform/system/process"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type fixtureModel struct{}

func (fixtureModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}
func fixture(t *testing.T, handler security.ApprovalHandler, environments ...map[string]string) (*Plugin, *session.Repository, abi.RuntimeContext) {
	t.Helper()
	ctx := context.Background()
	base := t.TempDir()
	repo, err := session.Open(ctx, filepath.Join(base, "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "product", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	guard, err := security.NewGuard(t.TempDir(), nil, nil, security.NewPermissionManager(security.Ask, handler))
	if err != nil {
		t.Fatal(err)
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	var environment map[string]string
	if len(environments) != 0 {
		environment = environments[0]
	}
	p, err := New(ctx, Config{Repository: repo, Owner: "product", NativeID: "native", Guard: guard, Servers: []protocol.Config{{ID: "fixture", Process: process.Config{Command: executable, Args: []string{"-test.run=^TestManagedMCPChild$", "--", "--managed-mcp-child"}, Cwd: base, Env: environment}, ConnectTimeout: 3 * time.Second, RequestTimeout: 3 * time.Second}}})
	if err != nil {
		t.Fatal(err)
	}
	state, err := abi.NewAgentState(fixtureModel{}, log, nil)
	if err != nil {
		t.Fatal(err)
	}
	runtime := abi.RuntimeContext{RunID: "run", State: state}
	t.Cleanup(func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := p.Close(cleanup); err != nil && !errors.Is(err, ErrJournal) {
			t.Error(err)
		}
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return p, repo, runtime
}
func allow(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
	return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
}

func TestMCPConfiguredCredentialIsAbsentFromActualProcessJournal(t *testing.T) {
	const secret = "mcp-opaque-environment-credential"
	p, _, runtime := fixture(t, security.ApprovalFunc(allow), map[string]string{"API_KEY": secret})
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	if _, err := invoke(context.Background(), p, `{"action":"search_tools","server_id":"fixture"}`); err != nil {
		t.Fatal(err)
	}
	if err := p.OnRunEnd(context.Background(), runtime, nil); err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(p.journal.records)
	if bytes.Contains(data, []byte(secret)) || !bytes.Contains(data, []byte("[REDACTED]")) || !bytes.Contains(data, []byte("environment-echo")) {
		t.Fatal("actual configured credential retained or I/O evidence missing", string(data))
	}
}

func TestRebindChildCreatesOwnLazyJournalAndRejectsSiblingGrandparent(t *testing.T) {
	p, repo, runtime := fixture(t, security.ApprovalFunc(allow))
	ctx := context.Background()
	if _, err := repo.Create(ctx, "product", "child", "native"); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Create(ctx, "product", "grandchild", "child"); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Create(ctx, "product", "sibling", ""); err != nil {
		t.Fatal(err)
	}
	parentReceipt, err := p.journal.append(ctx, "run", protocol.IOEvent{Server: "fixture", Direction: "stdout", Frame: json.RawMessage(`{"text":"parent-private"}`)})
	if err != nil {
		t.Fatal(err)
	}
	child, err := p.RebindChild(ctx, "child")
	if err != nil {
		t.Fatal(err)
	}
	defer child.Close(ctx)
	if child.Tool() == p.Tool() || child.journal == p.journal || child.servers[0] == p.servers[0] || child.Stats().Records != 0 {
		t.Fatal("child reused parent worker/cache")
	}
	childLog, _ := repo.OpenSession(ctx, "product", "child")
	state, _ := abi.NewAgentState(fixtureModel{}, childLog, nil)
	if err = child.OnRunStart(ctx, abi.RuntimeContext{RunID: "child-run", State: state}); err != nil {
		t.Fatal(err)
	}
	if child.servers[0].client != nil {
		t.Fatal("child lifecycle eagerly starts MCP")
	}
	childReceipt, err := child.journal.append(ctx, "child-run", protocol.IOEvent{Server: "fixture", Direction: "stderr", Frame: json.RawMessage(`{"text":"child-private"}`)})
	if err != nil {
		t.Fatal(err)
	}
	if childReceipt.Record.SessionID != "child" || p.Stats().Records != 1 || child.Stats().Records != 1 {
		t.Fatal("journal ownership/cache mixed")
	}
	for _, scope := range []struct{ id, uri string }{{"child", parentReceipt.Value.URI}, {"native", childReceipt.Value.URI}, {"sibling", childReceipt.Value.URI}} {
		if _, err = repo.ReadArtifact(ctx, "product", scope.id, scope.uri, 0, 1024); !errors.Is(err, session.ErrNotFound) {
			t.Fatal("ancestor/sibling journal access granted", err)
		}
	}
	for _, id := range []string{"grandchild", "sibling", "native"} {
		if _, err = p.RebindChild(ctx, id); err == nil {
			t.Fatal("non-direct child rebound", id)
		}
	}
	if _, err = child.RebindChild(ctx, "grandchild"); err != nil {
		t.Fatal("actual direct child cannot rebind", err)
	}
	if err = child.OnRunEnd(ctx, abi.RuntimeContext{RunID: "child-run", State: state}, nil); err != nil {
		t.Fatal(err)
	}
	if runtime.State.Session().ID() != "native" {
		t.Fatal("parent runtime mutated")
	}
}

func TestEncodedStreamRedactionIsIdenticalInMemoryAndNativeFacts(t *testing.T) {
	p, _, _ := fixture(t, security.ApprovalFunc(allow))
	ctx := context.Background()
	frames := []map[string]any{{"text": `{"api_key":"json-stderr-secret","safe":"ordinary"}`}, {"base64": base64.StdEncoding.EncodeToString(append([]byte{0xff}, []byte(`{"api_key"  : "binary-stderr-secret`)...))}, {"text": `{"api_key" : "partial-stderr-secret`}}
	for _, frame := range frames {
		data, _ := json.Marshal(frame)
		if _, err := p.journal.append(ctx, "run", protocol.IOEvent{Server: "fixture", Direction: "stderr", Frame: data}); err != nil {
			t.Fatal(err)
		}
	}
	cached, _ := json.Marshal(p.journal.records)
	durable, err := p.journal.log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(durable)
	if string(cached) != string(encoded) {
		t.Fatal("cache/native stream sanitization differs")
	}
	for _, record := range durable {
		var payload struct {
			Frame map[string]json.RawMessage `json:"frame"`
		}
		if json.Unmarshal(record.Payload, &payload) != nil {
			t.Fatal("bad frame")
		}
		var text, rawBase64 string
		json.Unmarshal(payload.Frame["text"], &text)
		json.Unmarshal(payload.Frame["base64"], &rawBase64)
		decoded, _ := base64.StdEncoding.DecodeString(rawBase64)
		if strings.Contains(text, "-secret") || strings.Contains(string(decoded), "-secret") {
			t.Fatal("raw/encoded secret survived durable journal")
		}
		var redacted bool
		json.Unmarshal(payload.Frame["redacted"], &redacted)
		if !redacted {
			t.Fatal("missing explicit processing receipt")
		}
	}
}

func TestRealWorkerSplitAndBinaryStderrRedactionPreservesSafeEvidence(t *testing.T) {
	p, _, runtime := fixture(t, security.ApprovalFunc(allow))
	p.servers[0].config.Process.Args = append(p.servers[0].config.Process.Args, "--stream-redaction")
	ctx := context.Background()
	if err := p.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	if _, err := invoke(ctx, p, `{"action":"search_tools","server_id":"fixture"}`); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		p.journal.mu.Lock()
		count := 0
		for _, record := range p.journal.records {
			var payload struct {
				Direction string `json:"direction"`
			}
			json.Unmarshal(record.Payload, &payload)
			if payload.Direction == "stderr" {
				count++
			}
		}
		p.journal.mu.Unlock()
		if count == 4 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("real framed stderr evidence did not arrive", count)
		}
		time.Sleep(5 * time.Millisecond)
	}
	if err := p.OnRunEnd(ctx, runtime, nil); err != nil {
		t.Fatal(err)
	}
	records, err := p.journal.log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	cached, _ := json.Marshal(p.journal.records)
	durable, _ := json.Marshal(records)
	if string(cached) != string(durable) {
		t.Fatal("real framed cache and durable records diverged")
	}
	var texts []string
	redacted := 0
	for _, record := range records {
		var payload struct {
			Direction string `json:"direction"`
			Frame     struct {
				Text     string `json:"text"`
				Base64   string `json:"base64"`
				Redacted bool   `json:"redacted"`
				Complete bool   `json:"complete"`
			} `json:"frame"`
		}
		if json.Unmarshal(record.Payload, &payload) != nil || payload.Direction != "stderr" {
			continue
		}
		text := payload.Frame.Text
		if payload.Frame.Base64 != "" {
			body, err := base64.StdEncoding.DecodeString(payload.Frame.Base64)
			if err != nil {
				t.Fatal(err)
			}
			text = string(body)
		}
		if strings.Contains(text, "-secret") || !payload.Frame.Complete {
			t.Fatal("split/raw/binary known secret persisted or framing hidden", text)
		}
		if payload.Frame.Redacted {
			redacted++
		}
		texts = append(texts, text)
	}
	if redacted != 3 || !strings.Contains(strings.Join(texts, ""), `"safe":"structured-safe-evidence"`) || !strings.Contains(strings.Join(texts, ""), "ordinary-unicode-日志🙂\n") {
		t.Fatal("normal evidence removed or known markers missed", texts, redacted)
	}
}
func invoke(ctx context.Context, p *Plugin, args string) (wire.ToolResult, error) {
	return p.gateway.Execute(ctx, abi.ToolExecutionContext{Session: p.journal.log, ActualTool: p.gateway, SessionID: p.id, RunID: "run", CallID: "call", Workspace: p.servers[0].config.Process.Cwd}, wire.ToolCall{ID: "call", Name: "mcp_runtime", Arguments: json.RawMessage(args)})
}

func TestLazyStartupOwnershipAndFailClosedApproval(t *testing.T) {
	var approvals atomic.Int32
	p, _, runtime := fixture(t, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals.Add(1)
		return security.ApprovalDecision{Allowed: false, Scope: security.Once}, nil
	}))
	var opens atomic.Int32
	p.open = func(context.Context, protocol.Config, protocol.Observer) (protocol.Transport, error) {
		opens.Add(1)
		return nil, errors.New("unexpected process startup")
	}
	if opens.Load() != 0 {
		t.Fatal("constructor starts processes")
	}
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	if opens.Load() != 0 || p.Stats().Records != 0 {
		t.Fatal("run start performs MCP I/O")
	}
	if _, err := invoke(context.Background(), p, `{"action":"search_tools"}`); err == nil {
		t.Fatal("unscoped discovery can start every server")
	}
	if opens.Load() != 0 || approvals.Load() != 0 {
		t.Fatal("unscoped discovery reached approval/startup")
	}
	call := wire.ToolCall{ID: "call", Name: "mcp_runtime", Arguments: []byte(`{"action":"list_servers"}`)}
	if _, err := (&gateway{p}).Execute(context.Background(), abi.ToolExecutionContext{SessionID: "child", RunID: "run", CallID: "call"}, call); err == nil {
		t.Fatal("copied parent gateway authorized child")
	}
	if approvals.Load() != 0 || p.Stats().Records != 0 {
		t.Fatal("foreign binding reached approval or journal")
	}
	if _, err := invoke(context.Background(), p, `{"action":"search_tools","server_id":"fixture"}`); !errors.Is(err, security.ErrPermissionDenied) {
		t.Fatalf("approval denial lost: %v", err)
	}
	if opens.Load() != 0 || p.Stats().Records != 0 {
		t.Fatal("denied invocation touched host/journal")
	}
	p.guard, _ = security.NewGuard(t.TempDir(), nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(allow)))
	if _, err := invoke(context.Background(), p, `{"action":"list_servers"}`); err != nil {
		t.Fatal(err)
	}
	if opens.Load() != 0 {
		t.Fatal("list_servers started process")
	}
	if _, err := invoke(context.Background(), p, `{"action":"list_servers","json_path":"$"}`); err == nil {
		t.Fatal("removed JSON-path input accepted")
	}
}

type failingTransport struct {
	closeErr        error
	listErr         error
	closeContextErr error
	calls           int
}

func (*failingTransport) Connected() bool                                      { return true }
func (f *failingTransport) ListTools(context.Context) ([]protocol.Tool, error) { return nil, f.listErr }
func (*failingTransport) CallTool(context.Context, string, json.RawMessage) (json.RawMessage, error) {
	return json.RawMessage(`{}`), nil
}
func (f *failingTransport) Close(ctx context.Context) error {
	f.calls++
	f.closeContextErr = ctx.Err()
	return f.closeErr
}

func TestCloseFailureIsRetainedAndCancelledCleanupStillReaps(t *testing.T) {
	p, _, runtime := fixture(t, security.ApprovalFunc(allow))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	listErr, closeErr := errors.New("catalog fixture failed"), errors.New("cleanup fixture failed")
	transport := &failingTransport{listErr: listErr, closeErr: closeErr}
	p.open = func(context.Context, protocol.Config, protocol.Observer) (protocol.Transport, error) {
		return transport, nil
	}
	if _, err := invoke(context.Background(), p, `{"action":"search_tools","server_id":"fixture"}`); !errors.Is(err, listErr) || !errors.Is(err, closeErr) {
		t.Fatalf("failure lost cleanup evidence: %v", err)
	}
	if p.servers[0].client == nil {
		t.Fatal("failed close discarded owned handle")
	}
	if err := p.OnRunEnd(context.Background(), runtime, nil); !errors.Is(err, closeErr) {
		t.Fatal("failed stop claimed clean exit", err)
	}
	if err := p.OnRunStart(context.Background(), runtime); err == nil {
		t.Fatal("new run reused incomplete cleanup")
	}
	transport.closeErr = nil
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := p.Close(ctx); err != nil {
		t.Fatal("cancelled caller prevented bounded cleanup", err)
	}
	if transport.closeContextErr != nil || p.servers[0].client != nil {
		t.Fatal("cancelled cleanup did not receive its separate cleanup budget")
	}
}

func TestRealWorkerFullRedactedMemoryDurabilityAndBoundedReference(t *testing.T) {
	p, repo, runtime := fixture(t, security.ApprovalFunc(allow))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	result, err := invoke(context.Background(), p, `{"action":"call_tool","server_id":"fixture","tool_name":"echo","arguments":{"api_key":"fixture-secret-only","text":"request-marker"}}`)
	if err != nil {
		t.Fatal(err)
	}
	text := result.Content[0].Text
	if len(text) > 4096 || strings.Contains(text, "complete-tail") || strings.Contains(text, "fixture-secret-only") {
		t.Fatal("raw response/secret entered model output")
	}
	var answer struct {
		Receipt receipt `json:"mcp_interaction_reference"`
	}
	if err = json.Unmarshal([]byte(text), &answer); err != nil || answer.Receipt.Value.URI == "" {
		t.Fatalf("missing reference: %v", err)
	}
	backend, err := repo.BoundReferences(context.Background(), "product", "native")
	if err != nil {
		t.Fatal(err)
	}
	matches, err := backend.SearchText(context.Background(), answer.Receipt.Value.URI, "complete-tail-日志🙂", 20)
	if err != nil || len(matches) != 1 {
		t.Fatalf("tail lost: %+v %v", matches, err)
	}
	if _, err = repo.ReadArtifact(context.Background(), "foreign", "native", answer.Receipt.Value.URI, 0, 2048); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("foreign artifact read", err)
	}
	if err = p.OnRunEnd(context.Background(), runtime, nil); err != nil {
		t.Fatal(err)
	}
	records, err := runtime.State.Session().(*session.Session).Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	cached, _ := json.Marshal(p.journal.records)
	stored, _ := json.Marshal(records)
	if string(cached) != string(stored) {
		t.Fatal("memory and durable frames differ")
	}
	serialized := string(stored)
	for _, needle := range []string{"idle-notification-marker", "stderr-marker", "complete-tail-日志🙂", "[REDACTED]", "request-marker"} {
		if !strings.Contains(serialized, needle) {
			t.Fatalf("missing full frame evidence %s", needle)
		}
	}
	if strings.Contains(serialized, "fixture-secret-only") {
		t.Fatal("secret retained in journal")
	}
	messages, err := runtime.State.Session().Messages(context.Background())
	if err != nil || len(messages) != 0 {
		t.Fatal("MCP log injected conversation", err)
	}
	// Reopen with no Agent/MCP construction and retrieve exactly the committed value.
	if err = repo.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := session.Open(context.Background(), filepath.Join(p.servers[0].config.Process.Cwd, "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	backend, err = reopened.BoundReferences(context.Background(), "product", "native")
	if err != nil {
		t.Fatal(err)
	}
	matches, err = backend.SearchText(context.Background(), answer.Receipt.Value.URI, "complete-tail-日志🙂", 20)
	if err != nil || len(matches) != 1 {
		t.Fatalf("journal not durably readable after reopen: %+v %v", matches, err)
	}
}

func TestBudgetFailureLatchesAndStopsWorker(t *testing.T) {
	p, _, runtime := fixture(t, security.ApprovalFunc(allow))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	p.journal.maxRecords = 2
	_, err := invoke(context.Background(), p, `{"action":"search_tools","server_id":"fixture"}`)
	if !errors.Is(err, ErrJournal) {
		t.Fatalf("journal failure not propagated: %v", err)
	}
	stats := p.Stats()
	if !stats.Failed || stats.Records != 2 {
		t.Fatalf("budget prefix %+v", stats)
	}
	if p.servers[0].client != nil {
		t.Fatal("budget failure left worker owned")
	}
	if _, err = invoke(context.Background(), p, `{"action":"list_servers"}`); !errors.Is(err, ErrJournal) {
		t.Fatal("failed journal reused", err)
	}
	if p.Stats().Records != stats.Records {
		t.Fatal("rejected frame silently appended")
	}
}

func TestCancellationUnknownAndNoAutomaticReconnect(t *testing.T) {
	p, _, runtime := fixture(t, security.ApprovalFunc(allow))
	if err := p.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	if _, err := invoke(context.Background(), p, `{"action":"search_tools","server_id":"fixture"}`); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	_, err := invoke(ctx, p, `{"action":"call_tool","server_id":"fixture","tool_name":"wait"}`)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("cancellation not retained: %v", err)
	}
	if !p.servers[0].failed {
		t.Fatal("cancelled worker may reconnect")
	}
	data, _ := json.Marshal(p.journal.records)
	if !strings.Contains(string(data), "acceptance_unknown") {
		t.Fatal("cancellation uncertainty missing")
	}
	var opens atomic.Int32
	p.open = func(context.Context, protocol.Config, protocol.Observer) (protocol.Transport, error) {
		opens.Add(1)
		return nil, errors.New("must not replay")
	}
	if _, err = invoke(context.Background(), p, `{"action":"call_tool","server_id":"fixture","tool_name":"wait"}`); err == nil {
		t.Fatal("cancelled worker reused")
	}
	if opens.Load() != 0 {
		t.Fatal("operation automatically reconnected/replayed")
	}
}

func TestManagedMCPChild(t *testing.T) {
	child := false
	streamRedaction := false
	for _, arg := range os.Args {
		child = child || arg == "--managed-mcp-child"
		streamRedaction = streamRedaction || arg == "--stream-redaction"
	}
	if !child {
		return
	}
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), protocol.FrameLimit+1)
	for scanner.Scan() {
		var request struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params struct {
				Name    string `json:"name"`
				Version string `json:"protocolVersion"`
			} `json:"params"`
		}
		if json.Unmarshal(scanner.Bytes(), &request) != nil {
			os.Exit(2)
		}
		if request.Method == "notifications/initialized" {
			if secret := os.Getenv("API_KEY"); secret != "" {
				fmt.Fprintln(os.Stderr, "environment-echo: "+secret)
				fmt.Fprintln(os.Stdout, `{"jsonrpc":"2.0","method":"notifications/message","params":{"text":"`+secret+`"}}`)
			}
			if streamRedaction {
				fmt.Fprint(os.Stderr, "api_")
				time.Sleep(20 * time.Millisecond)
				fmt.Fprint(os.Stderr, "key=split-stream-secret\n")
				fmt.Fprintln(os.Stderr, `{"api_key":"json-stream-secret","safe":"structured-safe-evidence"}`)
				os.Stderr.Write(append([]byte{0xff}, []byte(`{"API_KEY"  : "binary-stream-secret"}`+"\n")...))
				fmt.Fprintln(os.Stderr, "ordinary-unicode-日志🙂")
			} else {
				fmt.Fprintln(os.Stderr, "stderr-marker")
			}
			fmt.Fprintln(os.Stdout, `{"jsonrpc":"2.0","method":"notifications/message","params":{"text":"idle-notification-marker","api_key":"fixture-secret-only"}}`)
			continue
		}
		var result any
		switch request.Method {
		case "initialize":
			result = map[string]any{"protocolVersion": request.Params.Version, "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]string{"name": "fixture", "version": "1"}}
		case "tools/list":
			result = map[string]any{"tools": []protocol.Tool{{Name: "echo", Description: "large fixture", InputSchema: []byte(`{"type":"object"}`)}, {Name: "wait", InputSchema: []byte(`{"type":"object"}`)}}}
		case "tools/call":
			if request.Params.Name == "wait" {
				select {}
			}
			result = map[string]any{"content": []map[string]string{{"type": "text", "text": strings.Repeat("raw-日志🙂", 20000) + "complete-tail-日志🙂"}}, "api_key": "fixture-secret-only"}
		default:
			os.Exit(3)
		}
		data, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": result})
		fmt.Fprintln(os.Stdout, string(data))
	}
	os.Exit(0)
}

func TestPluginOnlyRegistersOneGatewayWithoutStartingWorkers(t *testing.T) {
	p, _, _ := fixture(t, security.ApprovalFunc(allow))
	var opens atomic.Int32
	p.open = func(context.Context, protocol.Config, protocol.Observer) (protocol.Transport, error) {
		opens.Add(1)
		return nil, errors.New("unexpected")
	}
	// A normal model turn uses the same actual native authority without eagerly
	// starting any server. Same-ID foreign logs are rejected by the boundary test.
	agent, err := core.NewBuilder(fixtureModel{}, p.journal.log).Plugin(p).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(agent.Snapshot().Tools) != 1 {
		t.Fatalf("schema fan-out: %+v", agent.Snapshot())
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "hello")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if opens.Load() != 0 || p.Stats().Records != 0 {
		t.Fatal("ordinary model turn starts MCP")
	}
}
