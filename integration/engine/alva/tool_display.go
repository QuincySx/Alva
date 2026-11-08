// INPUT: encoding/json, engine ToolDisplay helpers, shared record redaction and llm/wire tool calls/results
// OUTPUT: Alva built-in tool names mapped to engine.ToolDisplay values from redacted arguments and complete results; field redaction then bounding; shell receipt status
// POS: [L7 Alva product adapter] The Alva engine's own tool presentation knowledge; unknown tools are ToolOther; display text is redacted before it is clipped
package alva

import (
	"encoding/json"
	"strings"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// toolKinds names the Alva built-in tools the adapter classifies.
var toolKinds = map[string]engine.ToolKind{
	"execute_shell": engine.ToolCommand,
	"read_file":     engine.ToolRead,
	"write_file":    engine.ToolEdit,
	"edit_file":     engine.ToolEdit,
	"search_files":  engine.ToolSearch,
	"agent":         engine.ToolSubagent,
	"todo_write":    engine.ToolTodo,
}

type jsonFields map[string]json.RawMessage

func fieldsOf(raw []byte) jsonFields {
	var fields jsonFields
	trimmed := strings.TrimSpace(string(raw))
	if !strings.HasPrefix(trimmed, "{") || json.Unmarshal([]byte(trimmed), &fields) != nil {
		return nil
	}
	return fields
}

func (f jsonFields) text(key string) string {
	var value string
	_ = json.Unmarshal(f[key], &value)
	return value
}

func (f jsonFields) has(key string) bool { _, ok := f[key]; return ok }

// finishDisplay applies the record redaction rule to every display string and
// only then bounds it, so no clipped prefix of a secret survives.
func finishDisplay(raw engine.ToolDisplay, redactor *redaction.Redactor) *engine.ToolDisplay {
	return engine.BoundToolDisplay(engine.MapToolDisplayStrings(raw, redactor.Text))
}

// startDisplay derives a call's unbounded display from its (redacted) arguments.
func startDisplay(name string, arguments json.RawMessage) engine.ToolDisplay {
	kind, known := toolKinds[name]
	if !known {
		return engine.ToolDisplay{Kind: engine.ToolOther}
	}
	args := fieldsOf(arguments)
	display := engine.ToolDisplay{Kind: kind}
	switch kind {
	case engine.ToolCommand:
		display.Command = args.text("command")
	case engine.ToolRead:
		display.Path = args.text("path")
	case engine.ToolEdit:
		display.Path = args.text("path")
		if display.Path != "" {
			display.Files = 1
		}
		if args.has("old") {
			old, updated := args.text("old"), args.text("new")
			added, removed := engine.LineChanges(old, updated)
			display.Added, display.Removed = &added, &removed
			display.Diff = &engine.ToolDiff{Old: old, New: updated, Partial: true}
		} else if args.has("content") {
			content := args.text("content")
			added := engine.LineCount(content)
			display.Added = &added
			display.Diff = &engine.ToolDiff{New: content}
		}
	case engine.ToolSearch:
		display.Query, display.Path = args.text("query"), args.text("path")
	case engine.ToolSubagent:
		display.Task = args.text("task")
	case engine.ToolTodo:
		content := args.text("content")
		if display.Todos = engine.Checklist(content); len(display.Todos) == 0 {
			display.Plan = content
		}
	}
	return display
}

// endDisplay adds the fields a complete (redacted) result carries to the
// start display and returns the unbounded value.
func endDisplay(start *engine.ToolDisplay, result wire.ToolResult) engine.ToolDisplay {
	display := engine.ToolDisplay{Kind: engine.ToolOther}
	if start != nil {
		display = *engine.CloneToolDisplay(start)
	}
	var reference *wire.Reference
	var text []string
	for _, block := range result.Content {
		switch {
		case block.Kind == wire.ContentText:
			text = append(text, block.Text)
		case block.Kind == wire.ContentReference && block.Reference != nil && reference == nil:
			reference = block.Reference
		}
	}
	fields := fieldsOf([]byte(strings.Join(text, "\n")))
	switch display.Kind {
	case engine.ToolCommand:
		// execute_shell returns a job receipt, not output: the captured frames
		// live in native records only. Keep its status and exit code.
		var status struct {
			ExitCode *int `json:"exit_code"`
			Task     struct {
				Status string `json:"status"`
			} `json:"task"`
		}
		if fields.has("job_id") && json.Unmarshal(fields["status"], &status) == nil {
			display.OutputInTrace = true
			display.Status = strings.ToLower(status.Task.Status)
			if status.ExitCode != nil {
				display.ExitCode = status.ExitCode
			}
		}
	case engine.ToolRead:
		if reference != nil {
			display.Output = reference.URI
		}
	case engine.ToolSearch:
		var matches []struct {
			Path string `json:"path"`
			Text string `json:"text"`
		}
		if fields.has("matches") && json.Unmarshal(fields["matches"], &matches) == nil {
			found := len(matches)
			display.Matches = &found
			var truncated bool
			_ = json.Unmarshal(fields["truncated"], &truncated)
			display.MoreMatches = truncated
			display.Results = nil
			for _, match := range matches[:min(len(matches), engine.ToolDisplayResults)] {
				display.Results = append(display.Results, engine.ToolMatch{Path: match.Path, Text: match.Text})
			}
		}
	case engine.ToolSubagent:
		display.ChildSessionID, display.Status = fields.text("child_session_id"), fields.text("outcome")
		var output struct {
			URI string `json:"uri"`
		}
		if json.Unmarshal(fields["selected_output"], &output) == nil {
			display.Output = output.URI
		}
	}
	return display
}
