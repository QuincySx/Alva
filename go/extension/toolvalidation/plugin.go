// INPUT: bytes, context, sha256, json, errors, fmt, strconv, strings, sync, jsonschema, agent/core, kernel/abi, llm/wire
// OUTPUT: Plugin and New for replaceable tool-argument schema validation
// POS: [L4 extension] Validates invocation-bound prepared and frozen inputs without importing a schema engine into the SDK.
package toolvalidation

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"sync"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

const maxSchemaBytes = 1 << 20
const maxArgumentBytes = 4 << 20
const maxCachedSchemas = 128

// Plugin is the default harness policy and is also usable with core.Builder.
// It never fetches remote schemas, coerces arguments or inserts defaults.
type Plugin struct {
	mu      sync.Mutex
	schemas map[[32]byte]*jsonschema.Schema
}

func New() *Plugin           { return &Plugin{schemas: make(map[[32]byte]*jsonschema.Schema)} }
func (*Plugin) Name() string { return "tool-validation" }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	// Validate after ordinary preparation, then recheck the frozen execution.
	return r.Middleware(p, 10000)
}

func (p *Plugin) Finalize(ctx context.Context, late core.LateContext) ([]abi.Tool, error) {
	for _, definition := range late.ToolDefinitions {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if _, err := p.compile(definition.Parameters); err != nil {
			return nil, fmt.Errorf("tool %q schema: %w", definition.Name, err)
		}
	}
	return nil, nil
}

func (p *Plugin) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	// Includes tools returned by finalizers, which are intentionally absent
	// from the read-only Register snapshot seen by other finalizers.
	for _, tool := range runtime.State.Tools().Snapshot().Tools {
		if err := ctx.Err(); err != nil {
			return err
		}
		if _, err := p.compile(tool.ParametersSchema()); err != nil {
			return fmt.Errorf("tool %q schema: %w", tool.Name(), err)
		}
	}
	return nil
}

func (p *Plugin) PrepareTool(ctx context.Context, preparation abi.ToolPreparationContext, call wire.ToolCall) (abi.ToolPreparation, error) {
	tool := preparation.ActualTool
	if tool == nil {
		return abi.ToolPreparation{}, nil
	} // Kernel owns unknown-tool results.
	valid, err := p.validate(ctx, tool, call)
	if err != nil || valid {
		return abi.ToolPreparation{}, err
	}
	return abi.ToolPreparation{Result: &wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Tool arguments do not match the declared schema."}}}}, nil
}

func (p *Plugin) BeforeTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) error {
	if execution.ActualTool == nil {
		return nil
	}
	valid, err := p.validate(ctx, execution.ActualTool, call)
	if err != nil {
		return err
	}
	if !valid {
		return &abi.ToolBlockedError{Reason: "Tool arguments do not match the declared schema."}
	}
	return nil
}

func (p *Plugin) validate(ctx context.Context, tool abi.Tool, call wire.ToolCall) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	schema, err := p.compile(tool.ParametersSchema())
	if err != nil {
		return false, fmt.Errorf("tool %q schema: %w", call.Name, err)
	}
	var value any
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.UseNumber()
	if len(call.Arguments) > maxArgumentBytes || !wire.ValidJSON(call.Arguments) || decoder.Decode(&value) != nil || !boundedNumbers(value) {
		return false, nil
	}
	// Validation errors may quote argument values; retain only the decision.
	valid := schema.Validate(value) == nil
	if err := ctx.Err(); err != nil {
		return false, err
	}
	return valid, nil
}

func (p *Plugin) compile(raw json.RawMessage) (*jsonschema.Schema, error) {
	if len(raw) > maxSchemaBytes || !wire.ValidJSON(raw) {
		return nil, errors.New("schema must be valid JSON within 1 MiB")
	}
	key := sha256.Sum256(raw)
	p.mu.Lock()
	defer p.mu.Unlock()
	if schema := p.schemas[key]; schema != nil {
		return schema, nil
	}
	// Decode both schema and instance with UseNumber; schema bounds and enum
	// values must not round through float64 either.
	var source any
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(&source); err != nil {
		return nil, err
	}
	if !boundedNumbers(source) {
		return nil, errors.New("schema numeric work budget exceeded")
	}
	if object, ok := source.(map[string]any); ok {
		if dialect, present := object["$schema"]; present {
			switch dialect {
			case "http://json-schema.org/draft-07/schema#", "https://json-schema.org/draft-07/schema#", "https://json-schema.org/draft/2020-12/schema":
			default:
				return nil, errors.New("unsupported JSON schema dialect")
			}
		}
	}
	compiler := jsonschema.NewCompiler()
	compiler.DefaultDraft(jsonschema.Draft2020)
	compiler.UseLoader(denyLoader{})
	const resource = "https://alva.invalid/tool-schema"
	if err := compiler.AddResource(resource, source); err != nil {
		return nil, errors.New("invalid JSON schema resource")
	}
	resolved, err := compiler.Compile(resource)
	if err != nil {
		return nil, errors.New("invalid or unresolved JSON schema")
	}
	if len(p.schemas) >= maxCachedSchemas {
		clear(p.schemas)
	}
	if p.schemas == nil {
		p.schemas = make(map[[32]byte]*jsonschema.Schema)
	}
	p.schemas[key] = resolved
	return resolved, nil
}

type denyLoader struct{}

func (denyLoader) Load(string) (any, error) {
	return nil, errors.New("external schema loading is disabled")
}

// Bound rational-number work before passing untrusted instances to the schema
// engine. Large exact integers remain json.Number and never round through JS.
func boundedNumbers(value any) bool {
	switch v := value.(type) {
	case json.Number:
		if len(v) > 4096 {
			return false
		}
		if i := strings.IndexAny(string(v), "eE"); i >= 0 {
			exponent, err := strconv.Atoi(string(v)[i+1:])
			if err != nil || exponent < -4096 || exponent > 4096 {
				return false
			}
		}
	case map[string]any:
		for _, child := range v {
			if !boundedNumbers(child) {
				return false
			}
		}
	case []any:
		for _, child := range v {
			if !boundedNumbers(child) {
				return false
			}
		}
	}
	return true
}
