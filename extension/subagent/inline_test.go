// INPUT: context, encoding/json, errors, strings, testing, agent/core, kernel/abi and llm/wire.
// OUTPUT: Small child answers delivered inline, large ones truncated with a reference, child failures as results.
// POS: [L4 extension] Proves the parent model reads the child's answer without a second reference read.
package subagent

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type sayModel struct {
	text string
	fail error
}

func (m sayModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: m.text}); err != nil {
		return err
	}
	if m.fail != nil {
		return m.fail
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func childSaying(p *Plugin, model sayModel) {
	p.config.Factory = ChildFactoryFunc(func(ctx context.Context, request ChildRequest) (Child, error) {
		log := core.NewMemorySession(request.Scope.SessionID())
		agent, err := core.NewBuilder(model, log).Build(ctx)
		return Child{Runner: agent, Session: log}, err
	})
}

func TestChildAnswerIsDeliveredInlineWhenSmallAndTruncatedWhenLarge(t *testing.T) {
	small := "答案：42"
	large := strings.Repeat("é", 6000) // 12000 bytes, odd cut points
	for name, text := range map[string]string{"small": small, "large": large} {
		p, _, execution := setup(t, nil, nil)
		childSaying(p, sayModel{text: text})
		result, err := execute(t, p, execution, `{"task":"work"}`)
		if err != nil || result.IsError || len(result.Content) != 1 {
			t.Fatalf("%s: %+v %v", name, result, err)
		}
		var meta struct {
			Selected  *wire.Reference `json:"selected_output"`
			Text      string          `json:"selected_text"`
			Truncated bool            `json:"selected_text_truncated"`
		}
		if err := json.Unmarshal([]byte(result.Content[0].Text), &meta); err != nil || meta.Selected == nil {
			t.Fatalf("%s: one JSON receipt with the reference kept as evidence: %v", name, err)
		}
		if name == "small" && (meta.Text != small || meta.Truncated) {
			t.Fatalf("small answer not delivered verbatim: %q", meta.Text)
		}
		if name == "large" {
			if !meta.Truncated || len(meta.Text) > 8<<10 || !strings.HasPrefix(meta.Text, "éé") || strings.ContainsRune(meta.Text, '\uFFFD') {
				t.Fatalf("large answer must be a bounded rune-safe prefix: len=%d truncated=%v", len(meta.Text), meta.Truncated)
			}
		}
	}
}

func TestChildRunFailureIsAResultButHostFaultsAndBrokenObservationStillAbort(t *testing.T) {
	p, _, execution := setup(t, nil, nil)
	childSaying(p, sayModel{text: "partial", fail: errors.New("provider exploded")})
	result, err := execute(t, p, execution, `{"task":"work"}`)
	if err != nil || !result.IsError {
		t.Fatalf("child run failure must be a model-visible result: %+v %v", result, err)
	}
	var meta map[string]any
	if json.Unmarshal([]byte(result.Content[0].Text), &meta) != nil || meta["failure"] == "" || meta["failure"] == nil {
		t.Fatalf("parent model must learn that it failed: %s", result.Content[0].Text)
	}
	// Raw error text can carry secrets; only the classified note is shown, and
	// uncommitted output is labelled partial, never a selected answer.
	if strings.Contains(result.Content[0].Text, "provider exploded") || meta["selected_text"] != nil || meta["partial_text"] != "partial" || meta["selected_text_is_partial"] != true {
		t.Fatalf("failure text leaked or partial output mislabelled: %s", result.Content[0].Text)
	}
	p, _, execution = setup(t, func(context.Context, []wire.Message, func(abi.Event) error) error { return nil }, func(context.Context) error { return errors.New("cleanup broke") })
	if _, err = execute(t, p, execution, `{"task":"work"}`); err == nil {
		t.Fatal("cleanup failure must abort the parent run")
	}
}

// JSON escaping inflates '<' six-fold. Neither the failure note nor an inline
// answer may push the receipt over its cap and turn a recoverable failure into a
// run abort.
func TestEscapedChildTextNeverAbortsTheParent(t *testing.T) {
	p, _, execution := setup(t, nil, nil)
	childSaying(p, sayModel{text: "partial draft", fail: errors.New(strings.Repeat("<", 600) + "token=SECRET")})
	result, err := execute(t, p, execution, `{"task":"work"}`)
	if err != nil || !result.IsError || strings.Contains(result.Content[0].Text, "SECRET") {
		t.Fatalf("escaped failure text: err=%v isErr=%v", err, result.IsError)
	}
	p, _, execution = setup(t, nil, nil)
	childSaying(p, sayModel{text: strings.Repeat("<", 9000)})
	result, err = execute(t, p, execution, `{"task":"work"}`)
	if err != nil || result.IsError {
		t.Fatalf("escaped answer: err=%v", err)
	}
	var meta struct {
		Text      string `json:"selected_text"`
		Truncated bool   `json:"selected_text_truncated"`
	}
	if json.Unmarshal([]byte(result.Content[0].Text), &meta) != nil || !meta.Truncated || meta.Text == "" || len(result.Content[0].Text) > 16<<10 {
		t.Fatalf("escaped answer must be a bounded prefix: len=%d truncated=%v", len(result.Content[0].Text), meta.Truncated)
	}
}
