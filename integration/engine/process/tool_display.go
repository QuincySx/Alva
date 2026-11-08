// INPUT: encoding/json, errors, strings, engine ToolDisplay helpers and shared JSON redaction
// OUTPUT: Codex item types (app-server and exec) and Claude/Pi tool names mapped to unbounded engine.ToolDisplay values; finishDisplay (redact every string, then bound) and tool Input redaction
// POS: [integration / native external-engine adapter] Each vendor's own tool presentation knowledge from what its events already carry; never result bodies; every transport finishes displays before emitting
package process

import (
	"encoding/json"
	"strings"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
)

type displayFields map[string]json.RawMessage

func displayFieldsOf(raw []byte) displayFields {
	var fields displayFields
	trimmed := strings.TrimSpace(string(raw))
	if !strings.HasPrefix(trimmed, "{") || json.Unmarshal([]byte(trimmed), &fields) != nil {
		return nil
	}
	return fields
}

func (f displayFields) text(keys ...string) string {
	for _, key := range keys {
		var value string
		if json.Unmarshal(f[key], &value) == nil && value != "" {
			return value
		}
	}
	return ""
}

// command reads a string command or an argv array.
func (f displayFields) command(key string) string {
	if value := f.text(key); value != "" {
		return value
	}
	var argv []string
	if json.Unmarshal(f[key], &argv) == nil {
		return strings.Join(argv, " ")
	}
	return ""
}

func (f displayFields) integer(keys ...string) *int {
	for _, key := range keys {
		var value *int
		if json.Unmarshal(f[key], &value) == nil && value != nil {
			return value
		}
	}
	return nil
}

func (f displayFields) objects(key string) []displayFields {
	var raws []json.RawMessage
	if json.Unmarshal(f[key], &raws) != nil {
		return nil
	}
	var values []displayFields
	for _, raw := range raws {
		if value := displayFieldsOf(raw); value != nil {
			values = append(values, value)
		}
	}
	return values
}

func other() *engine.ToolDisplay { return &engine.ToolDisplay{Kind: engine.ToolOther} }

// replaceDisplay is an old/new text replacement in a file.
func replaceDisplay(path, old, updated string) *engine.ToolDisplay {
	added, removed := engine.LineChanges(old, updated)
	display := engine.ToolDisplay{Kind: engine.ToolEdit, Path: path, Added: &added, Removed: &removed, Diff: &engine.ToolDiff{Old: old, New: updated, Partial: true}}
	if path != "" {
		display.Files = 1
	}
	return &display
}

// writeDisplay is a whole-file write whose previous content is unknown.
func writeDisplay(path, content string) *engine.ToolDisplay {
	added := engine.LineCount(content)
	display := engine.ToolDisplay{Kind: engine.ToolEdit, Path: path, Added: &added, Diff: &engine.ToolDiff{New: content}}
	if path != "" {
		display.Files = 1
	}
	return &display
}

// codexItemDisplay maps a Codex app-server item (started or completed).
func codexItemDisplay(itemType string, raw json.RawMessage) *engine.ToolDisplay {
	item := displayFieldsOf(raw)
	switch itemType {
	case "commandExecution":
		return &engine.ToolDisplay{Kind: engine.ToolCommand, Command: item.command("command"), ExitCode: item.integer("exitCode")}
	case "fileChange":
		changes := item.objects("changes")
		display := engine.ToolDisplay{Kind: engine.ToolEdit, Files: len(changes)}
		added, removed := 0, 0
		for i, change := range changes {
			diff := change.text("diff")
			kind := displayFieldsOf(change["kind"]).text("type")
			if kind == "" {
				kind = change.text("kind")
			}
			var old, updated string
			switch kind {
			case "add":
				updated = diff
				added += engine.LineCount(diff)
			case "delete":
				old = diff
				removed += engine.LineCount(diff)
			default:
				var plus, minus int
				old, updated, plus, minus = engine.UnifiedDiff(diff)
				added, removed = added+plus, removed+minus
			}
			if i == 0 {
				display.Path = change.text("path")
				display.Diff = &engine.ToolDiff{Old: old, New: updated, Partial: kind != "add" && kind != "delete"}
			}
		}
		if len(changes) > 0 {
			display.Added, display.Removed = &added, &removed
		}
		return &display
	case "webSearch":
		return &engine.ToolDisplay{Kind: engine.ToolSearch, Query: item.text("query")}
	case "imageView":
		return &engine.ToolDisplay{Kind: engine.ToolRead, Path: item.text("path")}
	case "collabToolCall":
		display := engine.ToolDisplay{Kind: engine.ToolSubagent, Task: item.text("prompt")}
		if status := item.text("status"); status == "completed" || status == "failed" {
			display.Status = status
		}
		return &display
	}
	return other()
}

// codexExecDisplay maps a Codex exec JSONL item (snake_case fields).
func codexExecDisplay(itemKind string, item map[string]json.RawMessage) *engine.ToolDisplay {
	fields := displayFields(item)
	switch itemKind {
	case "command_execution":
		return &engine.ToolDisplay{Kind: engine.ToolCommand, Command: fields.command("command"), ExitCode: fields.integer("exit_code")}
	case "file_change":
		changes := fields.objects("changes")
		display := engine.ToolDisplay{Kind: engine.ToolEdit, Files: len(changes)}
		if len(changes) > 0 {
			display.Path = changes[0].text("path")
		}
		return &display
	}
	return other()
}

// claudeDisplay maps a Claude Code tool_use from its input; the result body
// is never stored, so the end display repeats it with an outcome.
func claudeDisplay(name string, raw json.RawMessage) *engine.ToolDisplay {
	input := displayFieldsOf(raw)
	switch name {
	case "Bash":
		return &engine.ToolDisplay{Kind: engine.ToolCommand, Command: input.text("command")}
	case "Read":
		return &engine.ToolDisplay{Kind: engine.ToolRead, Path: input.text("file_path")}
	case "Write":
		return writeDisplay(input.text("file_path"), input.text("content"))
	case "Edit":
		return replaceDisplay(input.text("file_path"), input.text("old_string"), input.text("new_string"))
	case "MultiEdit":
		var old, updated []string
		added, removed := 0, 0
		for _, edit := range input.objects("edits") {
			a, b := edit.text("old_string"), edit.text("new_string")
			plus, minus := engine.LineChanges(a, b)
			added, removed = added+plus, removed+minus
			old, updated = append(old, a), append(updated, b)
		}
		display := replaceDisplay(input.text("file_path"), strings.Join(old, "\n"), strings.Join(updated, "\n"))
		display.Added, display.Removed = &added, &removed
		return display
	case "Grep", "Glob":
		return &engine.ToolDisplay{Kind: engine.ToolSearch, Query: input.text("pattern"), Path: input.text("path")}
	case "WebSearch":
		return &engine.ToolDisplay{Kind: engine.ToolSearch, Query: input.text("query")}
	case "Task", "Agent":
		return &engine.ToolDisplay{Kind: engine.ToolSubagent, Task: input.text("description", "prompt")}
	case "TodoWrite":
		display := engine.ToolDisplay{Kind: engine.ToolTodo}
		for _, todo := range input.objects("todos") {
			status := engine.TodoPending
			switch todo.text("status") {
			case "completed":
				status = engine.TodoCompleted
			case "in_progress":
				status = engine.TodoInProgress
			}
			display.Todos = append(display.Todos, engine.TodoItem{Text: todo.text("content"), Status: status})
		}
		return &display
	}
	return other()
}

// piDisplay maps a Pi tool_execution_start from its arguments.
func piDisplay(name string, raw json.RawMessage) *engine.ToolDisplay {
	args := displayFieldsOf(raw)
	switch name {
	case "bash":
		return &engine.ToolDisplay{Kind: engine.ToolCommand, Command: args.text("command")}
	case "read":
		return &engine.ToolDisplay{Kind: engine.ToolRead, Path: args.text("path")}
	case "write":
		return writeDisplay(args.text("path"), args.text("content"))
	case "edit":
		return replaceDisplay(args.text("path"), args.text("oldText"), args.text("newText"))
	case "grep", "find":
		return &engine.ToolDisplay{Kind: engine.ToolSearch, Query: args.text("pattern"), Path: args.text("path")}
	}
	return other()
}

// settledDisplay is the end display of a call whose result body is not
// stored: the start display, with an agent call's outcome from its error flag.
func settledDisplay(start *engine.ToolDisplay, isError bool) *engine.ToolDisplay {
	if start == nil {
		return other()
	}
	display := engine.CloneToolDisplay(start)
	if display.Kind == engine.ToolSubagent && display.Status == "" {
		display.Status = "completed"
		if isError {
			display.Status = "failed"
		}
	}
	return display
}

// finishDisplay redacts every display string with the run's configured
// secrets and the record markers, and only then bounds it, so a secret cut by
// a field bound cannot leave a prefix behind. It never fails: if the secrets
// cannot be prepared, only the kind is kept.
func finishDisplay(raw *engine.ToolDisplay, secrets []string) *engine.ToolDisplay {
	if raw == nil {
		return nil
	}
	redactor, err := redaction.NewRedactor(secrets...)
	if err != nil {
		return &engine.ToolDisplay{Kind: engine.BoundToolDisplay(engine.ToolDisplay{Kind: raw.Kind}).Kind}
	}
	return engine.BoundToolDisplay(engine.MapToolDisplayStrings(*raw, redactor.Text))
}

// redactToolInput redacts tool arguments as JSON with the run's secrets; an
// input that cannot be redacted or outgrows the event bound is dropped (the
// private journal keeps it).
func redactToolInput(input json.RawMessage, secrets []string) json.RawMessage {
	if len(input) == 0 {
		return input
	}
	clean, err := redaction.RedactJSON(input, secrets...)
	if err != nil || len(clean) > 16<<10 {
		return nil
	}
	return clean
}
