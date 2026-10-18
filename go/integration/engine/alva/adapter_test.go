// INPUT: testing, app harness, SDK memory session and deterministic local models
// OUTPUT: Alva full-harness native identity, multi-turn, cancellation and reference regressions
// POS: [L7 Alva product adapter] No-network engine boundary tests that do not bypass BaseAgent assembly
package alva

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func TestFactoryExecutionActivityCommitsBeforeNativeWorkAndFailsClosed(t *testing.T) {
	fixtureErr := errors.New("activity durable commit unavailable")
	started := false
	adapter := New(func(ctx context.Context, id string, request engine.Request, _ bool) (harness.Runner, error) {
		if request.ReportActivity == nil {
			t.Fatal("factory received no actual activity boundary")
		}
		if err := request.ReportActivity(engine.Activity{ID: "extension-build", Name: "AEP prepare fixture", Effect: engine.EffectUnknown}); err != nil {
			return nil, err
		}
		started = true
		return nil, errors.New("fixture post-start setup failure")
	})
	err := adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		if event.Kind != engine.ExecutionActivity || event.Activity == nil || started {
			t.Fatal("not a genuine before-start typed event")
		}
		return fixtureErr
	})
	if !errors.Is(err, fixtureErr) || started {
		t.Fatal("native build happened despite failed durable activity admission", err)
	}
	err = adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error { return nil })
	if err == nil || !started {
		t.Fatal("successful activity boundary did not reach real setup fixture")
	}
}

func (f modelFunc) Stream(ctx context.Context, req abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, req, emit)
}
func TestActualHarnessResumesOnlyQualifiedNativeSession(t *testing.T) {
	logs := make(map[string]*core.MemorySession)
	adapter := New(func(ctx context.Context, id string, request engine.Request, resume bool) (harness.Runner, error) {
		log := logs[id]
		if resume && log == nil {
			return nil, engine.ErrSessionNotFound
		}
		if log == nil {
			log = core.NewMemorySession(id)
			logs[id] = log
		}
		model := modelFunc(func(_ context.Context, req abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			if len(req.Messages) < 1 {
				return errors.New("missing history")
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: req.Messages[len(req.Messages)-1].Text()}); err != nil {
				return err
			}
			return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"})
		})
		return harness.NewBuilder(model, log).Build(ctx)
	})
	var id string
	for _, prompt := range []string{"first", "second"} {
		var contract engine.StreamContract
		err := adapter.Execute(context.Background(), engine.Request{Prompt: prompt, ResumeSession: id}, func(event engine.Event) error {
			if event.Kind == engine.SessionStarted {
				if id != "" && id != event.SessionID {
					t.Fatal("native ID changed")
				}
				id = event.SessionID
			}
			return contract.Accept(event)
		})
		if err != nil || !contract.Terminal {
			t.Fatal(err)
		}
	}
	messages, err := logs[id].Messages(context.Background())
	if err != nil || len(messages) != 4 {
		t.Fatal("multi-turn lost or duplicated", len(messages), err)
	}
	if err := adapter.Execute(context.Background(), engine.Request{Prompt: "x", ResumeSession: "foreign"}, func(engine.Event) error { return nil }); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("unknown resume created a session", err)
	}
}
func TestCancelDeliveredButTerminalWaitsForHarness(t *testing.T) {
	started := make(chan string, 1)
	var once sync.Once
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		return harness.NewBuilder(modelFunc(func(ctx context.Context, _ abi.ModelRequest, _ func(wire.StreamChunk) error) error {
			once.Do(func() { started <- id })
			<-ctx.Done()
			return ctx.Err()
		}), core.NewMemorySession(id)).Build(ctx)
	})
	terminal := make(chan engine.Outcome, 1)
	var detail string
	done := make(chan error, 1)
	go func() {
		done <- adapter.Execute(context.Background(), engine.Request{Prompt: "wait"}, func(event engine.Event) error {
			if event.Kind == engine.Error {
				detail = event.Text
			}
			if event.Kind == engine.Completed {
				terminal <- event.Outcome
			}
			return nil
		})
	}()
	id := <-started
	if err := adapter.Cancel(context.Background(), id); err != nil {
		t.Fatal(err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if outcome := <-terminal; outcome != engine.Cancelled {
		t.Fatal(outcome, detail)
	}
}
func TestPermissionCallbackIsExplicitAndOnlyForActualActiveNative(t *testing.T) {
	started := make(chan string, 1)
	responses := 0
	adapter := NewWithPermissions(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		return harness.NewBuilder(modelFunc(func(ctx context.Context, _ abi.ModelRequest, _ func(wire.StreamChunk) error) error {
			started <- id
			<-ctx.Done()
			return ctx.Err()
		}), core.NewMemorySession(id)).Build(ctx)
	}, func(_ context.Context, nativeID, requestID string, decision engine.PermissionDecision) error {
		responses++
		if nativeID == "" || requestID != "actual-request" || !decision.Allow {
			t.Fatal("permission scope was rewritten")
		}
		return nil
	})
	if !adapter.Capabilities().PermissionCallback || New(nil).Capabilities().PermissionCallback {
		t.Fatal("callback support is not truthful")
	}
	done := make(chan error, 1)
	go func() {
		done <- adapter.Execute(context.Background(), engine.Request{Prompt: "wait"}, func(engine.Event) error { return nil })
	}()
	id := <-started
	if err := adapter.RespondPermission(context.Background(), "foreign", "actual-request", engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("foreign approval accepted")
	}
	if err := adapter.RespondPermission(context.Background(), id, "actual-request", engine.PermissionDecision{Allow: true, UpdatedInput: []byte(`{}`)}); err == nil {
		t.Fatal("unsupported input rewrite silently accepted")
	}
	if err := adapter.RespondPermission(context.Background(), id, "actual-request", engine.PermissionDecision{Allow: true}); err != nil || responses != 1 {
		t.Fatal("actual permission callback missing", err)
	}
	if err := adapter.Cancel(context.Background(), id); err != nil {
		t.Fatal(err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if err := adapter.RespondPermission(context.Background(), id, "actual-request", engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("terminal native permission accepted")
	}
}
func TestTypedReasoningRefusalAndOpaqueContentDoNotBecomeAnswer(t *testing.T) {
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		return harness.NewBuilder(modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			for _, chunk := range []wire.StreamChunk{{Kind: wire.ChunkReasoning, Text: "thinking only"}, {Kind: wire.ChunkContentBlock, ContentBlock: &wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: "thinking only", Signature: "must-not-display-signature"}}}, {Kind: wire.ChunkText, Text: "answer only"}, {Kind: wire.ChunkContentBlock, ContentBlock: &wire.ContentBlock{Kind: wire.ContentProviderState, ProviderState: &wire.ProviderState{Provider: "fixture", Data: []byte(`{"opaque":"must-not-display-opaque"}`)}}}, {Kind: wire.ChunkContentBlock, ContentBlock: &wire.ContentBlock{Kind: wire.ContentRefusal, Refusal: &wire.Refusal{Text: "explicit refusal"}}}, {Kind: wire.ChunkFinish}} {
				if err := emit(chunk); err != nil {
					return err
				}
			}
			return nil
		}), core.NewMemorySession(id)).Build(ctx)
	})
	var events []engine.Event
	if err := adapter.Execute(context.Background(), engine.Request{Prompt: "typed"}, func(event engine.Event) error { events = append(events, event); return nil }); err != nil {
		t.Fatal(err)
	}
	answer, thinking, refusal := "", "", ""
	for _, event := range events {
		if strings.Contains(event.Text, "must-not-display") {
			t.Fatal("opaque provider state or signature leaked")
		}
		switch event.Kind {
		case engine.MessageDelta:
			answer += event.Text
		case engine.ReasoningDelta:
			thinking += event.Text
		case engine.ContentBlock:
			if event.Name == "refusal" {
				refusal = event.Text
			}
		}
	}
	if answer != "answer only" || thinking != "thinking only" || refusal != "explicit refusal" {
		t.Fatal("typed messages merged or disappeared", answer, thinking, refusal)
	}
}
