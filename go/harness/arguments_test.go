// INPUT: context, encoding/json, filepath, strings, testing, core SDK, native session repository.
// OUTPUT: Executed argument projection, unchanged invocation/facts, source retrieval and deduplication regressions.
// POS: [L5 harness] Product request projection cannot modify current tool execution or hide unresolved call parameters.
package harness

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type exactArgumentsTool struct {
	expected string
	calls    *int
}

func (exactArgumentsTool) Name() string        { return "exact_arguments" }
func (exactArgumentsTool) Description() string { return "Local argument integrity fixture" }
func (exactArgumentsTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object"}`)
}
func (t exactArgumentsTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if string(call.Arguments) != t.expected {
		panic("projection changed actual tool parameters")
	}
	*t.calls++
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "fixture complete"}}}, nil
}
func (exactArgumentsTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (exactArgumentsTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead}
}
func (exactArgumentsTool) IsReadOnly(wire.ToolCall) bool { return true }

func TestExecutedArgumentsProjectionUsesSameDurableAuthority(t *testing.T) {
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(ctx, "owner", "actual", "")
	if err != nil {
		t.Fatal(err)
	}
	backend, err := repo.BoundReferences(ctx, "owner", "actual")
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(map[string]string{"selected_text": strings.Repeat("original argument 日志🙂\n", 2000)})
	calls, round := 0, 0
	var sourceURI string
	model := modelFunc(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		round++
		if round == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "exact_arguments", Arguments: raw}}); err != nil {
				return err
			}
		} else {
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolCall == nil || block.ToolCall.ID != "call" {
						continue
					}
					if block.ToolCall.Name != "exact_arguments" || len(block.ToolCall.Arguments) > 4096 {
						t.Fatal("historical call identity/size changed")
					}
					var retained struct {
						Value wire.Reference `json:"alva_retained_arguments"`
					}
					if err := json.Unmarshal(block.ToolCall.Arguments, &retained); err != nil || retained.Value.URI == "" {
						t.Fatal("missing retained arguments", err)
					}
					sourceURI = retained.Value.URI
				}
			}
			if len(request.Messages) < 3 || request.Messages[len(request.Messages)-1].Content[0].ToolResult.CallID != "call" {
				t.Fatal("call/result pair broken")
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	plugins, err := NativeSessionPlugins(ctx, repo, "owner", "actual")
	if err != nil {
		t.Fatal(err)
	}
	builder := NewBuilder(model, log).Tool(exactArgumentsTool{expected: string(raw), calls: &calls})
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	agent, err := builder.Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "run local fixture")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if calls != 1 || round != 2 || sourceURI == "" {
		t.Fatal("real invocation not completed")
	}
	records, err := log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	for _, record := range records {
		if record.ToolIntent != nil && string(record.ToolIntent.Call.Arguments) != string(raw) {
			t.Fatal("durable original intent changed")
		}
		if record.Message != nil {
			for _, block := range record.Message.Content {
				if block.ToolCall != nil && string(block.ToolCall.Arguments) != string(raw) {
					t.Fatal("durable assistant call changed")
				}
			}
		}
	}
	var recovered strings.Builder
	for offset := int64(0); ; {
		page, err := backend.ReadText(ctx, sourceURI, offset, 4096)
		if err != nil {
			t.Fatal(err)
		}
		recovered.WriteString(page.Text)
		if !page.Truncated {
			break
		}
		offset = page.NextOffset
	}
	if recovered.String() != string(raw) {
		t.Fatal("original argument bytes cannot be retrieved")
	}
	original, err := log.Messages(ctx)
	if err != nil {
		t.Fatal(err)
	}
	state, err := abi.NewAgentState(fixtureModelForArguments{}, log, nil)
	if err != nil {
		t.Fatal(err)
	}
	policy := referenceProjection{sessionID: "actual", backend: backend}
	for index := 0; index < 300; index++ {
		request := abi.ModelRequest{Messages: wire.CloneMessages(original)}
		if err := policy.BeforeModel(ctx, abi.RuntimeContext{State: state}, &request); err != nil {
			t.Fatal("repeated prepare exhausted backend", err)
		}
		var value struct {
			Reference wire.Reference `json:"alva_retained_arguments"`
		}
		_ = json.Unmarshal(request.Messages[1].Content[0].ToolCall.Arguments, &value)
		if value.Reference.URI != sourceURI {
			t.Fatal("repeated prepare created another artifact")
		}
	}
}

type fixtureModelForArguments struct{}

func (fixtureModelForArguments) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}

func TestUnresolvedArgumentsAreNeverProjected(t *testing.T) {
	plugin := memoryContextPlugin(t, "owned")
	raw, _ := json.Marshal(map[string]string{"text": strings.Repeat("x", 5000)})
	call := wire.ToolCall{ID: "reused", Name: "fixture", Arguments: raw}
	complete := wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &call}}}
	result := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "reused"}}}}
	pending := wire.CloneMessages([]wire.Message{complete})[0]
	request := abi.ModelRequest{Messages: wire.CloneMessages([]wire.Message{complete, result, pending})}
	if err := (referenceProjection{sessionID: "owned", backend: plugin.backend}).BeforeModel(context.Background(), testRuntime(t, "owned"), &request); err != nil {
		t.Fatal(err)
	}
	if string(request.Messages[2].Content[0].ToolCall.Arguments) != string(raw) {
		t.Fatal("pending call rewritten by matching an older reused ID")
	}
	if string(request.Messages[0].Content[0].ToolCall.Arguments) == string(raw) {
		t.Fatal("completed historical call was not projected")
	}
}
