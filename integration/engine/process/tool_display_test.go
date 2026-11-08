// INPUT: encoding/json, strings, testing, the vendor mapper and Codex item display mapping
// OUTPUT: Codex app-server/exec items and Claude/Pi tool frames carry their display; unknown tools are other; result bodies stay out; displays are redacted per string field before bounding and never fail on numeric literals
// POS: [integration / native external-engine adapter] Pure mapping tests without subprocesses, accounts or product state
package process

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

func TestCodexItemDisplays(t *testing.T) {
	command := codexItemDisplay("commandExecution", json.RawMessage(`{"id":"i","type":"commandExecution","command":"go test ./...","exitCode":1,"aggregatedOutput":"FAIL"}`))
	if command.Kind != engine.ToolCommand || command.Command != "go test ./..." || command.ExitCode == nil || *command.ExitCode != 1 {
		t.Fatalf("command: %+v", command)
	}
	change := codexItemDisplay("fileChange", json.RawMessage(`{"id":"i","type":"fileChange","changes":[{"path":"a.go","kind":{"type":"update","move_path":null},"diff":"@@ -1,2 +1,2 @@\n a\n-b\n+B\n"},{"path":"b.go","kind":{"type":"add"},"diff":"one\ntwo\n"}]}`))
	if change.Kind != engine.ToolEdit || change.Path != "a.go" || change.Files != 2 || *change.Added != 3 || *change.Removed != 1 || change.Diff == nil || !change.Diff.Partial || change.Diff.New != "a\nB" {
		t.Fatalf("file change: %+v %+v", change, change.Diff)
	}
	search := codexItemDisplay("webSearch", json.RawMessage(`{"id":"i","type":"webSearch","query":"mygo diff"}`))
	image := codexItemDisplay("imageView", json.RawMessage(`{"id":"i","type":"imageView","path":"/w/a.png"}`))
	collab := codexItemDisplay("collabToolCall", json.RawMessage(`{"id":"i","type":"collabToolCall","prompt":"Investigate","status":"failed"}`))
	mcp := codexItemDisplay("mcpToolCall", json.RawMessage(`{"id":"i","type":"mcpToolCall","server":"s","tool":"t"}`))
	if search.Kind != engine.ToolSearch || search.Query != "mygo diff" || image.Kind != engine.ToolRead || image.Path != "/w/a.png" || collab.Kind != engine.ToolSubagent || collab.Task != "Investigate" || collab.Status != "failed" || mcp.Kind != engine.ToolOther {
		t.Fatalf("codex items: %+v %+v %+v %+v", search, image, collab, mcp)
	}
	for _, display := range []*engine.ToolDisplay{command, change, search, image, collab, mcp} {
		if err := display.Validate(); err != nil {
			t.Fatal("codex display invalid", err)
		}
	}
}

func TestCodexExecClaudeAndPiFramesCarryDisplays(t *testing.T) {
	m := newMapper("codex", "engine-journal://run")
	events, err := m.mapLine([]byte(`{"type":"item.started","item":{"id":"c1","type":"command_execution","command":"ls","status":"in_progress"}}`))
	if err != nil || len(events) != 1 || events[0].Display == nil || events[0].Display.Command != "ls" {
		t.Fatal("codex exec start display", events, err)
	}
	events, err = m.mapLine([]byte(`{"type":"item.completed","item":{"id":"c1","type":"command_execution","command":"ls","exit_code":2,"status":"failed"}}`))
	if err != nil || len(events) != 1 || events[0].Display == nil || *events[0].Display.ExitCode != 2 || !events[0].IsError {
		t.Fatal("codex exec end display", events, err)
	}

	claude := newMapper("claude", "engine-journal://run")
	frames := []string{
		`{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"/w/a.go","old_string":"x := 1","new_string":"x := 2"}},{"type":"tool_use","id":"t2","name":"Task","input":{"description":"Find callers","prompt":"Search"}},{"type":"tool_use","id":"t3","name":"TodoWrite","input":{"todos":[{"content":"ship","status":"in_progress","activeForm":"Shipping"}]}},{"type":"tool_use","id":"t4","name":"NotebookEdit","input":{"notebook_path":"n.ipynb"}}]}}`,
		`{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"SECRET RESULT BODY"},{"type":"tool_result","tool_use_id":"t2","is_error":true,"content":"x"}]}}`,
	}
	var all []engine.Event
	for _, frame := range frames {
		events, err := claude.mapLine([]byte(frame))
		if err != nil {
			t.Fatal(err)
		}
		all = append(all, events...)
	}
	byKey := map[string]engine.Event{}
	for _, event := range all {
		byKey[string(event.Kind)+"/"+event.ID] = event
		if strings.Contains(event.Text, "SECRET RESULT BODY") || event.Display != nil && strings.Contains(event.Display.Plan+event.Display.Task, "SECRET") {
			t.Fatal("a Claude result body entered a product event")
		}
	}
	if d := byKey["tool_start/t1"].Display; d == nil || d.Kind != engine.ToolEdit || d.Path != "/w/a.go" || *d.Added != 1 || *d.Removed != 1 {
		t.Fatalf("claude edit: %+v", d)
	}
	if d := byKey["tool_end/t1"].Display; d == nil || d.Kind != engine.ToolEdit || d.Path != "/w/a.go" {
		t.Fatalf("claude edit end repeats its display: %+v", d)
	}
	if d := byKey["tool_end/t2"].Display; d == nil || d.Kind != engine.ToolSubagent || d.Task != "Find callers" || d.Status != "failed" {
		t.Fatalf("claude task end: %+v", d)
	}
	if d := byKey["tool_start/t3"].Display; d == nil || len(d.Todos) != 1 || d.Todos[0] != (engine.TodoItem{Text: "ship", Status: engine.TodoInProgress}) {
		t.Fatalf("claude todo: %+v", d)
	}
	if d := byKey["tool_start/t4"].Display; d == nil || d.Kind != engine.ToolOther {
		t.Fatalf("unknown claude tool: %+v", d)
	}

	pi := newMapper("pi", "engine-journal://run")
	start, err := pi.mapLine([]byte(`{"type":"tool_execution_start","toolCallId":"p1","toolName":"bash","args":{"command":"make test"}}`))
	if err != nil || len(start) != 1 || start[0].Display.Kind != engine.ToolCommand || start[0].Display.Command != "make test" {
		t.Fatal("pi start display", start, err)
	}
	end, err := pi.mapLine([]byte(`{"type":"tool_execution_end","toolCallId":"p1","toolName":"bash","isError":false,"result":{"content":"ok"}}`))
	if err != nil || len(end) != 1 || end[0].Display.Command != "make test" || end[0].Display.ExitCode != nil {
		t.Fatal("pi end display", end, err)
	}
	if d := piDisplay("edit", json.RawMessage(`{"path":"a.go","oldText":"a","newText":"b\nc"}`)); d.Kind != engine.ToolEdit || *d.Added != 2 || *d.Removed != 1 {
		t.Fatalf("pi edit: %+v", d)
	}
	if d := piDisplay("ls", json.RawMessage(`{"path":"."}`)); d.Kind != engine.ToolOther {
		t.Fatalf("pi unknown: %+v", d)
	}
}

func TestFinishDisplayRedactsStringFieldsWithoutFailing(t *testing.T) {
	secret := "sk-test-0123456789abcdef"
	display := finishDisplay(&engine.ToolDisplay{Kind: engine.ToolCommand, Command: "curl -H 'X-Key: " + secret + "' example.test"}, []string{secret})
	if strings.Contains(display.Command, secret) || display.Kind != engine.ToolCommand || display.Validate() != nil {
		t.Fatal("display kept a configured secret", display)
	}
	// A configured literal that is also a number (a free-form answer "1", an
	// inherited *_TOKEN=1) must not break numeric fields or the run.
	exit := 1
	matches := 1
	numeric := finishDisplay(&engine.ToolDisplay{Kind: engine.ToolCommand, Command: "exit 1", ExitCode: &exit}, []string{"1"})
	if numeric == nil || numeric.ExitCode == nil || *numeric.ExitCode != 1 || strings.Contains(numeric.Command, "1") || numeric.Validate() != nil {
		t.Fatalf("numeric literal broke the display: %+v", numeric)
	}
	search := finishDisplay(&engine.ToolDisplay{Kind: engine.ToolSearch, Query: "1", Matches: &matches, MoreMatches: true}, []string{"1", "true"})
	if search == nil || search.Matches == nil || *search.Matches != 1 || !search.MoreMatches || search.Validate() != nil {
		t.Fatalf("numeric/bool literal broke the display: %+v", search)
	}
	if finishDisplay(nil, []string{secret}) != nil {
		t.Fatal("nil display changed")
	}
	var many []string
	for i := 0; i < 600; i++ {
		many = append(many, fmt.Sprintf("secret-value-%04d", i))
	}
	if degraded := finishDisplay(&engine.ToolDisplay{Kind: engine.ToolCommand, Command: "secret-value-0001"}, many); degraded == nil || degraded.Kind != engine.ToolCommand || degraded.Command != "" {
		t.Fatalf("unpreparable secrets did not degrade to the kind: %+v", degraded)
	}
}

func TestDisplaysAreRedactedBeforeTheyAreBounded(t *testing.T) {
	secret := "zz-configured-secret-value"
	long := strings.Repeat("a", 505) + " " + secret
	item, _ := json.Marshal(map[string]any{"id": "i", "type": "commandExecution", "command": long})
	codex := finishDisplay(codexItemDisplay("commandExecution", item), []string{secret})
	m := newMapper("pi", "engine-journal://run")
	args, _ := json.Marshal(map[string]string{"command": long})
	frame, _ := json.Marshal(map[string]any{"type": "tool_execution_start", "toolCallId": "p1", "toolName": "bash", "args": json.RawMessage(args)})
	events, err := m.mapLine(frame)
	if err != nil || len(events) != 1 {
		t.Fatal(err, events)
	}
	pi := finishDisplay(events[0].Display, []string{secret})
	for name, display := range map[string]*engine.ToolDisplay{"codex": codex, "pi": pi} {
		if strings.Contains(display.Command, "zz-") || len(display.Command) > 512 || display.Validate() != nil {
			t.Fatalf("%s: a clipped secret prefix survived: %q", name, display.Command[len(display.Command)-20:])
		}
	}
}
