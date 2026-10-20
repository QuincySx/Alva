// INPUT: Standard library and public Alva SDK contracts/assembly only.
// OUTPUT: Deterministic model-tool-model consumer and an actual cancellation receipt.
// POS: [external SDK example] The application supplies services and lifetime; the SDK supplies the default loop.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Known dependencies use constructor injection. A real application can supply
// another Greeter without changing Tool, Plugin or the kernel.
type Greeter interface {
	Greet(context.Context, string) (string, error)
}

type greetingService struct{ cancel context.CancelFunc }

func (s greetingService) Greet(ctx context.Context, name string) (string, error) {
	if s.cancel != nil {
		// Deterministic cancellation after the actual tool entered: no timer,
		// unjoined background goroutine or pretend pre-cancelled execution.
		s.cancel()
		<-ctx.Done()
		return "", ctx.Err()
	}
	return "Hello, " + name, ctx.Err()
}

type greetTool struct{ service Greeter }

func (greetTool) Name() string        { return "greet" }
func (greetTool) Description() string { return "Return a local greeting" }
func (greetTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":80}},"required":["name"],"additionalProperties":false}`)
}
func (t greetTool) Execute(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var args struct {
		Name string `json:"name"`
	}
	if err := json.Unmarshal(call.Arguments, &args); err != nil || strings.TrimSpace(args.Name) == "" || len(args.Name) > 80 {
		return wire.ToolResult{}, errors.New("invalid greeting arguments")
	}
	text, err := t.service.Greet(ctx, args.Name)
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}, err
}
func (greetTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (greetTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Read an application-supplied greeting service"}
}
func (greetTool) IsReadOnly(wire.ToolCall) bool { return true }

type greetingPlugin struct {
	service Greeter
	before  func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error
}

func (greetingPlugin) Name() string { return "greeting" }
func (p greetingPlugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Tool(greetTool{service: p.service}); err != nil {
		return err
	}
	return r.Middleware(core.BeforeModel("greeting-context", p.before))
}

// This fixture exercises the real model interface without an account. A network
// model is another abi.LanguageModel, selected by the embedding application.
type localModel struct{ calls int }

func (m *localModel) Stream(ctx context.Context, req abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	m.calls++
	if len(req.Messages) == 0 || req.Messages[0].Role != wire.RoleSystem || req.Messages[0].Text() != "Use the local greeting service." || len(req.Tools) != 1 || req.Tools[0].Name != "greet" {
		return errors.New("application contributions missing")
	}
	switch m.calls {
	case 1:
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "greeting-call", Name: "greet", Arguments: json.RawMessage(`{"name":"Alva"}`)}}); err != nil {
			return err
		}
	case 2:
		if len(req.Messages) == 0 {
			return errors.New("tool result missing")
		}
		last := req.Messages[len(req.Messages)-1]
		if len(last.Content) != 1 || last.Content[0].ToolResult == nil || last.Content[0].ToolResult.IsError || len(last.Content[0].ToolResult.Content) != 1 {
			return errors.New("tool result not settled")
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: last.Content[0].ToolResult.Content[0].Text}); err != nil {
			return err
		}
	default:
		return errors.New("unexpected model replay")
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func run(mode string) error {
	if mode != "run" && mode != "cancel" {
		return errors.New("choose run or cancel")
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	service := greetingService{}
	if mode == "cancel" {
		service.cancel = cancel
	}
	model := &localModel{}
	log := core.NewMemorySession("external-sdk-example")
	hooks := 0
	agent, err := core.NewBuilder(model, log).Plugin(greetingPlugin{
		service: service,
		before: func(_ context.Context, _ abi.RuntimeContext, req *abi.ModelRequest) error {
			hooks++
			req.Messages = append([]wire.Message{wire.TextMessage(wire.RoleSystem, "Use the local greeting service.")}, req.Messages...)
			return nil
		},
	}).Build(ctx)
	if err != nil {
		return err
	}
	var committed string
	err = agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "Greet Alva")}, func(event abi.Event) error {
		// Streaming deltas can drive a UI; final output comes from a committed
		// assistant message, never an uncommitted stream draft.
		if event.Kind == abi.EventMessageCommitted && event.Message != nil && event.Message.Text() != "" {
			committed = event.Message.Text()
		}
		return nil
	})
	if mode == "run" && err != nil || mode == "cancel" && !errors.Is(err, context.Canceled) {
		return fmt.Errorf("unexpected run result: %w", err)
	}
	// Inspect settled facts after Run has returned and the cancelled work joined.
	records, err := log.Records(context.Background(), 0)
	if err != nil {
		return err
	}
	tools := 0
	for _, record := range records {
		if record.ToolOutcome == nil {
			continue
		}
		tools++
		expected := abi.OutcomeSucceeded
		if mode == "cancel" {
			expected = abi.OutcomeCancelled
		}
		if record.ToolOutcome.Status != expected {
			return errors.New("actual tool outcome did not settle")
		}
	}
	expectedCalls := 2
	if mode == "cancel" {
		expectedCalls = 1
	}
	if tools != 1 || model.calls != expectedCalls || hooks != expectedCalls || mode == "run" && committed != "Hello, Alva" || mode == "cancel" && committed != "" {
		return errors.New("consumer receipt does not match execution")
	}
	return json.NewEncoder(os.Stdout).Encode(struct {
		Mode       string `json:"mode"`
		Answer     string `json:"answer,omitempty"`
		ModelCalls int    `json:"model_calls"`
		ToolCalls  int    `json:"tool_calls"`
	}{mode, committed, model.calls, tools})
}

func main() {
	mode := "run"
	if len(os.Args) == 2 {
		mode = os.Args[1]
	} else if len(os.Args) > 2 {
		mode = ""
	}
	if err := run(mode); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
