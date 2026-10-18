// INPUT: encoding/json
// OUTPUT: ToolDefinition
// POS: [L0 foundation] Portable L0 model-facing tool schema value, independent of runtime Tool execution or kernel contracts.
package wire

import "encoding/json"

// ToolDefinition describes only the schema advertised to a model. It contains
// no execution authority, scheduling, filesystem access or plugin instance.
type ToolDefinition struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	Parameters  json.RawMessage `json:"parameters"`
}
