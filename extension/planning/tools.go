// INPUT: bytes, context, encoding/json, errors/io, agent Task, ABI/wire and native artifacts.
// OUTPUT: Real typed task creation/read/update/dependencies/cancel/output and durable plan-note Tools.
// POS: [L4 extension] Actual manual workflow state and references are returned without pretending background execution or permission changes.
package planning

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"

	"github.com/QuincySx/Alva/platform/storage/session"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type planningTool struct {
	plugin *Plugin
	name   string
}

func (t *planningTool) Name() string { return t.name }
func (t *planningTool) Description() string {
	return map[string]string{"task_create": "Create a manual workflow task through the configured Service; no process/agent is started and no output path invented.", "task_get": "Get real same-session task metadata and full retained task detail reference.", "task_list": "Page real same-session manual tasks, with exact status filtering and revision.", "task_update": "Update title/description/owner/metadata/dependencies/status or append a note reference through the configured Service. Strict transitions/dependency DAG; not execution.", "task_stop": "Cancel a manual workflow record, not an operating-system process or agent. Use the actual execution owner's stop API for those.", "task_output": "Return bounded references to explicitly appended workflow output notes, not synthetic process output.", "todo_write": "Replace configured session plan notes. Does not edit CLAUDE.md or arbitrary files, and does not change actual permission mode."}[t.name]
}
func (*planningTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (t *planningTool) IsReadOnly(wire.ToolCall) bool {
	return t.name == "task_get" || t.name == "task_list" || t.name == "task_output"
}
func (t *planningTool) DescribeEffect(wire.ToolCall) abi.Effect {
	kind := abi.EffectWrite
	if t.IsReadOnly(wire.ToolCall{}) {
		kind = abi.EffectRead
	}
	return abi.Effect{Kind: kind, Description: "Read or change durable session-owned workflow bookkeeping only; not permission policy or process execution"}
}
func (t *planningTool) ParametersSchema() json.RawMessage {
	properties := map[string]any{}
	str := func(key string, n int) { properties[key] = map[string]any{"type": "string", "maxLength": n} }
	var required []string
	switch t.name {
	case "task_create":
		str("subject", 512)
		str("description", 4096)
		str("owner", 128)
		required = []string{"subject"}
	case "task_get", "task_stop":
		str("task_id", 64)
		required = []string{"task_id"}
	case "task_output":
		str("task_id", 64)
		properties["after"] = map[string]any{"type": "integer", "minimum": 0}
		properties["limit"] = map[string]any{"type": "integer", "minimum": 1, "maximum": 8}
		required = []string{"task_id"}
	case "task_list":
		str("status", 32)
		properties["after"] = map[string]any{"type": "integer", "minimum": 0}
		properties["limit"] = map[string]any{"type": "integer", "minimum": 1, "maximum": 20}
	case "task_update":
		str("task_id", 64)
		str("subject", 512)
		str("description", 4096)
		str("owner", 128)
		str("status", 32)
		str("append_output", 64<<10)
		required = []string{"task_id"}
	case "todo_write":
		str("content", 64<<10)
		required = []string{"content"}
	}
	if t.name == "task_create" || t.name == "task_update" {
		properties["metadata"] = map[string]any{"type": "object"}
		properties["blocked_by"] = map[string]any{"type": "array", "maxItems": 20, "items": map[string]any{"type": "string", "maxLength": 64}}
	}
	value := map[string]any{"type": "object", "additionalProperties": false, "properties": properties}
	if len(required) > 0 {
		value["required"] = required
	}
	encoded, _ := json.Marshal(value)
	return encoded
}

type arguments struct {
	Subject, Description, Owner, Content, AppendOutput *string
	ID                                                 string `json:"task_id"`
	Status                                             agentabi.TaskStatus
	Metadata                                           json.RawMessage
	BlockedBy                                          []string `json:"blocked_by"`
	After, Limit                                       int
}

func (t *planningTool) decode(raw json.RawMessage) (arguments, error) {
	var a arguments
	if !bounded(string(raw), 128<<10) {
		return a, errors.New("planning input exceeds 128 KiB UTF-8")
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return a, errors.New("planning input must be an object")
	}
	var schema struct {
		Properties map[string]any
		Required   []string
	}
	json.Unmarshal(t.ParametersSchema(), &schema)
	for key, value := range fields {
		if _, ok := schema.Properties[key]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return a, errors.New("unknown/null planning field")
		}
	}
	for _, key := range schema.Required {
		if _, ok := fields[key]; !ok {
			return a, errors.New("required planning field missing")
		}
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err := d.Decode(&a); err != nil {
		return a, err
	}
	if d.Decode(new(any)) != io.EOF {
		return a, errors.New("one planning input object required")
	}
	if a.Status != "" && a.Status != agentabi.TaskPending && a.Status != agentabi.TaskRunning && !a.Status.Terminal() {
		return a, errors.New("unknown actual task status")
	}
	if len(a.Metadata) > 0 {
		var object map[string]any
		if json.Unmarshal(a.Metadata, &object) != nil || object == nil {
			return a, errors.New("task metadata must be object")
		}
	}
	if a.Limit == 0 {
		a.Limit = 10
		if t.name == "task_output" {
			a.Limit = 4
		}
	}
	return a, nil
}
func (t *planningTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	if e.ActualTool != t || e.CallID != call.ID || call.Name != t.name {
		return zero, errors.New("planning requires actual dispatched tool/call identity")
	}
	a, err := t.decode(call.Arguments)
	if err != nil {
		return zero, err
	}
	p := t.plugin
	if err = p.check(e); err != nil {
		return zero, err
	}
	if err = p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return zero, err
	}
	if err = p.check(e); err != nil {
		return zero, err
	}
	value := func(s *string) string {
		if s == nil {
			return ""
		}
		return *s
	}
	var body any
	switch t.name {
	case "task_create":
		task, err := p.service.Create(ctx, CreateRequest{Title: value(a.Subject), Description: value(a.Description), Owner: value(a.Owner), Metadata: a.Metadata, BlockedBy: a.BlockedBy})
		if err != nil {
			return zero, err
		}
		body, err = p.detail(ctx, task)
		if err != nil {
			return zero, err
		}
	case "task_update", "task_stop":
		status := a.Status
		if t.name == "task_stop" {
			status = agentabi.TaskCancelled
		}
		task, err := p.service.Update(ctx, UpdateRequest{ID: a.ID, Title: a.Subject, Description: a.Description, Owner: a.Owner, Status: status, Metadata: a.Metadata, BlockedBy: a.BlockedBy, AppendOutput: a.AppendOutput})
		if err != nil {
			return zero, err
		}
		body, err = p.detail(ctx, task)
		if err != nil {
			return zero, err
		}
	case "task_get", "task_output":
		task, found, err := p.service.Get(ctx, a.ID)
		if err != nil {
			return zero, err
		}
		if !found {
			return result(call.ID, map[string]any{"found": false, "task_id": a.ID}, true)
		}
		if t.name == "task_get" {
			body, err = p.detail(ctx, task)
			if err != nil {
				return zero, err
			}
		} else {
			if a.After < 0 || a.After > len(task.Output) || a.Limit < 1 || a.Limit > 8 {
				return zero, errors.New("bounded task output cursor/limit required")
			}
			end := min(len(task.Output), a.After+a.Limit)
			body = map[string]any{"task_id": a.ID, "references": task.Output[a.After:end], "next": end, "has_more": end < len(task.Output), "notice": "Only explicitly appended manual workflow notes, not process execution logs"}
		}
	case "task_list":
		page, err := p.service.List(ctx, a.Status, a.After, a.Limit)
		if err != nil {
			return zero, err
		}
		items := []any{}
		for _, r := range page.Tasks {
			items = append(items, map[string]any{"id": r.Task.ID, "kind": r.Task.Kind, "status": r.Task.Status})
		}
		notes, err := p.service.Notes(ctx)
		if err != nil {
			return zero, err
		}
		body = map[string]any{"tasks": items, "next": page.Next, "has_more": page.HasMore, "revision": page.Revision, "plan_notes": notes}
	case "todo_write":
		ref, err := p.service.SetNotes(ctx, value(a.Content))
		if err != nil {
			return zero, err
		}
		if err := ref.Validate(); err != nil {
			return zero, err
		}
		body = map[string]any{"notes": ref, "notice": "Configured session plan notes replaced; no workspace file or permission policy changed"}
	}
	return result(call.ID, body, false)
}
func result(id string, body any, isError bool) (wire.ToolResult, error) {
	raw, err := json.Marshal(body)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(raw) > 16<<10 {
		return wire.ToolResult{}, errors.New("planning receipt metadata exceeds 16 KiB; page fewer references")
	}
	return wire.ToolResult{CallID: id, IsError: isError, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(raw)}}}, nil
}
func (p *Plugin) detail(ctx context.Context, task TaskRecord) (any, error) {
	raw, err := json.Marshal(task)
	if err != nil {
		return nil, err
	}
	if len(raw) > 128<<10 {
		return nil, errors.New("configured planning detail exceeds 128 KiB")
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: "planning-task-detail", Name: task.Task.ID, MediaType: "application/json", Processing: "Configured planning Service task projection at this read, retained in this native session; backend owns business-state persistence; no execution, permission change or automatic model delivery implied", Data: raw, Text: true})
	if err != nil {
		return nil, &abi.SessionError{Operation: "retain planning detail", SessionID: p.config.SessionID, Err: err}
	}
	return map[string]any{"task_id": task.Task.ID, "status": task.Task.Status, "kind": task.Task.Kind, "detail": info.Reference()}, nil
}
