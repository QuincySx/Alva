// INPUT: context, encoding/json, errors, fmt, strings, testing, time, kernel/abi, llm/wire
// OUTPUT: Task metadata, factory registry, credential secrecy and full-content estimator regressions
// POS: [L2.3 agent contracts] Confirms agent contracts have usable semantics without network or host execution.
package abi

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	kernel "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type providerFixture struct {
	id    string
	model kernel.LanguageModel
	calls int
}

func (provider *providerFixture) ID() string { return provider.id }
func (provider *providerFixture) LanguageModel(context.Context, string) (kernel.LanguageModel, error) {
	provider.calls++
	return provider.model, nil
}

type fixtureModel struct{}

type missingModelFunction func(context.Context, kernel.ModelRequest, func(wire.StreamChunk) error) error

func (fn missingModelFunction) Stream(ctx context.Context, request kernel.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

func (*fixtureModel) Stream(context.Context, kernel.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}

func TestProviderRegistryReallyReplacesFactoryAndDetachesIDs(t *testing.T) {
	registry := NewProviderRegistry()
	first := &providerFixture{id: "p", model: &fixtureModel{}}
	second := &providerFixture{id: "p", model: &fixtureModel{}}
	if err := registry.Register(first); err != nil {
		t.Fatal(err)
	}
	if err := registry.Register(second); err != nil {
		t.Fatal(err)
	}
	model, err := registry.LanguageModel(context.Background(), "p", "model")
	if err != nil || model != second.model || first.calls != 0 || second.calls != 1 {
		t.Fatal("factory not replaced", model, err)
	}
	ids := registry.IDs()
	ids[0] = "mutated"
	if registry.IDs()[0] != "p" {
		t.Fatal("registry leaked identities")
	}
	_, err = registry.LanguageModel(context.Background(), "missing", "m")
	var missing *ProviderError
	if !errors.As(err, &missing) || missing.Kind != ProviderNoSuchModel || missing.ModelID != "missing:m" {
		t.Fatal(err)
	}
	second.model = nil
	if _, err := registry.LanguageModel(context.Background(), "p", "m"); err == nil {
		t.Fatal("nil model accepted")
	}
	second.model = missingModelFunction(nil)
	if _, err := registry.LanguageModel(context.Background(), "p", "m"); err == nil {
		t.Fatal("typed-nil function model escaped provider factory")
	}
}

func TestStaticCredentialCannotLeakThroughFormattingOrJSON(t *testing.T) {
	credential, err := NewStaticCredential("fixture-super-secret")
	if err != nil {
		t.Fatal(err)
	}
	for _, text := range []string{fmt.Sprint(credential), fmt.Sprintf("%+v", credential), fmt.Sprintf("%#v", credential), fmt.Sprintf("%#v", *credential)} {
		if strings.Contains(text, "fixture-super-secret") {
			t.Fatal("credential formatting leaked")
		}
	}
	if _, err := json.Marshal(credential); err == nil {
		t.Fatal("credential serialized")
	}
	key, err := credential.GetAPIKey(context.Background())
	if err != nil || key != "fixture-super-secret" {
		t.Fatal("explicit credential capability failed")
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if key, err := credential.GetAPIKey(ctx); key != "" || !errors.Is(err, context.Canceled) {
		t.Fatal("canceled credential disclosed")
	}
	if _, err := NewStaticCredential("header\r\ninjection"); err == nil {
		t.Fatal("header injection accepted")
	}
}

func TestTaskKindsStateTimingAndOutputCursor(t *testing.T) {
	ids := make(map[string]bool)
	prefixes := make(map[byte]bool)
	for _, kind := range []TaskKind{TaskLocalBash, TaskLocalAgent, TaskRemoteAgent, TaskInProcessTeammate, TaskLocalWorkflow, TaskMonitorMCP, TaskDream} {
		prefix, _ := kind.Prefix()
		if prefixes[prefix] {
			t.Fatal("duplicate kind prefix")
		}
		prefixes[prefix] = true
		for i := 0; i < 100; i++ {
			task, err := NewTask(kind, "title", "tool-use", "output.log")
			if err != nil || ids[task.ID] || len(task.ID) < 9 || task.ID[0] != prefix || task.CreatedAt.IsZero() || task.StartedAt != nil || task.EndedAt != nil {
				t.Fatal(task, err)
			}
			ids[task.ID] = true
			if err := task.Transition(TaskRunning); err != nil {
				t.Fatal(err)
			}
			if err := task.AddPaused(time.Millisecond); err != nil {
				t.Fatal(err)
			}
			if err := task.AdvanceOutput(10); err != nil {
				t.Fatal(err)
			}
			if err := task.AdvanceOutput(9); err == nil {
				t.Fatal("output cursor rewound")
			}
			if err := task.Transition(TaskSucceeded); err != nil || task.StartedAt == nil || task.EndedAt == nil || !task.Status.Terminal() {
				t.Fatal(task, err)
			}
			if err := task.AddPaused(time.Second); err == nil {
				t.Fatal("terminal task paused")
			}
		}
	}
}

func TestEstimatorCountsToolReasoningAndReferencesNotJustVisibleText(t *testing.T) {
	estimator := SimpleTokenEstimator{}
	if estimator.EstimateText("hello世界") != 4 {
		t.Fatal("ASCII/CJK heuristic changed")
	}
	text := wire.TextMessage(wire.RoleUser, "abcd")
	base, err := estimator.EstimateMessages([]wire.Message{text})
	if err != nil || base != 5 {
		t.Fatal(base, err)
	}
	call := wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{"payload":"` + strings.Repeat("x", 100) + `"}`)}
	message := wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &call}}}
	tokens, err := estimator.EstimateMessages([]wire.Message{message})
	if err != nil || tokens <= base {
		t.Fatal("tool arguments omitted", tokens, err)
	}
	for _, block := range []wire.ContentBlock{{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: strings.Repeat("think", 100), Signature: "signed"}}, {Kind: wire.ContentReference, Reference: &wire.Reference{URI: "artifact://not-opened", Processing: strings.Repeat("metadata", 100)}}} {
		message := wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{block}}
		if estimate, err := estimator.EstimateMessages([]wire.Message{message}); err != nil || estimate <= base {
			t.Fatal("nonvisible block omitted", estimate, err)
		}
	}
	message.Content[0].ToolCall.Arguments = json.RawMessage("invalid")
	if _, err := estimator.EstimateMessages([]wire.Message{message}); err == nil {
		t.Fatal("invalid wire estimated as empty")
	}
}
