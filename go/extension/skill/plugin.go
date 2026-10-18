// INPUT: bytes/context/json/errors/io/fs/reflect/strings/utf8, Skill Repository, ReferenceBackend, core/ABI/wire
// OUTPUT: Plugin, New, Tools, Register
// POS: [L4 extension] Skill prompt/tool wrapper with reference-first progressive loading and no script execution
package skillextension

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"reflect"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Plugin struct {
	repository skill.Repository
	sessionID  string
	backend    agentcontext.ReferenceBackend
	tool       *skillTool
}

func New(repository skill.Repository, sessionID string, backend agentcontext.ReferenceBackend) *Plugin {
	p := &Plugin{repository: repository, sessionID: sessionID, backend: backend}
	p.tool = &skillTool{plugin: p}
	return p
}
func (*Plugin) Name() string        { return "skills" }
func (p *Plugin) Tools() []abi.Tool { return []abi.Tool{p.tool} }
func (p *Plugin) Register(ctx context.Context, r *core.Registrar) error {
	if missingCapability(p.repository) || p.sessionID == "" || missingCapability(p.backend) {
		return errors.New("skill plugin requires repository and fixed-session reference backend")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	list, err := p.repository.List(ctx)
	if err = errors.Join(err, ctx.Err()); err != nil {
		return err
	}
	if len(list) > 1024 {
		return errors.New("skill catalog exceeds 1024 entries")
	}
	summary, err := skill.Summary(list)
	if err != nil {
		return err
	}
	if summary != "" {
		if err = r.SystemPrompt(summary); err != nil {
			return err
		}
	}
	return r.Tool(p.tool)
}

type skillTool struct{ plugin *Plugin }

func (*skillTool) Name() string { return "skill" }
func (*skillTool) Description() string {
	return "Load exact skill instructions or an explicitly selected resource. Startup contains metadata only. Large text is stored as a session reference; retrieve the complete relevant instructions with context_read. Never executes scripts or sends binary media."
}
func (*skillTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"name":{"type":"string"},"operation":{"type":"string","enum":["load","list_resources","resource"]},"path":{"type":"string"}},"required":["name"],"additionalProperties":false}`)
}
func (t *skillTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if t.plugin.sessionID == "" || execution.SessionID != t.plugin.sessionID || execution.ActualTool != t || execution.CallID != call.ID || call.Name != t.Name() {
		return wire.ToolResult{}, errors.New("skill requires its bound session and actual dispatched tool/call")
	}
	if missingCapability(t.plugin.repository) || missingCapability(t.plugin.backend) {
		return wire.ToolResult{}, errors.New("skill repository and reference implementations required")
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	if len(call.Arguments) > 4096 || !wire.ValidJSON(call.Arguments) {
		return problem(call, "Skill arguments must be one UTF-8 JSON object within 4 KiB"), nil
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(call.Arguments, &fields) != nil || fields == nil {
		return problem(call, "Skill arguments must be an object"), nil
	}
	for key, value := range fields {
		if (key != "name" && key != "operation" && key != "path") || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return problem(call, "Unknown or null skill argument"), nil
		}
	}
	var args struct {
		Name      string `json:"name"`
		Operation string `json:"operation"`
		Path      string `json:"path"`
	}
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&args); err != nil || !skill.ValidName(args.Name) || decoder.Decode(new(any)) != io.EOF {
		return problem(call, "Invalid skill arguments"), nil
	}
	if args.Operation == "" {
		args.Operation = "load"
	}
	var value any
	switch args.Operation {
	case "load":
		if args.Path != "" {
			return problem(call, "path is only valid for resource operation"), nil
		}
		document, err := t.plugin.repository.Load(ctx, args.Name)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return wire.ToolResult{}, err
		}
		if document.Meta.Name != args.Name || !utf8.ValidString(document.Body) || len(document.Body) > skill.MaxDocumentBytes || len(document.Meta.AllowedTools) > 128 {
			return wire.ToolResult{}, errors.New("skill repository returned an invalid selected document")
		}
		for _, name := range document.Meta.AllowedTools {
			if name == "" || len(name) > 128 || !utf8.ValidString(name) || strings.ContainsAny(name, "\r\n\x00") {
				return wire.ToolResult{}, errors.New("skill repository returned invalid tool metadata")
			}
		}
		value = map[string]any{"name": args.Name, "instructions": document.Body, "allowed_tools": document.Meta.AllowedTools, "tool_declaration_grants_host_access": false, "processing": "Complete skill instruction text loaded on explicit request; no scripts executed or media processed"}
	case "list_resources":
		if args.Path != "" {
			return problem(call, "path is only valid for resource operation"), nil
		}
		paths, err := t.plugin.repository.Resources(ctx, args.Name)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return wire.ToolResult{}, err
		}
		if len(paths) > 2048 {
			return wire.ToolResult{}, errors.New("skill resource index exceeds 2048 entries")
		}
		for _, path := range paths {
			if len(path) > 4096 || !utf8.ValidString(path) || !fs.ValidPath(path) || strings.Contains(path, "\\") {
				return wire.ToolResult{}, errors.New("skill repository returned invalid resource metadata")
			}
		}
		value = map[string]any{"name": args.Name, "resources": paths, "contents_loaded": false}
	case "resource":
		if !fs.ValidPath(args.Path) || strings.Contains(args.Path, "\\") || strings.Count(args.Path, "/") > 16 {
			return problem(call, "resource requires one selected relative path within the skill"), nil
		}
		resource, err := t.plugin.repository.Resource(ctx, args.Name, args.Path)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return wire.ToolResult{}, err
		}
		if resource.Path != args.Path || len(resource.Bytes) > skill.MaxDocumentBytes || len(resource.MediaType) > 256 || !utf8.ValidString(resource.MediaType) {
			return wire.ToolResult{}, errors.New("skill repository returned an invalid selected resource")
		}
		if !utf8.Valid(resource.Bytes) {
			return problem(call, "Binary resource not delivered. Explicit media processing is required; nothing was executed or sent as media."), nil
		}
		value = map[string]any{"name": args.Name, "path": resource.Path, "media_type": resource.MediaType, "text": string(resource.Bytes), "processing": "Explicit UTF-8 resource read; script text is not execution"}
	default:
		return problem(call, "Unknown skill operation"), nil
	}
	data, err := json.Marshal(value)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) > 8<<20 {
		return wire.ToolResult{}, errors.New("encoded skill projection exceeds 8 MiB")
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) > 4096 {
		ref, err := t.plugin.backend.StoreText(ctx, string(data))
		if err = errors.Join(err, ctx.Err()); err != nil {
			return wire.ToolResult{}, err
		}
		if err := ref.Validate(); err != nil {
			return wire.ToolResult{}, err
		}
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &ref}}}, nil
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
func problem(call wire.ToolCall, text string) wire.ToolResult {
	return wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}
}
func (*skillTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: "skill-repository", Access: abi.AccessRead}}}
}
func (*skillTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Read explicitly configured skill roots, never execute or install"}
}
func (*skillTool) IsReadOnly(wire.ToolCall) bool { return true }

func missingCapability(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Map, reflect.Slice, reflect.Interface, reflect.Chan:
		return v.IsNil()
	}
	return false
}
