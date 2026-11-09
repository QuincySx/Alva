// INPUT: context, JSON/errors/strings/testing, interaction and memory SDK/reference contracts.
// OUTPUT: Inline question execution and answered-but-unsaved error regressions.
// POS: [L4 extension] No-store consumers and reference-first consumers use the same actual human interaction.
package interaction_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/extension/interaction"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestQuestionRunsWithoutArtifactStoreOrReader(t *testing.T) {
	asks := 0
	tool, err := interaction.NewTool(interaction.ToolConfig{Handler: interaction.HandlerFunc(func(_ context.Context, q interaction.Question) (interaction.Answer, error) {
		asks++
		if q.CallID != "ask" {
			t.Fatal(q)
		}
		return interaction.Answer{Selected: []int{2}}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "ask", Name: "ask_human", Arguments: json.RawMessage(`{"question":"Choose","options":[{"label":"A"},{"label":"B","value":"chosen"}]}`)}}); err != nil {
				return err
			}
		} else {
			result := request.Messages[len(request.Messages)-1].Content[0].ToolResult
			var receipt struct {
				Answer    *interaction.ResolvedAnswer `json:"answer_value"`
				Reference *wire.Reference             `json:"answer"`
			}
			if result == nil || result.IsError || json.Unmarshal([]byte(result.Content[0].Text), &receipt) != nil || receipt.Answer == nil || receipt.Answer.Text != "chosen" || !receipt.Answer.Structured || receipt.Reference != nil {
				t.Fatal("inline answer needs a reader", result)
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, core.NewMemorySession("question-inline")).Tool(tool).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 2 || asks != 1 {
		t.Fatal(turns, asks, err)
	}
}

type failingAnswers struct {
	agentcontext.ReferenceBackend
	writes int
	err    error
}

func (r *failingAnswers) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	r.writes++
	if r.writes == 2 {
		return wire.Reference{}, r.err
	}
	return r.ReferenceBackend.StoreText(ctx, text)
}

func TestReferenceFailureReportsReceivedAnswerWithoutInlineFallback(t *testing.T) {
	refs, _ := agentcontext.NewMemoryReferences("question-failure", 1<<20)
	failure := errors.New("private storage failure")
	backend := &failingAnswers{ReferenceBackend: refs, err: failure}
	asks := 0
	tool, err := interaction.NewTool(interaction.ToolConfig{Handler: interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
		asks++
		answer := "private full answer"
		return interaction.Answer{Text: &answer}, nil
	}), References: backend})
	if err != nil {
		t.Fatal(err)
	}
	result, err := tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, SessionID: "question-failure", RunID: "run", CallID: "ask"}, wire.ToolCall{ID: "ask", Name: tool.Name(), Arguments: json.RawMessage(`{"question":"actual human question"}`)})
	var received *interaction.AnswerReceivedError
	if !errors.As(err, &received) || !errors.Is(err, failure) || received.QuestionID == "" || asks != 1 || backend.writes != 2 || len(result.Content) != 0 {
		t.Fatal("answer outcome ambiguous or repeated", result, err, asks)
	}
	if strings.Contains(err.Error(), "private") || !strings.Contains(err.Error(), `"answered":true`) {
		t.Fatal("unsafe or missing recovery receipt", err)
	}
}

func TestInlineAnswerLimitDoesNotTruncateOrAskAgain(t *testing.T) {
	asks := 0
	tool, err := interaction.NewTool(interaction.ToolConfig{Handler: interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
		asks++
		answer := strings.Repeat("<", 4096)
		return interaction.Answer{Text: &answer}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	result, err := tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, SessionID: "s", RunID: "run", CallID: "ask"}, wire.ToolCall{ID: "ask", Name: tool.Name(), Arguments: json.RawMessage(`{"question":"large answer"}`)})
	var received *interaction.AnswerReceivedError
	if !errors.As(err, &received) || asks != 1 || len(result.Content) != 0 {
		t.Fatal("oversize answered question became success or repeated", result, err, asks)
	}
}

func TestOptionalReferenceBackendRejectsTypedNil(t *testing.T) {
	var refs *failingAnswers
	handler := interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
		t.Fatal("nil backend reached human")
		return interaction.Answer{}, nil
	})
	if _, err := interaction.NewTool(interaction.ToolConfig{Handler: handler, References: refs}); err == nil {
		t.Fatal("typed-nil backend silently selected inline output")
	}
}

// With reference output configured the model still gets a small answer directly,
// and a large one as a bounded prefix, the complete text staying in the reference.
func TestRetainedAnswerIsAlsoDeliveredDirectly(t *testing.T) {
	for name, answer := range map[string]string{
		"small":   "just do it",
		"large":   strings.Repeat("长", 5000), // 15000 bytes
		"escaped": strings.Repeat("<", 9000), // JSON-inflated past the receipt cap
	} {
		refs, _ := agentcontext.NewMemoryReferences("s", 1<<20)
		tool, err := interaction.NewTool(interaction.ToolConfig{References: refs, Handler: interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
			a := answer
			return interaction.Answer{Text: &a}, nil
		})})
		if err != nil {
			t.Fatal(err)
		}
		result, err := tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, SessionID: "s", RunID: "run", CallID: "ask"}, wire.ToolCall{ID: "ask", Name: tool.Name(), Arguments: json.RawMessage(`{"question":"q"}`)})
		if err != nil || result.IsError {
			t.Fatalf("%s: %+v %v", name, result, err)
		}
		var receipt struct {
			Value     *interaction.ResolvedAnswer `json:"answer_value"`
			Truncated bool                        `json:"answer_truncated"`
			Answer    *wire.Reference             `json:"answer"`
		}
		if err := json.Unmarshal([]byte(result.Content[0].Text), &receipt); err != nil || receipt.Value == nil || receipt.Answer == nil {
			t.Fatalf("%s: need inline value and reference: %v %s", name, err, result.Content[0].Text)
		}
		page, err := refs.ReadText(t.Context(), receipt.Answer.URI, 0, 4096)
		if err != nil || page.Text == "" {
			t.Fatalf("%s: reference must hold the real answer: %v", name, err)
		}
		switch name {
		case "small":
			if receipt.Value.Text != answer || receipt.Truncated {
				t.Fatalf("small answer not verbatim: %+v", receipt)
			}
		default:
			if !receipt.Truncated || !strings.HasPrefix(answer, receipt.Value.Text) || len(receipt.Value.Text) == 0 || len(receipt.Value.Text) > 8<<10 {
				t.Fatalf("%s: want bounded non-empty prefix, got %d bytes truncated=%v", name, len(receipt.Value.Text), receipt.Truncated)
			}
		}
	}
}
