// INPUT: Granted workspace host operations, explicit security Guard, references and kernel Tool contracts
// OUTPUT: FileTools with bounded model arguments, reference-first reads and backup-before-write policy
// POS: [L4 extension] Adapts native filesystem capabilities to model calls without exposing roots or owning grants.
package files

import (
	"bytes"
	"encoding/json"
	"errors"
	"strings"

	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/platform/system/httpjson"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type fileTool struct {
	kind      string
	workspace *workspacefs.Workspace
	guard     *security.Guard
	backend   agentcontext.ReferenceBackend
	sessionID string
}

func FileTools(workspace *workspacefs.Workspace, guard *security.Guard, backend agentcontext.ReferenceBackend, sessionID string) []abi.Tool {
	var out []abi.Tool
	for _, name := range []string{"read_file", "write_file", "edit_file", "list_directory", "search_files"} {
		out = append(out, fileTool{name, workspace, guard, backend, sessionID})
	}
	return out
}
func (t fileTool) Name() string { return t.kind }
func (t fileTool) Description() string {
	switch t.kind {
	case "read_file":
		return "Store a UTF-8 regular file (max 20 MiB) as a session reference; original text is not automatically fed to the model. Use context_read/search to inspect selected ranges."
	case "write_file":
		return "Atomically write UTF-8 content, retaining a session reference to prior text for recovery. Requires host write authorization."
	case "edit_file":
		return "Replace exact text in a UTF-8 file, retaining a session backup; ambiguous matches require all=true."
	case "list_directory":
		return "List up to 256 immediate regular-file/directory metadata entries without opening their contents."
	default:
		return "Literal bounded text search in workspace files; reports scanned bytes/files and truncation, never claims exhaustive results beyond budget."
	}
}
func (t fileTool) ParametersSchema() json.RawMessage {
	switch t.kind {
	case "read_file", "list_directory":
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}`)
	case "write_file":
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"],"additionalProperties":false}`)
	case "edit_file":
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"},"all":{"type":"boolean"}},"required":["path","old","new"],"additionalProperties":false}`)
	default:
		return json.RawMessage(`{"type":"object","properties":{"path":{"type":"string"},"query":{"type":"string"}},"required":["path","query"],"additionalProperties":false}`)
	}
}

type fileArgs struct {
	Path    string `json:"path"`
	Content string `json:"content"`
	Old     string `json:"old"`
	New     string `json:"new"`
	All     bool   `json:"all"`
	Query   string `json:"query"`
}

func decodeFile(call wire.ToolCall, kind string) (fileArgs, error) {
	var args fileArgs
	if call.Name != kind {
		return args, errors.New("file tool identity mismatch")
	}
	if len(call.Arguments) > 2<<20 {
		return args, errors.New("file arguments exceed 2 MiB")
	}
	if err := httpjson.ValidateObject(call.Arguments, &args); err != nil {
		return args, err
	}
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&args); err != nil {
		return args, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(call.Arguments, &fields); err != nil {
		return args, err
	}
	allowed := map[string]bool{"path": true}
	required := []string{"path"}
	switch kind {
	case "write_file":
		allowed["content"] = true
		required = append(required, "content")
	case "edit_file":
		allowed["old"] = true
		allowed["new"] = true
		allowed["all"] = true
		required = append(required, "old", "new")
	case "search_files":
		allowed["query"] = true
		required = append(required, "query")
	}
	for field := range fields {
		if !allowed[field] {
			return args, errors.New("unexpected file argument")
		}
		if bytes.Equal(bytes.TrimSpace(fields[field]), []byte("null")) {
			return args, errors.New("file arguments cannot be null")
		}
	}
	for _, field := range required {
		if _, exists := fields[field]; !exists {
			return args, errors.New("missing file argument")
		}
	}
	if args.Path == "" || len(args.Path) > 4096 || strings.ContainsRune(args.Path, 0) {
		return args, errors.New("invalid path")
	}
	return args, nil
}
func toolValue(call wire.ToolCall, value any) (wire.ToolResult, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return wire.ToolResult{}, err
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
func toolProblem(call wire.ToolCall, message string) (wire.ToolResult, error) {
	return wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: message}}}, nil
}

func (t fileTool) Scheduling(wire.ToolCall) abi.Scheduling {
	access := abi.AccessRead
	if t.kind == "write_file" || t.kind == "edit_file" {
		access = abi.AccessWrite
	}
	resource := "native-filesystem"
	if t.workspace != nil {
		resource += ":" + t.workspace.Directory()
	}
	return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: resource, Access: access}}}
}
func (t fileTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	access := abi.AccessRead
	kind := abi.EffectRead
	if !t.IsReadOnly(call) {
		access = abi.AccessWrite
		kind = abi.EffectWrite
	}
	args, err := decodeFile(call, t.kind)
	if err != nil {
		return abi.Effect{Kind: abi.EffectUnknown, Description: "Invalid native file arguments"}
	}
	return abi.Effect{Kind: kind, Description: t.Description(), Resources: []abi.ResourceClaim{{Resource: args.Path, Access: access}}}
}
func (t fileTool) IsReadOnly(wire.ToolCall) bool {
	return t.kind != "write_file" && t.kind != "edit_file"
}
