// INPUT: context, json, errors, fmt, filepath, strings, testing, agent/core/context, kernel/abi, wire
// OUTPUT: Atomic 85-call failure parity and long-history compaction/reopen regressions
// POS: [Native runtime integration testing] Reproduces concrete SDK/SQLite boundary failures without real providers or side effects.
package session

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type contractModel struct{ toolCalls, invocations, messages int }

func (m *contractModel) Stream(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	m.invocations++
	m.messages = len(request.Messages)
	for i := 0; i < m.toolCalls; i++ {
		call := wire.ToolCall{ID: fmt.Sprintf("call-%d", i), Name: "unused", Arguments: json.RawMessage(`{}`)}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
			return err
		}
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

type rejectPreparation struct{}

func (rejectPreparation) Name() string { return "reject-preparation" }
func (rejectPreparation) PrepareTool(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
	return abi.ToolPreparation{}, errors.New("fixture preparation failure")
}

type neverExecutedTool struct{ executions int }

func (*neverExecutedTool) Name() string        { return "unused" }
func (*neverExecutedTool) Description() string { return "Must not execute after failed preparation" }
func (*neverExecutedTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object"}`)
}
func (tool *neverExecutedTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	tool.executions++
	return wire.ToolResult{CallID: call.ID}, nil
}
func (*neverExecutedTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (*neverExecutedTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectWrite}
}
func (*neverExecutedTool) IsReadOnly(wire.ToolCall) bool { return false }

func TestLargeFailedPreparationHasSameAtomicFactsAcrossBackends(t *testing.T) {
	for _, backend := range []string{"memory", "sqlite"} {
		t.Run(backend, func(t *testing.T) {
			var log abi.PersistentSession
			if backend == "memory" {
				log = core.NewMemorySession("memory")
			} else {
				_, log = repoSession(t)
			}
			model := &contractModel{toolCalls: 85}
			tool := &neverExecutedTool{}
			agent, err := core.NewBuilder(model, log).Tool(tool).Middleware(rejectPreparation{}).Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil })
			if err == nil || !strings.Contains(err.Error(), "fixture preparation failure") || strings.Contains(err.Error(), "append_records") {
				t.Fatal("preparation evidence failed to commit", err)
			}
			records, err := log.Records(t.Context(), 0)
			if err != nil {
				t.Fatal(err)
			}
			intents, outcomes, models := 0, 0, 0
			for _, record := range records {
				if record.ToolIntent != nil {
					intents++
					if !record.ToolIntent.NoSideEffect {
						t.Fatal("failed preparation permits side effect")
					}
				}
				if record.ToolOutcome != nil {
					outcomes++
				}
				if record.ModelOutcome != nil {
					models++
					if record.ModelOutcome.Status != abi.OutcomeSucceeded {
						t.Fatal("completed model misreported")
					}
				}
			}
			if intents != 85 || outcomes != 85 || models != 1 {
				t.Fatalf("incomplete atomic evidence %d/%d/%d", intents, outcomes, models)
			}
			if tool.executions != 0 {
				t.Fatal("tool executed despite failed preparation")
			}
		})
	}
}

func TestLongConversationReopensAndReachesContextCompaction(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "private")
	repo, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "long-history", "")
	if err != nil {
		t.Fatal(err)
	}
	for start := 0; start < 4097; start += 256 {
		var batch []abi.SessionRecord
		for i := start; i < start+256 && i < 4097; i++ {
			batch = append(batch, conversation(fmt.Sprintf("message-%d", i)))
		}
		if _, err = log.AppendRecords(ctx, batch); err != nil {
			t.Fatal(err)
		}
	}
	if err = repo.Close(); err != nil {
		t.Fatal(err)
	}
	repo, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err = repo.OpenSession(ctx, "owner", "long-history")
	if err != nil {
		t.Fatal(err)
	}
	messages, err := log.Messages(ctx)
	if err != nil || len(messages) != 4097 || messages[0].Text() != "message-0" || messages[4096].Text() != "message-4096" {
		t.Fatal("complete history lost on reopen", len(messages), err)
	}
	model := &contractModel{}
	agent, err := core.NewBuilder(model, log).ContextSystem(agentcontext.DefaultSystem(256, 512)).Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(ctx, nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if model.invocations != 1 || model.messages > 30 {
		t.Fatalf("compaction never reached or ineffective: calls=%d messages=%d", model.invocations, model.messages)
	}
	messages, err = log.Messages(ctx)
	if err != nil || len(messages) != 4098 {
		t.Fatal("compaction changed durable transcript", len(messages), err)
	}
}

func TestConversationRestorePreflightsBytesAndMissingMetadata(t *testing.T) {
	for _, fixture := range []string{"oversized", "missing"} {
		t.Run(fixture, func(t *testing.T) {
			repo, log := repoSession(t)
			if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{conversation("must not disappear")}); err != nil {
				t.Fatal(err)
			}
			if fixture == "oversized" {
				if _, err := repo.db.Exec("UPDATE artifacts SET bytes=?", maxRuntimeBytes+1); err != nil {
					t.Fatal(err)
				}
			} else {
				if _, err := repo.db.Exec("PRAGMA foreign_keys=OFF"); err != nil {
					t.Fatal(err)
				}
				if _, err := repo.db.Exec("DELETE FROM artifact_chunks"); err != nil {
					t.Fatal(err)
				}
				if _, err := repo.db.Exec("DELETE FROM artifacts"); err != nil {
					t.Fatal(err)
				}
			}
			if messages, err := log.Messages(t.Context()); err == nil || messages != nil {
				t.Fatal("corrupt metadata returned partial or empty success", err)
			}
		})
	}
}
