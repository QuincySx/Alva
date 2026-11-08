// INPUT: Actual harness tool waves, an in-memory SDK session and a deterministic fixture Tool
// OUTPUT: Tool start arguments and result prefixes stay bounded, marked truncated and paired with their native references; built-in names carry their display
// POS: [L7 Alva product adapter] Engine tool-evidence conformance without network, product layer or presentation rules
package alva

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// fixtureTool returns whatever result the test hands it, read-only. It is
// registered by pointer: the security plugin trusts only a comparable instance.
type fixtureTool struct {
	name   string
	result wire.ToolResult
}

func (t fixtureTool) Name() string                    { return t.name }
func (fixtureTool) Description() string               { return "Deterministic test tool." }
func (fixtureTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (t fixtureTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	result := t.result
	result.CallID = call.ID
	return result, nil
}
func (fixtureTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (fixtureTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Returns a fixture result."}
}
func (fixtureTool) IsReadOnly(wire.ToolCall) bool { return true }

// runToolCall drives one actual model tool call and returns its engine events
// together with the native records they reference.
func runToolCall(t *testing.T, arguments string, result wire.ToolResult) (start, end engine.Event, records []abi.SessionRecord) {
	t.Helper()
	return runNamedToolCall(t, "fixture_tool", arguments, result)
}

func runNamedToolCall(t *testing.T, name, arguments string, result wire.ToolResult) (start, end engine.Event, records []abi.SessionRecord) {
	t.Helper()
	var log *core.MemorySession
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		log = core.NewMemorySession(id)
		turn := 0
		model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			turn++
			if turn == 1 {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call-1", Name: name, Arguments: json.RawMessage(arguments)}}); err != nil {
					return err
				}
			} else if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "done"}); err != nil {
				return err
			}
			return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		})
		return harness.NewBuilder(model, log).Tool(&fixtureTool{name: name, result: result}).Build(ctx)
	})
	var contract engine.StreamContract
	starts, ends := 0, 0
	err := adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		switch event.Kind {
		case engine.ToolStart:
			start = event
			starts++
		case engine.ToolEnd:
			end = event
			ends++
		}
		return contract.Accept(event)
	})
	if err != nil || starts != 1 || ends != 1 || !contract.Terminal {
		t.Fatal("tool call did not produce one accepted start/end pair", err, starts, ends)
	}
	records, err = log.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	return start, end, records
}

func recordAt(t *testing.T, records []abi.SessionRecord, reference *engine.Reference) abi.SessionRecord {
	t.Helper()
	if reference == nil {
		t.Fatal("tool event lacks its native reference")
	}
	for _, record := range records {
		if record.ID == reference.RecordID && record.Sequence == reference.Sequence {
			return record
		}
	}
	t.Fatal("tool event reference does not name an actual native record", reference)
	return abi.SessionRecord{}
}

func TestToolEventsCarryBoundedArgumentsAndResult(t *testing.T) {
	arguments := `{"path":"notes/plan.md","query":"todo"}`
	result := wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "line one"}, {Kind: wire.ContentText, Text: "line two"}}}
	start, end, records := runToolCall(t, arguments, result)
	if string(start.Input) != arguments || start.Effect != engine.EffectUnknown || start.Name != "fixture_tool" {
		t.Fatal("start lost its bounded arguments or invented an effect", string(start.Input), start.Effect)
	}
	if intent := recordAt(t, records, start.Reference); intent.ToolIntent == nil || string(intent.ToolIntent.Call.Arguments) != arguments {
		t.Fatal("start reference is not the tool intent", intent)
	}
	if end.Text != "line one\nline two" || end.Truncated || end.IsError || end.Name != "fixture_tool" || strings.Contains(end.Text, "retained in session records") {
		t.Fatal("end is not the actual result text", end.Text, end.Truncated)
	}
	if outcome := recordAt(t, records, end.Reference); outcome.ToolOutcome == nil || outcome.ToolOutcome.CallID != "call-1" {
		t.Fatal("end reference is not the tool outcome", outcome)
	}
}

func TestToolEventsLeaveOversizedEvidenceToReferences(t *testing.T) {
	// A multi-byte rune straddles the 8 KiB prefix so the cut must back off.
	long := strings.Repeat("o", toolResultPrefix-1) + "果" + strings.Repeat("o", 4096)
	arguments := `{"content":"` + strings.Repeat("x", toolInputLimit) + `"}`
	start, end, records := runToolCall(t, arguments, wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: long}}})
	if start.Input != nil || start.Reference == nil {
		t.Fatal("oversized arguments entered the event instead of staying behind the reference", len(start.Input))
	}
	if intent := recordAt(t, records, start.Reference); intent.ToolIntent == nil || string(intent.ToolIntent.Call.Arguments) != arguments {
		t.Fatal("complete arguments are not reachable through the start reference")
	}
	if !end.Truncated || len(end.Text) > toolResultPrefix || !utf8.ValidString(end.Text) || !strings.HasPrefix(long, end.Text) || len(end.Text) < toolResultPrefix-4 {
		t.Fatal("oversized result is not a marked UTF-8 prefix", end.Truncated, len(end.Text))
	}
	outcome := recordAt(t, records, end.Reference)
	if outcome.ToolOutcome == nil || outcome.ToolOutcome.Result.Content[0].Text != long {
		t.Fatal("complete result is not reachable through the end reference")
	}
	// Exactly at the bound nothing is dropped.
	exact := strings.Repeat("e", toolResultPrefix)
	if _, end, _ := runToolCall(t, `{}`, wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: exact}}}); end.Truncated || end.Text != exact {
		t.Fatal("a result at the bound was marked truncated", end.Truncated, len(end.Text))
	}
}

func TestToolEventsKeepErrorResultsAndReferenceLocators(t *testing.T) {
	_, end, _ := runToolCall(t, `{}`, wire.ToolResult{IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Old text is missing or ambiguous"}}})
	if !end.IsError || end.Text != "Old text is missing or ambiguous" || end.Truncated || end.Reference == nil {
		t.Fatal("error result lost its text, flag or reference", end)
	}
	reference := wire.Reference{URI: "alva-artifact://fixture", MediaType: "text/plain; charset=utf-8", Bytes: 42}
	_, end, _ = runToolCall(t, `{}`, wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &reference}, {Kind: wire.ContentImage, Image: &wire.Image{Payload: wire.MediaPayload{MediaType: "image/png", Data: []byte(strings.Repeat("A", 64))}}}}})
	var locator struct {
		Reference wire.Reference `json:"reference"`
	}
	first, rest, _ := strings.Cut(end.Text, "\n")
	if json.Unmarshal([]byte(first), &locator) != nil || locator.Reference != reference || rest != "[image content retained in the native record]" || strings.Contains(end.Text, strings.Repeat("A", 64)) {
		t.Fatal("reference/media content was not rendered as a small locator", end.Text)
	}
}

func textResult(text string) wire.ToolResult {
	return wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}
}

func TestToolEventsCarryTheAlvaDisplayOfEachBuiltIn(t *testing.T) {
	start, end, _ := runToolCall(t, `{}`, textResult("x"))
	if start.Display == nil || start.Display.Kind != engine.ToolOther || end.Display == nil || end.Display.Kind != engine.ToolOther {
		t.Fatal("an unknown tool is not other", start.Display, end.Display)
	}
	start, end, _ = runNamedToolCall(t, "execute_shell", `{"command":"go test ./..."}`, textResult(`{"job_id":"j","status":{"exit_code":3},"output":"frames"}`))
	if start.Display.Kind != engine.ToolCommand || start.Display.Command != "go test ./..." || start.Display.ExitCode != nil || end.Display.ExitCode == nil || *end.Display.ExitCode != 3 || end.Display.Command != "go test ./..." {
		t.Fatalf("command display: %+v / %+v", start.Display, end.Display)
	}
	reference := wire.Reference{URI: "alva-artifact://file", MediaType: "text/plain", Bytes: 10}
	_, end, _ = runNamedToolCall(t, "read_file", `{"path":"src/main.go"}`, wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &reference}}})
	if end.Display.Kind != engine.ToolRead || end.Display.Path != "src/main.go" || end.Display.Output != reference.URI {
		t.Fatalf("read display: %+v", end.Display)
	}
	start, _, _ = runNamedToolCall(t, "edit_file", `{"path":"a.go","old":"a\nb\nc","new":"a\nB\nc\nd"}`, textResult(`{"written_bytes":9}`))
	if d := start.Display; d.Kind != engine.ToolEdit || d.Path != "a.go" || d.Files != 1 || *d.Added != 2 || *d.Removed != 1 || d.Diff == nil || !d.Diff.Partial || d.Diff.New != "a\nB\nc\nd" {
		t.Fatalf("edit display: %+v", d)
	}
	start, _, _ = runNamedToolCall(t, "write_file", `{"path":"n.go","content":"x\ny\n"}`, textResult(`{}`))
	if d := start.Display; d.Kind != engine.ToolEdit || *d.Added != 2 || d.Removed != nil || d.Diff.Old != "" {
		t.Fatalf("whole-file write display: %+v", d)
	}
	// The display counts every match of the complete result even when the
	// 8 KiB text prefix is truncated.
	var matches []string
	for i := 0; i < 400; i++ {
		matches = append(matches, `{"path":"pkg/file.go","offset":1,"text":"`+strings.Repeat("m", 40)+`"}`)
	}
	_, end, _ = runNamedToolCall(t, "search_files", `{"path":".","query":"needle"}`, textResult(`{"literal":true,"matches":[`+strings.Join(matches, ",")+`],"truncated":true}`))
	if d := end.Display; !end.Truncated || d.Kind != engine.ToolSearch || d.Query != "needle" || *d.Matches != 400 || !d.MoreMatches || len(d.Results) != engine.ToolDisplayResults {
		t.Fatalf("search display: %+v truncated=%v", d, end.Truncated)
	}
	_, end, _ = runNamedToolCall(t, "agent", `{"task":"Review the diff"}`, textResult(`{"child_session_id":"native-child","selected_output":{"uri":"alva-artifact://out"},"outcome":"completed"}`))
	if d := end.Display; d.Kind != engine.ToolSubagent || d.Task != "Review the diff" || d.ChildSessionID != "native-child" || d.Status != "completed" || d.Output != "alva-artifact://out" {
		t.Fatalf("subagent display: %+v", d)
	}
	start, _, _ = runNamedToolCall(t, "todo_write", `{"content":"- [x] one\n- [ ] two"}`, textResult(`{"saved":true}`))
	if d := start.Display; d.Kind != engine.ToolTodo || len(d.Todos) != 2 || d.Todos[0].Status != engine.TodoCompleted {
		t.Fatalf("todo display: %+v", d)
	}
	start, _, _ = runNamedToolCall(t, "todo_write", `{"content":"free-form note"}`, textResult(`{"saved":true}`))
	if d := start.Display; d.Plan != "free-form note" || d.Todos != nil {
		t.Fatalf("free-form plan display: %+v", d)
	}
}
