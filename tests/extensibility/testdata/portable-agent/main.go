// INPUT: context, JSON/errors/fmt/os, ordinary extensions and public memory SDK.
// OUTPUT: Independent plan/question/schema CLI fixtures runnable unchanged on native, JS/WASM and WASIp1.
// POS: [verification consumer] Actual default loop use without Plugin, native storage, artifact backend or reader tools.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/extension/planning"
	"github.com/QuincySx/Alva/extension/utility"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

type localModel func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (m localModel) Stream(ctx context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return m(ctx, r, emit)
}

func run() error {
	if len(os.Args) != 2 {
		return errors.New("choose explicit local fixture: plan, question or schema")
	}
	var tool abi.Tool
	var agent *core.Agent
	var args json.RawMessage
	var err error
	var saved string
	invocations := 0
	switch os.Args[1] {
	case "plan":
		tool, err = planning.NewPlanTool(func(_ context.Context, content string) (*wire.Reference, error) {
			saved = content
			invocations++
			return nil, nil
		})
		args = json.RawMessage(`{"content":"standalone fixture plan"}`)
	case "question":
		tool, err = interaction.NewTool(interaction.ToolConfig{Handler: interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
			invocations++
			answer := "explicit fixture answer"
			return interaction.Answer{Text: &answer}, nil
		})})
		args = json.RawMessage(`{"question":"Deterministic fixture question"}`)
	case "schema":
		tool, err = utility.NewSearchTool(utility.SearchConfig{Snapshot: func() abi.ToolRegistrySnapshot { invocations++; return agent.State().Tools().Snapshot() }})
		args = json.RawMessage(`{"action":"schema","name":"tool_search"}`)
	default:
		return errors.New("unsupported fixture mode")
	}
	if err != nil {
		return err
	}
	turns := 0
	output := ""
	model := localModel(func(_ context.Context, r abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if len(r.Tools) != 1 {
				return errors.New("consumer installed unrelated tools")
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "fixture-call", Name: tool.Name(), Arguments: args}}); err != nil {
				return err
			}
		} else if turns == 2 {
			if len(r.Messages) == 0 {
				return errors.New("no settled tool conversation")
			}
			blocks := r.Messages[len(r.Messages)-1].Content
			if len(blocks) != 1 || blocks[0].ToolResult == nil {
				return errors.New("no actual tool result")
			}
			result := blocks[0].ToolResult
			if result.CallID != "fixture-call" || result.IsError || len(result.Content) != 1 {
				return errors.New("tool failed")
			}
			output = result.Content[0].Text
		} else {
			return errors.New("unexpected replay")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err = core.NewBuilder(model, core.NewMemorySession("independent-consumer")).Tool(tool).Build(context.Background())
	if err != nil {
		return err
	}
	if len(agent.Snapshot().Plugins) != 0 {
		return errors.New("consumer unexpectedly needs a Plugin")
	}
	if err := agent.Run(context.Background(), nil, func(abi.Event) error { return nil }); err != nil {
		return err
	}
	if turns != 2 || invocations != 1 || output == "" || os.Args[1] == "plan" && saved != "standalone fixture plan" {
		return errors.New("capability was not actually consumed once")
	}
	_, err = fmt.Fprintln(os.Stdout, output)
	return err
}
