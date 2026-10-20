// INPUT: Actual JavaScript/standalone WASIp1 author fixtures, native loader and public Go SDK loop.
// OUTPUT: End-to-end remote tools, error propagation, isolation and joined cancellation acceptance.
// POS: [L4 extension acceptance] Exercises both transports through identical Plugin/Guard/reference contracts.
package loader

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func portableAgent(t *testing.T, f pluginFixture, args string, result *wire.ToolResult) *core.Agent {
	t.Helper()
	turn := 0
	model := pluginModel(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			call := wire.ToolCall{ID: "portable-call", Name: f.p.Tool().Name(), Arguments: json.RawMessage(`{"action":"call","tool":"echo","arguments":` + args + `}`)}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
				return err
			}
		} else {
			for _, m := range request.Messages {
				for _, block := range m.Content {
					if block.ToolResult != nil && block.ToolResult.CallID == "portable-call" {
						*result = *block.ToolResult
					}
				}
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "done"}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.workspace).Plugin(f.p).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	return agent
}

func TestPortablePluginsActualRemoteToolsAndFailure(t *testing.T) {
	for _, mode := range []string{"javascript-sdk", "wasm-sdk"} {
		for _, failed := range []bool{false, true} {
			name := mode + "/success"
			if failed {
				name = mode + "/failure"
			}
			t.Run(name, func(t *testing.T) {
				f := newPluginFixture(t, mode, nil)
				var result wire.ToolResult
				args := `{"text":"Unicode-完整尾部"}`
				if failed {
					args = `{"fail":true}`
				}
				agent := portableAgent(t, f, args, &result)
				if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "explicit portable plugin")}, func(abi.Event) error { return nil }); err != nil {
					t.Fatal(err)
				}
				if result.CallID != "portable-call" || result.IsError != failed {
					t.Fatalf("actual remote error classification lost: %+v", result)
				}
				var uri string
				for _, block := range result.Content {
					if block.Reference != nil {
						uri = block.Reference.URI
					}
				}
				if uri == "" {
					t.Fatal("remote result did not use bounded reference")
				}
				refs, err := f.repo.BoundReferences(t.Context(), "owner", "native")
				if err != nil {
					t.Fatal(err)
				}
				query := "Unicode-完整尾部"
				if failed {
					query = "fail"
				}
				matches, err := refs.SearchText(t.Context(), uri, query, 5)
				if err != nil || len(matches) != 1 {
					t.Fatalf("actual remote result missing: %v %v", matches, err)
				}
				if !f.p.CleanupComplete() {
					t.Fatal("successful run returned before actual transport/snapshot cleanup")
				}
			})
		}
	}
}

func TestPortablePluginCancellationAfterActualToolEntry(t *testing.T) {
	for _, mode := range []string{"javascript-sdk", "wasm-sdk"} {
		t.Run(mode, func(t *testing.T) {
			f := newPluginFixture(t, mode, nil)
			var result wire.ToolResult
			agent := portableAgent(t, f, `{"hang":true}`, &result)
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			done := make(chan error, 1)
			joined := make(chan struct{})
			go func() {
				defer close(joined)
				done <- agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "cancel entered tool")}, func(abi.Event) error { return nil })
			}()
			defer func() {
				cancel()
				cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
				defer stop()
				_ = f.p.Close(cleanup)
				select {
				case <-joined:
				case <-time.After(5 * time.Second):
					t.Error("Agent work did not join during test cleanup")
				}
			}()
			deadline := time.After(5 * time.Second)
			for {
				f.p.journal.mu.Lock()
				data, _ := json.Marshal(f.p.journal.cache)
				f.p.journal.mu.Unlock()
				if strings.Contains(string(data), "fixture tool entered") {
					break
				}
				select {
				case err := <-done:
					t.Fatalf("run ended before tool entered: %v", err)
				case <-deadline:
					t.Fatal("remote tool did not actually enter")
				case <-time.After(5 * time.Millisecond):
				}
			}
			cancel()
			select {
			case err := <-done:
				if !errors.Is(err, context.Canceled) {
					t.Fatalf("lost real cancellation: %v", err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("actual remote execution survived cancellation")
			}
			if !f.p.CleanupComplete() {
				t.Fatal("cancellation returned without physical execution and snapshot cleanup")
			}
			records, err := f.log.Records(t.Context(), 0)
			if err != nil {
				t.Fatal(err)
			}
			outcomes := 0
			for _, record := range records {
				if record.ToolOutcome != nil && record.ToolOutcome.CallID == "portable-call" {
					outcomes++
					if record.ToolOutcome.Status != abi.OutcomeCancelled {
						t.Fatal("cancelled external effect misclassified")
					}
				}
			}
			if outcomes != 1 {
				t.Fatal("tool cancellation not settled exactly once", outcomes)
			}
		})
	}
}

func TestWASMPluginHasNoAmbientEnvironmentOrFilesystem(t *testing.T) {
	t.Setenv("ALVA_WASM_PRIVATE_FIXTURE", "must-not-be-inherited")
	f := newPluginFixture(t, "wasm-sdk", nil)
	var result wire.ToolResult
	agent := portableAgent(t, f, `{"probe":true}`, &result)
	if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "explicit isolation probe")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	refs, err := f.repo.BoundReferences(t.Context(), "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	for _, block := range result.Content {
		if block.Reference != nil {
			matches, err := refs.SearchText(t.Context(), block.Reference.URI, "env=0;file_denied=true", 1)
			if err != nil || len(matches) != 1 {
				t.Fatal("WASM received ambient host authority", matches, err)
			}
			return
		}
	}
	t.Fatal("isolation probe result missing")
}

func TestWASMPluginCannotChooseNativeInterpreterOrEnvironment(t *testing.T) {
	f := newPluginFixture(t, "wasm-sdk", nil)
	for _, variant := range []string{"interpreter", "args", "environment"} {
		config := f.p.config
		switch variant {
		case "interpreter":
			config.Interpreter = "must-not-execute"
		case "args":
			config.InterpreterArgs = []string{"--dir=/"}
		case "environment":
			config.Env = map[string]string{"ALVA_TEST": "not-delegated"}
		}
		if p, err := OpenPlugin(t.Context(), config); err == nil || p != nil {
			t.Fatal("WASM plugin admitted native authority", variant)
		}
	}
	if _, err := os.Stat(f.p.config.Bundle.Entry()); err != nil {
		t.Fatal("rejected configuration consumed existing snapshot", err)
	}
}
