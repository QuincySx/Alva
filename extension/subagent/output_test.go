// INPUT: context, encoding/json, errors, testing, agent/core, kernel/abi and llm/wire.
// OUTPUT: Actual SDK child AfterModel authority, partial evidence and immutable tool schema regressions.
// POS: [L4 extension] Exercises child selection using real commit events rather than token-draft fixtures.
package subagent

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type selectedModel struct{ fail bool }

func (m selectedModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "stream-original"}); err != nil {
		return err
	}
	if m.fail {
		return errors.New("stream broke")
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}
func TestSelectedChildUsesActualAfterModelCommit(t *testing.T) {
	for _, fail := range []bool{false, true} {
		p, refs, execution := setup(t, nil, nil)
		p.config.Factory = ChildFactoryFunc(func(ctx context.Context, request ChildRequest) (Child, error) {
			log := core.NewMemorySession(request.Scope.SessionID())
			agent, err := core.NewBuilder(selectedModel{fail: fail}, log).Middleware(core.AfterModel("rewrite", func(_ context.Context, _ abi.RuntimeContext, message *wire.Message) error {
				message.Content = []wire.ContentBlock{{Kind: wire.ContentText, Text: "committed-rewrite"}}
				return nil
			})).Build(ctx)
			return Child{Runner: agent, Session: log}, err
		})
		result, err := execute(t, p, execution, `{"task":"work"}`)
		if (err != nil) != fail {
			t.Fatal("wrong run outcome", err)
		}
		var receipt struct {
			Selected  *wire.Reference `json:"selected_output"`
			Committed bool            `json:"selected_output_committed"`
		}
		if err := json.Unmarshal([]byte(result.Content[0].Text), &receipt); err != nil {
			t.Fatal(err)
		}
		if receipt.Committed == fail || receipt.Selected == nil {
			t.Fatal("wrong selection evidence", receipt)
		}
		page, err := refs.ReadText(context.Background(), receipt.Selected.URI, 0, 4096)
		want := "committed-rewrite"
		if fail {
			want = "stream-original"
		}
		if err != nil || page.Text != want {
			t.Fatal("draft replaced committed answer", page, err)
		}
	}
}
func TestRegisteredChildSchemaDoesNotDependOnActiveRun(t *testing.T) {
	p, _, execution := setup(t, nil, nil)
	state := p.state
	before := state.Tools().Snapshot()
	if err := p.OnRunEnd(context.Background(), abi.RuntimeContext{RunID: execution.RunID}, nil); err != nil {
		t.Fatal(err)
	}
	after := state.Tools().Snapshot()
	if before.Version != after.Version || string(before.Definitions[0].Parameters) != string(after.Definitions[0].Parameters) {
		t.Fatal("unchanged registration changed schema", before, after)
	}
}

type restrictedOutputFactory struct{ ChildFactory }

func (restrictedOutputFactory) GrantableTools() []abi.Tool { return nil }

func TestGrantContextPreservesStablePrefixAndConversationTail(t *testing.T) {
	p, _, e := setup(t, nil, nil)
	p.config.Factory = restrictedOutputFactory{p.config.Factory}
	for _, role := range []wire.Role{wire.RoleUser, wire.RoleTool} {
		stable := wire.TextMessage(wire.RoleSystem, "stable system")
		tail := wire.TextMessage(role, "actual latest input or result")
		request := abi.ModelRequest{Messages: []wire.Message{stable, tail}}
		if err := p.BeforeModel(t.Context(), abi.RuntimeContext{RunID: e.RunID, State: p.state}, &request); err != nil {
			t.Fatal(err)
		}
		if len(request.Messages) != 3 || request.Messages[0].Text() != stable.Text() || request.Messages[1].Role != wire.RoleSystem || request.Messages[2].Role != role || request.Messages[2].Text() != tail.Text() {
			t.Fatal("catalog displaced actual conversation tail", request.Messages)
		}
	}
}

func TestChildRejectsCallbacksAfterActualRunnerExit(t *testing.T) {
	p, _, e := setup(t, nil, nil)
	var late func(abi.Event) error
	p.config.Factory = ChildFactoryFunc(func(_ context.Context, r ChildRequest) (Child, error) {
		log := core.NewMemorySession(r.Scope.SessionID())
		runner := runFunc(func(ctx context.Context, _ []wire.Message, emit func(abi.Event) error) error {
			message := wire.TextMessage(wire.RoleAssistant, "committed")
			records, err := log.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})})
			if err != nil {
				return err
			}
			late = emit
			return emit(abi.Event{Kind: abi.EventMessageCommitted, Message: records[0].Message, Record: &abi.RecordReference{SessionID: log.ID(), RecordID: records[0].ID, Sequence: records[0].Sequence}})
		})
		return Child{Runner: runner, Session: log}, nil
	})
	if _, err := execute(t, p, e, `{"task":"work"}`); err != nil {
		t.Fatal(err)
	}
	if late == nil || late(abi.Event{Kind: abi.EventTextDelta, Text: "late mutation"}) == nil {
		t.Fatal("closed child callback remained live")
	}
}
