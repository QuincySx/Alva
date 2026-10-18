// INPUT: Real AEP child, checked-in Python wire client, private bundle/session and Go SDK loop
// OUTPUT: Effective arguments, expiring handles, reference-only calls and fail-before-launch regressions
// POS: [L4 extension] Native Go Plugin and explicit Python client acceptance without user code, credentials or model billing
package loader

import (
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/protocol/aep"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestPluginProcessHelper(t *testing.T) {
	mode := ""
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "--loader-mode=") {
			mode = strings.TrimPrefix(arg, "--loader-mode=")
		}
	}
	if mode == "" {
		return
	}
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), aep.FrameLimit+1)
	write := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	var previous string
	counter := 0
	callback := func(method string, params any) map[string]json.RawMessage {
		counter++
		id := fmt.Sprintf("p-%d", counter)
		write(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
		if !scanner.Scan() {
			os.Exit(5)
		}
		var result map[string]json.RawMessage
		if json.Unmarshal(scanner.Bytes(), &result) != nil {
			os.Exit(6)
		}
		return result
	}
	for scanner.Scan() {
		var request struct {
			ID, Method string
			Params     json.RawMessage
		}
		if json.Unmarshal(scanner.Bytes(), &request) != nil {
			os.Exit(2)
		}
		switch request.Method {
		case "initialize":
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": aep.Handshake{ProtocolVersion: aep.Version, Plugin: aep.PluginInfo{Name: "fixture", Version: "1"}, Capabilities: []string{"host:log", "host:state.get_metadata", "host:state.get_messages"}, Subscriptions: pluginEvents, Tools: []aep.Tool{{Name: "remote_echo", Description: strings.Repeat("full description ", 128), InputSchema: json.RawMessage(`{"type":"object","properties":{"text":{"type":"string"}},"additionalProperties":false}`)}}}})
		case "initialized":
			if secret := os.Getenv("API_KEY"); secret != "" {
				fmt.Fprintln(os.Stderr, "environment-echo: "+secret)
				write(map[string]any{"jsonrpc": "2.0", "method": "host/log", "params": map[string]string{"level": "info", "message": secret}})
			}
			if mode == "split-secret" {
				_, _ = os.Stderr.Write([]byte("api_"))
				time.Sleep(10 * time.Millisecond)
				_, _ = os.Stderr.Write([]byte("key=AEP-SPLIT-SECRET\n"))
				_, _ = os.Stderr.Write(append([]byte{0xff}, []byte(` {"api_key" : "AEP-BINARY-SECRET"}`+"\n")...))
			}
			fmt.Fprintln(os.Stderr, "idle stderr original evidence")
			write(map[string]any{"jsonrpc": "2.0", "method": "host/log", "params": map[string]string{"level": "info", "message": "idle notification original evidence"}})
		case "tools/call":
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": map[string]any{"isError": false, "text": strings.Repeat("完整日志", 6000) + "完整尾部-END", "api_key": "AEP-SECRET-NOT-RETAINED"}})
		case "shutdown":
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": map[string]any{}})
			os.Exit(0)
		default:
			if !strings.HasPrefix(request.Method, "extension/") {
				os.Exit(3)
			}
			var params struct {
				StateHandle string        `json:"stateHandle"`
				ToolCall    wire.ToolCall `json:"toolCall"`
			}
			_ = json.Unmarshal(request.Params, &params)
			if previous != "" {
				stale := callback("host/state.get_metadata", map[string]string{"handle": previous})
				var value struct{ Code int }
				if json.Unmarshal(stale["error"], &value) != nil || value.Code != aep.HandleExpired {
					os.Exit(7)
				}
			}
			current := callback("host/state.get_metadata", map[string]string{"handle": params.StateHandle})
			if len(current["result"]) == 0 {
				os.Exit(8)
			}
			if request.Method == "extension/on_llm_call_start" {
				messages := callback("host/state.get_messages", map[string]any{"handle": params.StateHandle, "limit": 2})
				if len(messages["result"]) == 0 {
					os.Exit(9)
				}
			}
			previous = params.StateHandle
			action := aep.Action{Action: "continue"}
			if request.Method == "extension/before_tool_call" && params.ToolCall.Name == "target" {
				switch mode {
				case "modify":
					action = aep.Action{Action: "modify", ModifiedArguments: json.RawMessage(`{"path":"effective"}`)}
				case "replace":
					action = aep.Action{Action: "replace_result", Result: json.RawMessage(`{"is_error":true,"text":"replacement-result"}`)}
				case "block":
					action = aep.Action{Action: "block", Reason: "fixture veto"}
				case "invalid":
					action = aep.Action{Action: "modify_response", Response: json.RawMessage(`{"role":"assistant","content":[]}`)}
				case "hang":
					for scanner.Scan() {
					}
					os.Exit(0)
				}
			}
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": action})
		}
	}
	os.Exit(0)
}

type pluginFixture struct {
	p         *Plugin
	repo      *session.Repository
	log       *session.Session
	workspace string
}

func newPluginFixture(t *testing.T, mode string, before func(context.Context) error, environments ...map[string]string) pluginFixture {
	t.Helper()
	source, parent := t.TempDir(), privateParent(t)
	manifest := writeBundle(t, source, "")
	data, err := os.ReadFile(filepath.Join(source, "alva.toml"))
	if err != nil {
		t.Fatal(err)
	}
	data = []byte(strings.Replace(string(data), `["host:log"]`, `["host:log","host:state.get_metadata","host:state.get_messages"]`, 1))
	if err = os.WriteFile(filepath.Join(source, "alva.toml"), data, 0600); err != nil {
		t.Fatal(err)
	}
	manifest, err = ReadManifest(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	bundle, err := Stage(context.Background(), source, parent, manifest)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = bundle.Close() })
	repo, err := session.Open(context.Background(), filepath.Join(t.TempDir(), "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	log, err := repo.Create(context.Background(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	workspace := t.TempDir()
	files, err := workspacefs.OpenWorkspace(workspace)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = files.Close() })
	guard, err := security.NewGuard(workspace, nil, files, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	if before == nil {
		before = func(context.Context) error { return nil }
	}
	args := []string{"-test.run=TestPluginProcessHelper", "--", "--loader-mode=" + mode}
	env := map[string]string{}
	if mode == "python-sdk" {
		executable = "/usr/bin/python3"
		if _, err := os.Stat(executable); err != nil {
			t.Skip("explicit system Python fixture is unavailable")
		}
		_, filename, _, ok := runtime.Caller(0)
		if !ok {
			t.Fatal("fixture source path unavailable")
		}
		data, err := os.ReadFile(filepath.Join(filepath.Dir(filename), "testdata", "sdk_fixture.py"))
		if err != nil {
			t.Fatal(err)
		}
		// Re-stage from a new approved source, never mutate approved executable
		// bytes after validation. The old exact generated snapshot is recovered.
		if err = bundle.Close(); err != nil {
			t.Fatal(err)
		}
		if err = os.WriteFile(filepath.Join(source, "main.py"), data, 0600); err != nil {
			t.Fatal(err)
		}
		bundle, err = Stage(context.Background(), source, parent, manifest)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = bundle.Close() })
		args = []string{"-s", "-S"}
		env = map[string]string{"PYTHONPATH": filepath.Clean(filepath.Join(filepath.Dir(filename), "../..", "integration", "protocol", "aep", "python")), "PYTHONDONTWRITEBYTECODE": "1"}
	}
	if len(environments) != 0 {
		for key, value := range environments[0] {
			env[key] = value
		}
	}
	p, err := OpenPlugin(context.Background(), PluginConfig{Bundle: bundle, Interpreter: executable, InterpreterArgs: args, Env: env, Workspace: workspace, Repository: repo, Owner: "owner", NativeID: "native", Guard: guard, ApprovedHostAccess: true, BeforeLaunch: before})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := p.Close(ctx); err != nil {
			p.journal.mu.Lock()
			expectedFailure := p.journal.failed
			p.journal.mu.Unlock()
			if !expectedFailure {
				t.Error(err)
			} else if !p.CleanupComplete() {
				t.Error("expected journal failure did not actually clean up the child", err)
			}
		}
		if !p.CleanupComplete() {
			t.Error("Plugin Close did not physically reap its child and remove the snapshot")
		}
	})
	return pluginFixture{p, repo, log, workspace}
}

func TestAEPConfiguredCredentialIsAbsentFromActualProcessJournal(t *testing.T) {
	const secret = "aep-opaque-environment-credential"
	f := newPluginFixture(t, "remote", nil, map[string]string{"API_KEY": secret})
	model := pluginModel(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(context.Background(), nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	records, err := f.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(records)
	if strings.Contains(string(data), secret) || !strings.Contains(string(data), "environment-echo") || !strings.Contains(string(data), "[REDACTED]") {
		t.Fatal("actual configured credential retained or I/O evidence missing", string(data))
	}
}

type pluginModel func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f pluginModel) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, r, emit)
}

type targetTool struct {
	calls     atomic.Int32
	arguments string
}

func (*targetTool) Name() string                      { return "target" }
func (*targetTool) Description() string               { return "real target fixture" }
func (*targetTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (t *targetTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	t.calls.Add(1)
	t.arguments = string(call.Arguments)
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "actual target executed"}}}, nil
}
func (*targetTool) Scheduling(call wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: string(call.Arguments), Access: abi.AccessWrite}}}
}
func (*targetTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectWrite, Description: string(call.Arguments)}
}
func (*targetTool) IsReadOnly(wire.ToolCall) bool { return false }

func TestPluginEffectiveArgumentsAndExpiringStateHandles(t *testing.T) {
	f := newPluginFixture(t, "modify", nil)
	tool := &targetTool{}
	turn := 0
	model := pluginModel(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		for _, definition := range request.Tools {
			if definition.Name == "remote_echo" {
				t.Fatal("full remote tool catalog advertised to model")
			}
		}
		if turn == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "target-call", Name: "target", Arguments: json.RawMessage(`{"path":"declared"}`)}}); err != nil {
				return err
			}
		} else {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "done"}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Tool(tool).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "execute")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if tool.calls.Load() != 1 || tool.arguments != `{"path":"effective"}` {
		t.Fatal("prepared args not actually executed", tool.arguments)
	}
	records, err := f.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	expired := false
	idle := false
	for _, record := range records {
		if record.ToolIntent != nil && record.ToolIntent.Call.ID == "target-call" {
			found = true
			if record.ToolIntent.DeclaredCall == nil || string(record.ToolIntent.DeclaredCall.Arguments) != `{"path":"declared"}` || string(record.ToolIntent.Call.Arguments) != tool.arguments || record.ToolIntent.Scheduling.Resources[0].Resource != tool.arguments {
				t.Fatal("effective intent lost original declaration or scheduling")
			}
		}
		data, _ := json.Marshal(record)
		expired = expired || strings.Contains(string(data), `-32000`)
		idle = idle || strings.Contains(string(data), "idle notification original evidence")
	}
	if !found || !expired || !idle {
		t.Fatal("missing actual intent/expired callback/notification facts", found, expired, idle)
	}
	if _, err = os.Stat(f.p.config.Bundle.Root()); !os.IsNotExist(err) {
		t.Fatal("owned snapshot not reclaimed after actual child exit", err)
	}
}

func TestPluginReplacementAndBlockSkipActualTool(t *testing.T) {
	for _, mode := range []string{"replace", "block", "invalid"} {
		t.Run(mode, func(t *testing.T) {
			f := newPluginFixture(t, mode, nil)
			tool := &targetTool{}
			turn := 0
			model := pluginModel(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				turn++
				if turn == 1 {
					if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "target-call", Name: "target", Arguments: json.RawMessage(`{"path":"declared"}`)}}); err != nil {
						return err
					}
				}
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Tool(tool).Build(context.Background())
			if err != nil {
				t.Fatal(err)
			}
			if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "execute")}, func(abi.Event) error { return nil }); err != nil {
				t.Fatal(err)
			}
			want := int32(0)
			if mode == "invalid" {
				want = 1
			}
			if tool.calls.Load() != want {
				t.Fatal("invalid action applied or replacement/block executed target")
			}
		})
	}
}

func TestPluginRemoteCallRetainsFullRedactedOutputAsReference(t *testing.T) {
	var starts atomic.Int32
	f := newPluginFixture(t, "remote", func(context.Context) error { starts.Add(1); return nil })
	turn := 0
	var result wire.ToolResult
	model := pluginModel(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "remote-call", Name: f.p.Tool().Name(), Arguments: json.RawMessage(`{"action":"call","tool":"remote_echo","arguments":{"text":"hello"}}`)}}); err != nil {
				return err
			}
		} else {
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult != nil {
						result = *block.ToolResult
					}
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "selected remote tool")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if starts.Load() != 1 || result.IsError {
		t.Fatal("launch/rpc outcome invalid")
	}
	encoded, _ := json.Marshal(result)
	if len(encoded) > 4096 || strings.Contains(string(encoded), "完整尾部-END") || strings.Contains(string(encoded), "AEP-SECRET-NOT-RETAINED") {
		t.Fatal("raw output entered model")
	}
	var uri string
	for _, block := range result.Content {
		if block.Reference != nil {
			uri = block.Reference.URI
		}
	}
	if uri == "" {
		t.Fatal("no selected result reference")
	}
	refs, err := f.repo.BoundReferences(context.Background(), "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	matches, err := refs.SearchText(context.Background(), uri, "完整尾部-END", 5)
	if err != nil || len(matches) != 1 {
		t.Fatal("full Unicode tail not retained", err)
	}
	secret, err := refs.SearchText(context.Background(), uri, "AEP-SECRET-NOT-RETAINED", 5)
	if err != nil || len(secret) != 0 {
		t.Fatal("known credential not redacted", err)
	}
}

func TestPluginCancelledPreparationDoesNotExecuteOrReplay(t *testing.T) {
	f := newPluginFixture(t, "hang", nil)
	tool := &targetTool{}
	model := pluginModel(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "target-call", Name: "target", Arguments: json.RawMessage(`{"path":"declared"}`)}}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Tool(tool).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	err = agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "cancel")}, func(abi.Event) error { return nil })
	if err == nil || tool.calls.Load() != 0 {
		t.Fatal("cancelled prepare executed/faked success")
	}
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("lost real cancel cause", err)
	}
	if err = f.p.healthy(); err == nil {
		t.Fatal("closed failed worker reusable")
	}
}

func TestPluginFailedRiskRecordPreventsLaunchAndDoesNotConsumeSnapshot(t *testing.T) {
	f := newPluginFixture(t, "remote", nil)
	source, parent := t.TempDir(), privateParent(t)
	manifest := writeBundle(t, source, "")
	bundle, err := Stage(context.Background(), source, parent, manifest)
	if err != nil {
		t.Fatal(err)
	}
	defer bundle.Close()
	want := errors.New("durable attempt activity commit failed")
	config := f.p.config
	config.Bundle = bundle
	config.Interpreter = "must-not-be-launched"
	config.BeforeLaunch = func(context.Context) error { return want }
	if p, err := OpenPlugin(context.Background(), config); p != nil || !errors.Is(err, want) {
		t.Fatal("failed activity commit allowed launch or lost cause", err)
	}
	if _, err = os.Stat(bundle.Entry()); err != nil {
		t.Fatal("unsuccessful Open consumed caller-owned snapshot", err)
	}
}

func TestPluginJournalBudgetFailsBeforeAnotherDurableWrite(t *testing.T) {
	f := newPluginFixture(t, "remote", nil)
	f.p.journal.mu.Lock()
	f.p.journal.bytes = 64 << 20
	f.p.journal.mu.Unlock()
	before, err := f.repo.Page(context.Background(), "owner", "native", 0, 1, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = f.p.journal.record(context.Background(), "run", "budget-test", map[string]string{"text": "must not commit"}); err == nil {
		t.Fatal("journal silently discarded or exceeded full budget")
	}
	after, err := f.repo.Page(context.Background(), "owner", "native", 0, 1, 0)
	if err != nil || before.HighWater != after.HighWater {
		t.Fatal("over-budget interaction committed", err)
	}
}

func TestCheckedInPythonSDKMutationsUseActualProtocol(t *testing.T) {
	f := newPluginFixture(t, "python-sdk", nil)
	tool := &targetTool{}
	turn := 0
	observedErrorResult := false
	model := pluginModel(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		found := false
		for _, message := range request.Messages {
			found = found || message.Text() == "sdk-system"
			for _, block := range message.Content {
				if block.ToolResult != nil && block.ToolResult.CallID == "target-call" {
					observedErrorResult = block.ToolResult.IsError
				}
			}
		}
		if !found {
			return errors.New("real Python SDK modify_messages not applied")
		}
		if turn == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "target-call", Name: "target", Arguments: json.RawMessage(`{"path":"declared"}`)}}); err != nil {
				return err
			}
		} else {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "done"}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Tool(tool).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "SDK user message")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if tool.arguments != `{"path": "sdk-effective"}` && tool.arguments != `{"path":"sdk-effective"}` {
		t.Fatal("SDK effective arguments not executed", tool.arguments)
	}
	if !observedErrorResult {
		t.Fatal("SDK action result is_error lost")
	}
	messages, err := f.log.Messages(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if messages[len(messages)-1].Text() != "sdk-done" {
		t.Fatal("SDK response mutation text lost")
	}
}

func TestPluginSplitAndBinaryStderrCannotBypassKnownRedaction(t *testing.T) {
	f := newPluginFixture(t, "split-secret", nil)
	model := pluginModel(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "run")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	records, err := f.log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(records)
	if strings.Contains(string(data), "AEP-SPLIT-SECRET") || strings.Contains(string(data), "AEP-BINARY-SECRET") || !strings.Contains(string(data), "REDACTED") {
		t.Fatal("known credential crossed framing/redaction boundary")
	}
	f.p.journal.mu.Lock()
	cache, _ := json.Marshal(f.p.journal.cache)
	f.p.journal.mu.Unlock()
	if strings.Contains(string(cache), "AEP-SPLIT-SECRET") || strings.Contains(string(cache), "AEP-BINARY-SECRET") {
		t.Fatal("memory cache retained known credentials")
	}
}
