// INPUT: context, encoding/json, errors, filepath, strings, testing/time, SDK/security, native session artifacts.
// OUTPUT: Real next-request config consumer, durable/CAS state, live registry/schema references and cancellable timer regressions.
// POS: [L4 extension] Tests reject placeholder configuration/search and preserve original system and conversation facts.
package utility

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(c context.Context, r abi.ModelRequest, e func(wire.StreamChunk) error) error {
	return f(c, r, e)
}

type fixture struct {
	repo  *session.Repository
	log   *session.Session
	c     Config
	p     *Plugin
	state *abi.AgentState
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	cwd := t.TempDir()
	guard, _ := security.NewGuard(cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	c := Config{Repository: repo, Owner: "owner", SessionID: "native", Cwd: cwd, Guard: guard}
	p, err := New(ctx, c)
	if err != nil {
		t.Fatal(err)
	}
	m := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
	state, _ := abi.NewAgentState(m, log, p.Tools())
	f := &fixture{repo: repo, log: log, c: c, p: p, state: state}
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return f
}
func (f *fixture) start(t *testing.T) {
	t.Helper()
	if err := f.p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "run", State: f.state, Workspace: f.c.Cwd}); err != nil {
		t.Fatal(err)
	}
}
func (f *fixture) call(ctx context.Context, name string, raw string) (wire.ToolResult, error) {
	for _, tool := range f.p.Tools() {
		if tool.Name() == name {
			return tool.Execute(ctx, abi.ToolExecutionContext{ActualTool: tool, Session: f.log, SessionID: "native", RunID: "run", CallID: raw, Workspace: f.c.Cwd}, wire.ToolCall{ID: raw, Name: name, Arguments: json.RawMessage(raw)})
		}
	}
	return wire.ToolResult{}, errors.New("missing tool")
}
func TestConfigActualNextRequestConsumerPersistenceAndCAS(t *testing.T) {
	f := newFixture(t)
	ctx := context.Background()
	other, err := New(ctx, f.c)
	if err != nil {
		t.Fatal(err)
	}
	turn := 0
	seen := false
	m := modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		system := ""
		for _, msg := range r.Messages {
			if msg.Role == wire.RoleSystem {
				system += msg.Text()
			}
		}
		if !strings.Contains(system, "original instruction") {
			return errors.New("original system lost")
		}
		if turn == 1 {
			if strings.Contains(system, "Session response presentation preferences (advisory") {
				return errors.New("preference fabricated before set")
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "actual-call", Name: "config", Arguments: json.RawMessage(`{"action":"set","key":"response.language","value":"zh-TW"}`)}}); err != nil {
				return err
			}
		} else {
			if !strings.Contains(system, `"response.language":"zh-TW"`) {
				return errors.New("config had no actual next-request consumer")
			}
			seen = true
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	a, err := core.NewBuilder(m, f.log).Workspace(f.c.Cwd).SystemPrompt("original instruction").Plugin(f.p).Plugin(builtin.NewSecurityPlugin(f.c.Guard, f.p.Tools())).Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "set response preference")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if !seen {
		t.Fatal("no second actual request")
	}
	restored, err := New(ctx, f.c)
	if err != nil || restored.preferences["response.language"] != "zh-TW" {
		t.Fatal("preference not truly durable", err)
	}
	if err = other.set(ctx, "response.verbosity", "detailed"); !errors.Is(err, session.ErrRevisionConflict) {
		t.Fatal("stale config cache overwrote actual facts", err)
	}
	for _, key := range []string{"api_key", "provider", "permission-mode", "response.language"} {
		value := "anything"
		if err = restored.set(ctx, key, value); err == nil {
			t.Fatal("unsafe/non-enumerated value accepted")
		}
	}
	messages, err := f.log.Messages(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for _, message := range messages {
		if strings.Contains(message.Text(), "Session response presentation preferences") {
			t.Fatal("request-only preference polluted durable conversation")
		}
	}
}

type actualTool struct{}

func (actualTool) Name() string        { return "actual_selected" }
func (actualTool) Description() string { return strings.Repeat("descriptive ", 1000) + " late keyword" }
func (actualTool) ParametersSchema() json.RawMessage {
	raw, _ := json.Marshal(map[string]any{"type": "object", "description": strings.Repeat("full selected schema ", 200)})
	return raw
}
func (actualTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (actualTool) IsReadOnly(wire.ToolCall) bool           { return true }
func (actualTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (actualTool) Execute(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{}, errors.New("catalog must not execute tool")
}
func TestToolSearchRealLiveRegistryAndSelectedSchemaReference(t *testing.T) {
	f := newFixture(t)
	f.start(t)
	if err := f.state.Tools().Register(actualTool{}); err != nil {
		t.Fatal(err)
	}
	result, err := f.call(context.Background(), "tool_search", `{"query":"actual_selected"}`)
	if err != nil || !strings.Contains(result.Content[0].Text, "actual_selected") || strings.Contains(result.Content[0].Text, "full selected schema") {
		t.Fatal("not actual bounded registry search", err)
	}
	result, err = f.call(context.Background(), "tool_search", `{"action":"schema","name":"actual_selected"}`)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(result.Content[0].Text, "full selected schema") {
		t.Fatal("large schema inline")
	}
	var value struct{ Schema wire.Reference }
	if err = json.Unmarshal([]byte(result.Content[0].Text), &value); err != nil {
		t.Fatal(err)
	}
	_, data, err := f.repo.ReadArtifactFull(context.Background(), "owner", "native", value.Schema.URI, 64<<10)
	if err != nil || !strings.Contains(string(data), "full selected schema") {
		t.Fatal("actual schema not retained", err)
	}
}
func TestRealSleepCancelAndBounds(t *testing.T) {
	f := newFixture(t)
	f.start(t)
	started := time.Now()
	if _, err := f.call(context.Background(), "sleep", `{"duration_ms":10}`); err != nil || time.Since(started) < 8*time.Millisecond {
		t.Fatal("fake sleep", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, err := f.call(ctx, "sleep", `{"duration_ms":300000}`); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("timer ignored cancellation", err)
	}
	if _, err := f.call(context.Background(), "sleep", `{"duration_ms":300001}`); err == nil {
		t.Fatal("sleep over cap")
	}
	for _, tool := range f.p.Tools() {
		if tool.Name() == "sleep" && tool.Scheduling(wire.ToolCall{}).ExecutionMode != abi.Coordinator {
			t.Fatal("sleep can hold global execution lock")
		}
	}
}
