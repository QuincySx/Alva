// INPUT: context, JSON, errors, strings, sync, testing, portable utility/references and memory-only SDK.
// OUTPUT: Standalone preferences with a real model consumer and live registry/schema discovery regressions.
// POS: [L4 extension] Ordinary utility Tools use host-selected stores and hooks without native storage or Plugin setup.
package utility_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/extension/utility"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type preferences struct {
	mu       sync.Mutex
	values   map[string]string
	revision uint64
	writes   int
}

func (s *preferences) Preferences(ctx context.Context) (utility.PreferenceSnapshot, error) {
	if err := ctx.Err(); err != nil {
		return utility.PreferenceSnapshot{}, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	values := make(map[string]string, len(s.values))
	for key, value := range s.values {
		values[key] = value
	}
	return utility.PreferenceSnapshot{Values: values, Revision: s.revision}, nil
}

func (s *preferences) SetPreference(ctx context.Context, key, value string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.values == nil {
		s.values = map[string]string{}
	}
	s.values[key] = value
	s.revision++
	s.writes++
	return nil
}

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, request, emit)
}

func TestPreferencesRunWithOneOrdinaryHookAndMemoryStore(t *testing.T) {
	store := &preferences{}
	tool, err := utility.NewConfigTool(store)
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		system := ""
		for _, message := range request.Messages {
			if message.Role == wire.RoleSystem {
				system += message.Text()
			}
		}
		if !strings.Contains(system, "original instruction") {
			t.Fatal("preference projection replaced original instructions")
		}
		if turns == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "set", Name: "config", Arguments: json.RawMessage(`{"action":"set","key":"response.language","value":"zh-TW"}`)}}); err != nil {
				return err
			}
		} else if !strings.Contains(system, `"response.language":"zh-TW"`) {
			t.Fatal("configured store had no real next-request consumer")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	log := core.NewMemorySession("portable-config")
	agent, err := core.NewBuilder(model, log).SystemPrompt("original instruction").Tool(tool).
		Middleware(core.BeforeModel("presentation", func(ctx context.Context, _ abi.RuntimeContext, request *abi.ModelRequest) error {
			return utility.ApplyPreferences(ctx, store, request)
		})).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 2 || store.writes != 1 {
		t.Fatal("standalone preference consumer failed", turns, store.writes, err)
	}
	messages, _ := log.Messages(t.Context())
	for _, message := range messages {
		if strings.Contains(message.Text(), "Session response presentation preferences (advisory") {
			t.Fatal("request projection polluted conversation facts")
		}
	}
}

func TestSearchReadsActualLiveRegistryAndOnlyRetainsSelectedSchema(t *testing.T) {
	refs, _ := agentcontext.NewMemoryReferences("portable-catalog", 1<<20)
	var agent *core.Agent
	retained := 0
	search, err := utility.NewSearchTool(func() abi.ToolRegistrySnapshot { return agent.State().Tools().Snapshot() }, func(ctx context.Context, _ string, raw json.RawMessage) (wire.Reference, error) {
		retained++
		return refs.StoreText(ctx, string(raw))
	})
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			// Added after Build: the search must observe execution's live registry.
			if err := agent.State().Tools().Register(catalogTool{}); err != nil {
				return err
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "search", Name: "tool_search", Arguments: json.RawMessage(`{"query":"late_tool"}`)}}); err != nil {
				return err
			}
		} else {
			id := map[int]string{2: "search", 3: "schema"}[turns]
			found := false
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult == nil || block.ToolResult.CallID != id {
						continue
					}
					text := block.ToolResult.Content[0].Text
					if block.ToolResult.IsError || !strings.Contains(text, "late_tool") {
						t.Fatal("search did not read actual live metadata", text)
					}
					if turns == 2 && retained != 0 {
						t.Fatal("search eagerly stored every tool schema")
					}
					if turns == 3 {
						var receipt struct{ Schema wire.Reference }
						if json.Unmarshal([]byte(text), &receipt) != nil || receipt.Schema.Validate() != nil || retained != 1 {
							t.Fatal("selected schema lost evidence", text, retained)
						}
						page, err := refs.ReadText(ctx, receipt.Schema.URI, 0, 4096)
						if err != nil || !strings.Contains(page.Text, "actual schema body") {
							t.Fatal("selected source not retained", page, err)
						}
					}
					found = true
				}
			}
			if !found {
				t.Fatal("missing real search result")
			}
			if turns == 2 {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "schema", Name: "tool_search", Arguments: json.RawMessage(`{"action":"schema","name":"late_tool"}`)}}); err != nil {
					return err
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err = core.NewBuilder(model, core.NewMemorySession("portable-catalog")).Tool(search).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 3 {
		t.Fatal("standalone catalog failed", turns, err)
	}
}

type catalogTool struct{}

func (catalogTool) Name() string        { return "late_tool" }
func (catalogTool) Description() string { return "metadata only" }
func (catalogTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","description":"actual schema body"}`)
}
func (catalogTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (catalogTool) IsReadOnly(wire.ToolCall) bool           { return true }
func (catalogTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (catalogTool) Execute(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{}, errors.New("catalog must never execute discovered tools")
}

func TestPreferencesRejectUnsafeBackendValuesAndInputs(t *testing.T) {
	store := &preferences{}
	tool, _ := utility.NewConfigTool(store)
	execution := abi.ToolExecutionContext{ActualTool: tool, CallID: "config"}
	for _, raw := range []string{`{"action":"set","key":"api_key","value":"secret"}`, `{"action":"set","key":"response.format","value":"executable"}`, `{"action":"set","action":"list"}`, `{"Action":"list"}`, `{"action":"set","key":"response.language","value":"\ud800"}`} {
		if _, err := tool.Execute(t.Context(), execution, wire.ToolCall{ID: "config", Name: "config", Arguments: json.RawMessage(raw)}); err == nil {
			t.Fatal("invalid configuration accepted", raw)
		}
	}
	if store.writes != 0 {
		t.Fatal("invalid input reached the backend")
	}
	store.values = map[string]string{"api_key": "must not enter model input"}
	request := abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleSystem, "original")}}
	if err := utility.ApplyPreferences(t.Context(), store, &request); err == nil || len(request.Messages) != 1 {
		t.Fatal("unsafe backend output entered request", request, err)
	}
}
