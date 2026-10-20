// INPUT: context, JSON/errors, testing/time, interaction/security and actual SDK agents.
// OUTPUT: Wait/resume, exact once-only approvals/questions, expiry and commit/cleanup failure regressions.
// POS: [L5 harness testing] Exercises real model/tool/model boundaries with a volatile pluggable backend.
package durable

import (
	"context"
	"encoding/json"
	"errors"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, r, emit)
}
func call(emit func(wire.StreamChunk) error, id, name, args string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: id, Name: name, Arguments: json.RawMessage(args)}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}
func discard(abi.Event) error { return nil }
func controller(t *testing.T) (*Controller, *MemoryBackend) {
	t.Helper()
	b := NewMemoryBackend(t.Name())
	c, err := New(Config{Backend: b, TTL: time.Hour})
	if err != nil {
		t.Fatal(err)
	}
	return c, b
}
func assemble(t *testing.T, c *Controller, m abi.LanguageModel, tools ...abi.Tool) *core.Agent {
	t.Helper()
	b := core.NewBuilder(m, c.config.Backend.Log()).Plugin(c)
	for _, tool := range tools {
		b.Tool(tool)
	}
	a, err := b.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	return a
}
func resolve(t *testing.T, c *Controller, r Request, value string) {
	t.Helper()
	if err := c.Resolve(t.Context(), Resolution{r.ID, r.Binding, json.RawMessage(value)}); err != nil {
		t.Fatal(err)
	}
}

func TestExternalWaitExitsThenDeliversExactlyOnce(t *testing.T) {
	c, b := controller(t)
	models := 0
	m := modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		models++
		if models == 1 {
			return call(emit, "external", "await_external", `{"request":{"job":"explicit-host-job"}}`)
		}
		if models != 2 || !strings.Contains(r.Messages[len(r.Messages)-1].Text(), `"answer":42`) {
			t.Fatal("external result missing/duplicated", models, r.Messages)
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	a := assemble(t, c, m)
	terminals := 0
	out, err := c.Start(t.Context(), a, []wire.Message{wire.TextMessage(wire.RoleUser, "work")}, func(e abi.Event) error {
		if e.Kind == abi.EventRunFailed || e.Kind == abi.EventRunCompleted {
			terminals++
		}
		return nil
	})
	if err != nil || !out.Suspended || len(out.Requests) != 1 || models != 1 || terminals != 0 {
		t.Fatal("wait pretended completion/failure", out, err, models, terminals)
	}
	r := out.Requests[0]
	if err := c.Resolve(t.Context(), Resolution{r.ID, "changed", json.RawMessage(`{"answer":42}`)}); err == nil {
		t.Fatal("unreviewed binding accepted")
	}
	resolve(t, c, r, `{"answer":42}`)
	resolve(t, c, r, `{"answer":42}`)
	if err := c.Resolve(t.Context(), Resolution{r.ID, r.Binding, json.RawMessage(`{"answer":43}`)}); err == nil {
		t.Fatal("resolved result overwritten")
	}
	out, err = c.Resume(t.Context(), a, func(e abi.Event) error {
		if e.Kind == abi.EventRunCompleted {
			if _, err := c.Snapshot(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
		return nil
	})
	if err != nil || out.Suspended || models != 2 {
		t.Fatal(out, err, models)
	}
	if _, err := c.Resume(t.Context(), a, discard); err == nil || models != 2 {
		t.Fatal("resume replayed completed work", err, models)
	}
	messages, err := b.Log().Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	deliveries := 0
	for _, m := range messages {
		if strings.Contains(m.Text(), `"durable_results"`) {
			deliveries++
		}
	}
	if deliveries != 1 {
		t.Fatal("external result delivered multiple times", deliveries)
	}
}

type guardedTool struct {
	guard    *security.Guard
	executed *int
}

func (*guardedTool) Name() string                            { return "side_effect" }
func (*guardedTool) Description() string                     { return "fixture explicit side effect" }
func (*guardedTool) ParametersSchema() json.RawMessage       { return json.RawMessage(`{"type":"object"}`) }
func (*guardedTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (*guardedTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown, Description: "fixture mutation"}
}
func (*guardedTool) IsReadOnly(wire.ToolCall) bool { return false }
func (t *guardedTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if err := t.guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	*t.executed++
	return wire.ToolResult{}, nil
}
func TestApprovalMatchesExactActionAndIsConsumedOnce(t *testing.T) {
	for _, changed := range []bool{false, true} {
		t.Run(map[bool]string{false: "once", true: "changed arguments"}[changed], func(t *testing.T) {
			c, _ := controller(t)
			executed, models := 0, 0
			// This fixture performs only a counter mutation and has no OS paths.
			// It also runs in WASIp1 with no filesystem preopens.
			workspace := "/workspace"
			if runtime.GOOS == "windows" {
				workspace = `C:\workspace`
			}
			guard, err := security.NewGuard(workspace, nil, nil, security.NewPermissionManager(security.Ask, c))
			if err != nil {
				t.Fatal(err)
			}
			tool := &guardedTool{guard: guard, executed: &executed}
			m := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				models++
				args := `{"value":1}`
				if changed && models > 1 {
					args = `{"value":2}`
				}
				return call(emit, strings.Repeat("c", models), tool.Name(), args)
			})
			a := assemble(t, c, m, tool)
			out, err := c.Start(t.Context(), a, nil, discard)
			if err != nil || !out.Suspended || executed != 0 {
				t.Fatal(out, err, executed)
			}
			resolve(t, c, out.Requests[0], `{"allow":true}`)
			out, err = c.Resume(t.Context(), a, discard)
			want := 1
			if changed {
				want = 0
			}
			if err != nil || !out.Suspended || executed != want || len(out.Requests) != 1 {
				t.Fatal("approval broadened/replayed", out, err, executed)
			}
		})
	}
}
func TestQuestionAnswerSurvivesYieldWithoutAskingAgain(t *testing.T) {
	c, _ := controller(t)
	asks := 0
	tool, err := interaction.NewTool(interaction.ToolConfig{Handler: c})
	if err != nil {
		t.Fatal(err)
	}
	m := modelFunc(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		asks++
		if asks < 3 {
			return call(emit, strings.Repeat("q", asks), tool.Name(), `{"question":"Pick","options":[{"label":"A"},{"label":"B"}]}`)
		}
		last := r.Messages[len(r.Messages)-1].Content[0].ToolResult
		if last == nil || !strings.Contains(last.Content[0].Text, `"text":"B"`) {
			t.Fatal("restored human answer missing", last)
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	a := assemble(t, c, m, tool)
	out, err := c.Start(t.Context(), a, nil, discard)
	if err != nil || !out.Suspended || asks != 1 {
		t.Fatal(out, err)
	}
	var source interaction.Question
	if json.Unmarshal(out.Requests[0].Payload, &source) != nil || source.ID == "" || source.RunID != out.Requests[0].RunID || source.CallID != out.Requests[0].CallID {
		t.Fatal("question source identity was fabricated", source, out.Requests)
	}
	if err := c.Resolve(t.Context(), Resolution{out.Requests[0].ID, out.Requests[0].Binding, json.RawMessage(`{"selected":[9]}`)}); err == nil {
		t.Fatal("invalid option accepted")
	}
	resolve(t, c, out.Requests[0], `{"selected":[2]}`)
	out, err = c.Resume(t.Context(), a, discard)
	if err != nil || out.Suspended || asks != 3 {
		t.Fatal(out, err, asks)
	}
}
func TestWaitExpiryAndInterruptedStateNeverReplay(t *testing.T) {
	c, b := controller(t)
	now := time.Now()
	c.config.Now = func() time.Time { return now }
	models := 0
	a := assemble(t, c, modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		models++
		return call(emit, "c", "await_external", `{"request":{}}`)
	}))
	out, err := c.Start(t.Context(), a, nil, discard)
	if err != nil {
		t.Fatal(err)
	}
	now = now.Add(2 * time.Hour)
	if err := c.Resolve(t.Context(), Resolution{out.Requests[0].ID, out.Requests[0].Binding, json.RawMessage(`{}`)}); !errors.Is(err, ErrExpired) {
		t.Fatal(err)
	}
	if _, err := c.Resume(t.Context(), a, discard); !errors.Is(err, ErrExpired) || models != 1 {
		t.Fatal(err, models)
	}
	if err := c.Abandon(t.Context()); err != nil {
		t.Fatal(err)
	}
	s, _ := b.Load(t.Context())
	s.State.Status = Running
	if _, err := b.Commit(t.Context(), s.Revision, s.State, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := c.Start(t.Context(), a, nil, discard); !errors.Is(err, ErrInterrupted) || models != 1 {
		t.Fatal("running state auto-replayed", err, models)
	}
}
func TestUnsupportedStateAndRawAgentAdmissionFailBeforeIO(t *testing.T) {
	c, b := controller(t)
	models := 0
	a := assemble(t, c, modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { models++; return nil }))
	if err := a.Run(t.Context(), nil, discard); err == nil || models != 0 {
		t.Fatal("raw Agent bypassed wait owner", err, models)
	}
	b.snapshot.State.Version = 2
	if _, err := c.Start(t.Context(), a, nil, discard); err == nil || models != 0 {
		t.Fatal("unsupported checkpoint executed", err, models)
	}
}

func TestSuspensionNeverHidesActualCleanupFailure(t *testing.T) {
	c, _ := controller(t)
	failure := &abi.SessionError{Operation: "flush", SessionID: c.config.Backend.Log().ID(), Err: errors.New("disk unavailable")}
	m := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
		return call(e, "c", "await_external", `{"request":{}}`)
	})
	a, err := core.NewBuilder(m, c.config.Backend.Log()).Plugin(c).Middleware(core.RunEnd("failed cleanup", func(context.Context, abi.RuntimeContext, error) error { return failure })).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	out, err := c.Start(t.Context(), a, nil, discard)
	if !errors.Is(err, failure) || out.Suspended {
		t.Fatal("cleanup failure disguised as durable wait", out, err)
	}
	s, err := c.Snapshot(t.Context())
	if err != nil || s.State.Status != Interrupted {
		t.Fatal("invalid checkpoint after cleanup failure", s, err)
	}
	if onlySuspension(&abi.SessionError{Operation: "append", SessionID: "s", Err: &suspension{}}) {
		t.Fatal("typed commit failure became a suspension")
	}
}
