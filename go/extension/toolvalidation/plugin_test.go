// INPUT: context, json, fmt, os, filepath, strings, testing, core, kernel/abi, wire
// OUTPUT: Actual SDK validation, immutable numbers, late preparation and invocation-bound replacement regressions
// POS: [L4 validation testing] Proves invalid calls never reach Tool.Execute and failures remain model-recoverable.
package toolvalidation

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const integerSchema = `{"type":"object","properties":{"n":{"type":"integer"}},"required":["n"],"additionalProperties":false}`

type fixtureTool struct {
	schema    string
	calls     int
	arguments string
}

func (*fixtureTool) Name() string                        { return "integer_only" }
func (*fixtureTool) Description() string                 { return "Validation fixture" }
func (t *fixtureTool) ParametersSchema() json.RawMessage { return json.RawMessage(t.schema) }
func (t *fixtureTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	t.calls++
	t.arguments = string(call.Arguments)
	return wire.ToolResult{CallID: call.ID}, nil
}
func (*fixtureTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (*fixtureTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (*fixtureTool) IsReadOnly(wire.ToolCall) bool           { return true }

type fixtureModel struct {
	arguments string
	turns     int
	result    *wire.ToolResult
}

func (m *fixtureModel) Stream(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	m.turns++
	if m.turns == 1 {
		call := wire.ToolCall{ID: "call", Name: "integer_only", Arguments: json.RawMessage(m.arguments)}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
			return err
		}
	} else {
		for _, message := range request.Messages {
			for _, block := range message.Content {
				if block.ToolResult != nil {
					result := *block.ToolResult
					m.result = &result
				}
			}
		}
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func TestInvalidArgumentsNeverExecute(t *testing.T) {
	for _, args := range []string{`{"n":"private-fixture-value"}`, `{}`, `{"n":1,"extra":true}`, `{"n":1.5}`, `{"n":1e999999999}`} {
		t.Run(args, func(t *testing.T) {
			tool := &fixtureTool{schema: integerSchema}
			model := &fixtureModel{arguments: args}
			log := core.NewMemorySession("validation")
			agent, err := core.NewBuilder(model, log).Tool(tool).Plugin(New()).Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil {
				t.Fatal(err)
			}
			if tool.calls != 0 || model.turns != 2 || model.result == nil || !model.result.IsError {
				t.Fatalf("invalid call escaped: executions=%d turns=%d result=%+v", tool.calls, model.turns, model.result)
			}
			for _, block := range model.result.Content {
				if strings.Contains(block.Text, "private-fixture-value") {
					t.Fatal("validation error echoed argument value")
				}
			}
			records, err := log.Records(t.Context(), 0)
			if err != nil {
				t.Fatal(err)
			}
			found := false
			for _, record := range records {
				if record.ToolIntent != nil {
					found = true
					if !record.ToolIntent.NoSideEffect {
						t.Fatal("prepared rejection not recorded as unexecuted")
					}
				}
			}
			if !found {
				t.Fatal("missing rejected-call evidence")
			}
		})
	}
}

func TestValidArgumentsRemainExact(t *testing.T) {
	for _, args := range []string{`{"n":9007199254740993}`, `{"n":1.0}`, `{"n":1e3}`} {
		t.Run(args, func(t *testing.T) {
			tool := &fixtureTool{schema: integerSchema}
			model := &fixtureModel{arguments: args}
			agent, err := core.NewBuilder(model, core.NewMemorySession("exact")).Tool(tool).Plugin(New()).Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil {
				t.Fatal(err)
			}
			if tool.calls != 1 || tool.arguments != args {
				t.Fatalf("valid raw arguments changed: %d %q", tool.calls, tool.arguments)
			}
		})
	}
}

type laterPreparation struct{}

func (laterPreparation) Name() string { return "later-preparation" }
func (laterPreparation) PrepareTool(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
	return abi.ToolPreparation{Arguments: json.RawMessage(`{"n":"invalid-after-validation"}`)}, nil
}

func TestLaterPreparationCannotBypassFrozenValidation(t *testing.T) {
	tool := &fixtureTool{schema: integerSchema}
	model := &fixtureModel{arguments: `{"n":1}`}
	agent, err := core.NewBuilder(model, core.NewMemorySession("late")).Tool(tool).Plugin(New()).Middleware(laterPreparation{}, 20000).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if tool.calls != 0 || model.result == nil || !model.result.IsError {
		t.Fatal("late preparation bypassed validation")
	}
}

func TestInvalidSchemasFailAssembly(t *testing.T) {
	for _, schema := range []string{`{"type":"unknown"}`, `{"$ref":"https://example.invalid/never-fetch"}`, `{"$schema":"https://example.invalid/dialect"}`, `{"const":1e-999999999}`} {
		t.Run(schema, func(t *testing.T) {
			_, err := core.NewBuilder(&fixtureModel{arguments: `{}`}, core.NewMemorySession("schema")).Tool(&fixtureTool{schema: schema}).Plugin(New()).Build(t.Context())
			if err == nil {
				t.Fatal("unsupported or unbounded schema accepted")
			}
		})
	}
}

func TestNumericSchemasUseExactValues(t *testing.T) {
	for _, fixture := range []struct {
		name, schema, arguments string
		valid                   bool
	}{
		{"large-min-valid", `{"type":"object","properties":{"n":{"minimum":9007199254740993}}}`, `{"n":9007199254740993}`, true},
		{"large-min-invalid", `{"type":"object","properties":{"n":{"minimum":9007199254740993}}}`, `{"n":9007199254740992}`, false},
		{"large-enum-valid", `{"type":"object","properties":{"n":{"enum":[9007199254740993]}}}`, `{"n":9007199254740993}`, true},
		{"large-enum-invalid", `{"type":"object","properties":{"n":{"enum":[9007199254740993]}}}`, `{"n":9007199254740992}`, false},
		{"decimal-valid", `{"type":"object","properties":{"n":{"const":0.1}}}`, `{"n":0.1}`, true},
		{"decimal-invalid", `{"type":"object","properties":{"n":{"const":0.1}}}`, `{"n":0.10000000000000001}`, false},
		{"multiple-valid", `{"type":"object","properties":{"n":{"multipleOf":0.1}}}`, `{"n":0.3}`, true},
		{"multiple-invalid", `{"type":"object","properties":{"n":{"multipleOf":0.1}}}`, `{"n":0.31}`, false},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			valid, err := New().validate(t.Context(), &fixtureTool{schema: fixture.schema}, wire.ToolCall{Arguments: json.RawMessage(fixture.arguments)})
			if err != nil || valid != fixture.valid {
				t.Fatalf("numeric decision changed: valid=%v want=%v error=%v", valid, fixture.valid, err)
			}
		})
	}
}

func TestSchemaLoaderCannotReadLocalFiles(t *testing.T) {
	path := filepath.Join(t.TempDir(), "must-not-load.json")
	if err := os.WriteFile(path, []byte(`{"type":"object"}`), 0600); err != nil {
		t.Fatal(err)
	}
	raw, err := json.Marshal(map[string]string{"$ref": "file://" + path})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = New().compile(raw); err == nil {
		t.Fatal("local file schema loaded without authorization")
	}
}

type replaceTool struct{ replacement *fixtureTool }

func (replaceTool) Name() string { return "replace-tool" }
func (r replaceTool) BeforeModel(_ context.Context, runtime abi.RuntimeContext, _ *abi.ModelRequest) error {
	_, err := runtime.State.Tools().Replace(r.replacement)
	return err
}

const stringSchema = `{"type":"object","properties":{"n":{"type":"string"}},"required":["n"]}`

type replacementModel struct {
	arguments []string
	schemas   []string
	results   map[string]wire.ToolResult
}

func (m *replacementModel) Stream(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	for _, definition := range request.Tools {
		if definition.Name == "integer_only" {
			m.schemas = append(m.schemas, string(definition.Parameters))
		}
	}
	for _, message := range request.Messages {
		for _, block := range message.Content {
			if block.ToolResult != nil {
				m.results[block.ToolResult.CallID] = *block.ToolResult
			}
		}
	}
	if turn := len(m.schemas); turn <= len(m.arguments) {
		call := wire.ToolCall{ID: fmt.Sprintf("call-%d", turn), Name: "integer_only", Arguments: json.RawMessage(m.arguments[turn-1])}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
			return err
		}
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func TestReplacementUsesInvocationSchemaUntilNextModelBoundary(t *testing.T) {
	for _, test := range []struct {
		name          string
		firstArgument string
		originalCalls int
		firstRejected bool
	}{
		{"original schema remains executable", `{"n":1}`, 1, false},
		{"replacement schema is rejected until next boundary", `{"n":"new-schema"}`, 0, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			original := &fixtureTool{schema: integerSchema}
			replacement := &fixtureTool{schema: stringSchema}
			model := &replacementModel{arguments: []string{test.firstArgument, `{"n":"new-schema"}`}, results: make(map[string]wire.ToolResult)}
			log := core.NewMemorySession("replacement")
			agent, err := core.NewBuilder(model, log).Tool(original).Plugin(New()).Middleware(replaceTool{replacement}).Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil {
				t.Fatal(err)
			}
			if original.calls != test.originalCalls || replacement.calls != 1 {
				t.Fatalf("validation read live replacement instead of invocation: original=%d replacement=%d", original.calls, replacement.calls)
			}
			if len(model.schemas) != 3 || model.schemas[0] != integerSchema || model.schemas[1] != stringSchema || model.schemas[2] != stringSchema {
				t.Fatalf("schema replacement crossed the model boundary: %v", model.schemas)
			}
			for index, rejected := range []bool{test.firstRejected, false} {
				callID := fmt.Sprintf("call-%d", index+1)
				if result, ok := model.results[callID]; !ok || result.IsError != rejected {
					t.Fatalf("wrong effective schema validation for %s: %+v", callID, result)
				}
			}
			records, err := log.Records(t.Context(), 0)
			if err != nil {
				t.Fatal(err)
			}
			var intents []*abi.ToolIntent
			for _, record := range records {
				if record.ToolIntent != nil {
					intents = append(intents, record.ToolIntent)
				}
			}
			if len(intents) != 2 || intents[0].NoSideEffect != test.firstRejected || intents[1].NoSideEffect {
				t.Fatal("prepared rejection did not follow the advertised invocation schema", intents)
			}
		})
	}
}

func TestPreparationUnknownBindingDoesNotConsultLiveRegistry(t *testing.T) {
	tool := &fixtureTool{schema: integerSchema}
	state, err := abi.NewAgentState(&fixtureModel{}, core.NewMemorySession("unbound"), []abi.Tool{tool})
	if err != nil {
		t.Fatal(err)
	}
	prepared, err := New().PrepareTool(t.Context(), abi.ToolPreparationContext{Runtime: abi.RuntimeContext{State: state}}, wire.ToolCall{ID: "unbound", Name: tool.Name(), Arguments: json.RawMessage(`{"n":"invalid-for-live-tool"}`)})
	if err != nil || prepared.Result != nil || prepared.Arguments != nil {
		t.Fatal("unknown/disallowed invocation borrowed a live registry schema", prepared, err)
	}
}
