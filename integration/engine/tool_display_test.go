// INPUT: encoding/json, strings, testing and canonical tool displays
// OUTPUT: Display bounds, Bound output always valid for hostile strings in every field, string mapping, kind/field ownership, event placement and shared line/diff/checklist helpers
// POS: [L4.5 engine contract] Portable tool-display conformance independent of concrete engines or tool names
package engine

import (
	"encoding/json"
	"math/rand"
	"strings"
	"testing"
)

func count(n int) *int { return &n }

func TestToolDisplayAcceptsBoundedDisplaysOnToolEventsOnly(t *testing.T) {
	var c StreamContract
	start := Event{Kind: ToolStart, ID: "t", Name: "x", Display: &ToolDisplay{Kind: ToolEdit, Path: "a.go", Files: 1, Added: count(2), Removed: count(1), Diff: &ToolDiff{Old: "a", New: "b\nc", Partial: true}}}
	end := Event{Kind: ToolEnd, ID: "t", Name: "x", Text: "ok", Display: start.Display}
	for _, event := range []Event{start, end} {
		if err := c.Accept(event); err != nil {
			t.Fatal("bounded display rejected", event.Kind, err)
		}
	}
	for _, event := range []Event{
		{Kind: Message, ID: "m", Text: "x", Display: &ToolDisplay{Kind: ToolOther}},
		{Kind: PermissionRequest, ID: "p", Name: "x", Display: &ToolDisplay{Kind: ToolOther}},
	} {
		probe := c.Clone()
		if probe.Accept(event) == nil {
			t.Fatal("display accepted on a non-tool event", event.Kind)
		}
	}
}

func TestToolDisplayValidationRejectsMalformedOrOversized(t *testing.T) {
	long := strings.Repeat("x", 2048)
	many := make([]ToolMatch, ToolDisplayResults+1)
	for _, display := range []ToolDisplay{
		{Kind: "shell"},
		{Kind: ""},
		{Kind: ToolCommand, Command: long},
		{Kind: ToolRead, Path: strings.Repeat("p", 4096)},
		{Kind: ToolEdit, Diff: &ToolDiff{Old: long}},
		{Kind: ToolEdit, Added: count(-1)},
		{Kind: ToolSearch, Results: many},
		{Kind: ToolTodo, Todos: []TodoItem{{Text: "x", Status: "doing"}}},
		{Kind: ToolSubagent, Status: "Completed"},
		{Kind: ToolCommand, Path: "a.go"},
		{Kind: ToolRead, Command: "ls"},
		{Kind: ToolOther, Output: "alva-artifact://x"},
		{Kind: ToolCommand, Command: "bad\x00"},
	} {
		if (&display).Validate() == nil {
			t.Fatalf("malformed display accepted: %+v", display)
		}
		event := Event{Kind: ToolStart, ID: "t", Name: "x", Display: &display}
		var c StreamContract
		if c.Accept(event) == nil {
			t.Fatalf("contract accepted malformed display: %+v", display)
		}
	}
}

func TestBoundToolDisplayClipsWithinTheEncodedBudget(t *testing.T) {
	var results []ToolMatch
	var todos []TodoItem
	for i := 0; i < 80; i++ {
		results = append(results, ToolMatch{Path: strings.Repeat("d/", 300) + "f.go", Line: -3, Text: strings.Repeat("长", 200)})
		todos = append(todos, TodoItem{Text: strings.Repeat("t", 400), Status: "weird"})
	}
	raw := ToolDisplay{Kind: ToolSearch, Query: strings.Repeat("q", 4096) + "\nsecond", Path: "root", Matches: count(80), Results: results, Command: "dropped", Todos: todos}
	bounded := BoundToolDisplay(raw)
	if err := bounded.Validate(); err != nil {
		t.Fatal("bounded display does not validate", err)
	}
	data, _ := json.Marshal(bounded)
	if len(data) > ToolDisplayBytes || bounded.Command != "" || bounded.Todos != nil || *bounded.Matches != 80 || len(bounded.Query) > 512 {
		t.Fatal("bound kept foreign fields, lost counts or exceeded the budget", len(data), bounded.Command, len(bounded.Todos))
	}
	for _, match := range bounded.Results {
		if match.Line < 0 || len(match.Text) > 160 {
			t.Fatal("result not clipped", match.Line, len(match.Text))
		}
	}
	diff := BoundToolDisplay(ToolDisplay{Kind: ToolEdit, Path: "a.go", Diff: &ToolDiff{Old: strings.Repeat("old line\n", 400), New: "short\n"}})
	if !diff.Diff.Partial || len(diff.Diff.Old) > 1024 || !strings.HasSuffix(diff.Diff.Old, "\n") || diff.Diff.New != "short\n" {
		t.Fatal("clipped diff is not a marked line-boundary prefix", diff.Diff.Partial, len(diff.Diff.Old))
	}
	if unknown := BoundToolDisplay(ToolDisplay{Kind: "shell", Command: "ls"}); unknown.Kind != ToolOther || unknown.Command != "" {
		t.Fatal("unknown kind did not become other", unknown)
	}
	todo := BoundToolDisplay(ToolDisplay{Kind: ToolTodo, Todos: todos})
	if err := todo.Validate(); err != nil || len(todo.Todos) == 0 || todo.Todos[0].Status != TodoPending {
		t.Fatal("todo list not bounded or statuses not normalized", err, len(todo.Todos))
	}
}

func TestToolDisplayLineDiffAndChecklistHelpers(t *testing.T) {
	if added, removed := LineChanges("a\nb\nc\n", "a\nB\nc\nd\n"); added != 2 || removed != 1 {
		t.Fatal("line changes", added, removed)
	}
	if added, removed := LineChanges("", "x\ny"); added != 2 || removed != 0 {
		t.Fatal("pure addition", added, removed)
	}
	old, updated, added, removed := UnifiedDiff("--- a/a.go\n+++ b/a.go\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n")
	if old != "a\nb\nc" || updated != "a\nB\nc" || added != 1 || removed != 1 {
		t.Fatal("unified diff", old, updated, added, removed)
	}
	if LineCount("one\ntwo") != 2 || LineCount("one\n") != 1 || LineCount("") != 0 {
		t.Fatal("line count")
	}
	todos := Checklist("# Plan\n- [x] done\n* [~] doing\n- [ ] next\nnot an item")
	if len(todos) != 3 || todos[0] != (TodoItem{Text: "done", Status: TodoCompleted}) || todos[1].Status != TodoInProgress || todos[2].Status != TodoPending || Checklist("free text") != nil {
		t.Fatal("checklist", todos)
	}
}

// hostileStrings covers NUL, invalid UTF-8, control characters that expand
// under JSON escaping, upper case, long and multi-byte text.
func hostileStrings(random *rand.Rand) []string {
	values := []string{"", "\x00", "a\x00b", "\xff\xfe", "ok\xc3", strings.Repeat("\x01", 3000), "MIXED Case", strings.Repeat("长", 2000), "line\nnext\nmore", strings.Repeat("x", 9000)}
	for i := 0; i < 20; i++ {
		data := make([]byte, random.Intn(3000))
		random.Read(data)
		values = append(values, string(data))
	}
	return values
}

func TestBoundToolDisplayOutputAlwaysValidates(t *testing.T) {
	random := rand.New(rand.NewSource(13))
	values := hostileStrings(random)
	kinds := []ToolKind{ToolCommand, ToolRead, ToolEdit, ToolSearch, ToolSubagent, ToolTodo, ToolOther}
	for _, kind := range kinds {
		for i, value := range values {
			other := values[(i+7)%len(values)]
			display := ToolDisplay{Kind: kind, Command: value, ExitCode: count(-3), OutputInTrace: true, Path: other, Files: -1, Added: count(-1), Removed: count(2), Diff: &ToolDiff{Old: value, New: other}, Query: value, Matches: count(-5), MoreMatches: true, Task: other, ChildSessionID: value, Status: value, Plan: other, Output: value}
			for j := 0; j < 40; j++ {
				display.Results = append(display.Results, ToolMatch{Path: values[(i+j)%len(values)], Line: -j, Text: values[(i+2*j)%len(values)]})
				display.Todos = append(display.Todos, TodoItem{Text: values[(i+3*j)%len(values)], Status: TodoStatus(values[j%len(values)])})
			}
			bounded := BoundToolDisplay(display)
			if err := bounded.Validate(); err != nil {
				t.Fatalf("kind %s value %d: bounded display rejected: %v", kind, i, err)
			}
			event := Event{Kind: ToolEnd, ID: "t", Name: "x", Display: bounded}
			probe := StreamContract{tools: map[string]bool{"t": true}}
			if err := probe.Accept(event); err != nil {
				t.Fatalf("kind %s value %d: contract rejected a bounded display: %v", kind, i, err)
			}
		}
	}
}

func TestMapToolDisplayStringsVisitsEveryStringField(t *testing.T) {
	display := ToolDisplay{Kind: ToolSearch, Command: "c", Path: "p", Query: "q", Task: "t", ChildSessionID: "s", Status: "st", Plan: "pl", Output: "o", Diff: &ToolDiff{Old: "do", New: "dn"}, Results: []ToolMatch{{Path: "rp", Text: "rt"}}, Todos: []TodoItem{{Text: "tt", Status: TodoPending}}}
	mapped := MapToolDisplayStrings(display, func(value string) string { return "(" + value + ")" })
	data, _ := json.Marshal(mapped)
	for _, value := range []string{"c", "p", "q", "t", "s", "st", "pl", "o", "do", "dn", "rp", "rt", "tt"} {
		if !strings.Contains(string(data), `"(`+value+`)"`) {
			t.Fatal("string field not mapped", value, string(data))
		}
	}
	if mapped.Kind != ToolSearch || display.Diff.Old != "do" || display.Results[0].Text != "rt" || mapped.Todos[0].Status != TodoPending {
		t.Fatal("mapping changed Kind/status or mutated the input", mapped)
	}
}
