// INPUT: context, JSON/errors/strings/testing, utility and kernel ABI/wire.
// OUTPUT: Inline schema discovery and output-policy isolation regressions.
// POS: [L4 extension] Read-only catalog access needs no writer while configured retention never leaks inline fallback.
package utility_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/extension/utility"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type schemaTool struct {
	catalogTool
	schema json.RawMessage
}

func (t schemaTool) ParametersSchema() json.RawMessage { return t.schema }

func TestCatalogInlineAndRetainedOutputPolicies(t *testing.T) {
	failure := errors.New("configured writer unavailable")
	for _, mode := range []string{"inline", "oversize", "retained-failure"} {
		t.Run(mode, func(t *testing.T) {
			schema := json.RawMessage(`{"type":"object","description":"actual schema body"}`)
			if mode == "oversize" {
				raw, _ := json.Marshal(map[string]string{"type": "object", "description": strings.Repeat("<", 4096)})
				schema = raw
			}
			config := utility.SearchConfig{Snapshot: func() abi.ToolRegistrySnapshot {
				return abi.ToolRegistrySnapshot{Version: 7, Tools: []abi.Tool{schemaTool{schema: schema}}}
			}}
			writes := 0
			if mode == "retained-failure" {
				config.RetainSchema = func(context.Context, string, json.RawMessage) (wire.Reference, error) {
					writes++
					return wire.Reference{}, failure
				}
			}
			tool, err := utility.NewSearchTool(config)
			if err != nil {
				t.Fatal(err)
			}
			result, err := tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, CallID: "schema"}, wire.ToolCall{ID: "schema", Name: tool.Name(), Arguments: json.RawMessage(`{"action":"schema","name":"late_tool"}`)})
			if mode == "inline" {
				var body struct {
					Schema          json.RawMessage
					RegistryVersion uint64 `json:"registry_version"`
				}
				if err != nil || json.Unmarshal([]byte(result.Content[0].Text), &body) != nil || !strings.Contains(string(body.Schema), "actual schema body") || body.RegistryVersion != 7 || writes != 0 {
					t.Fatal("catalog required storage or lost schema", result, err)
				}
			} else if err == nil || len(result.Content) != 0 || mode == "retained-failure" && (!errors.Is(err, failure) || writes != 1) {
				t.Fatal("output failure silently truncated or fell back inline", result, err, writes)
			}
		})
	}
}
