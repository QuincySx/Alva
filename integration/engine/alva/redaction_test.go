// INPUT: Actual harness tool waves, the real redacting native session store and fixture tools named like Alva built-ins
// OUTPUT: A credential in tool arguments or results is [REDACTED] alike in native records and in ToolStart/ToolEnd Input, Text and Display; shell receipts become status
// POS: [L7 Alva product adapter] Product events never show what the native store hides
package alva

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const fakeBearer = "fake-bearer-token-0123456789"
const fakeAPIKey = "fake-api-key-abcdef"

// storedToolCall runs one tool call against the real native store and
// returns its engine events and the stored records' JSON.
func storedToolCall(t *testing.T, name, arguments string, result wire.ToolResult) (start, end engine.Event, stored string) {
	t.Helper()
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	var log abi.PersistentSession
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		created, err := repo.Create(ctx, "owner", id, "")
		if err != nil {
			return nil, err
		}
		log = created
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
		return harness.NewBuilder(model, created).Workspace(t.TempDir()).Tool(&fixtureTool{name: name, result: result}).Build(ctx)
	})
	var contract engine.StreamContract
	err = adapter.Execute(ctx, engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		switch event.Kind {
		case engine.ToolStart:
			start = event
		case engine.ToolEnd:
			end = event
		}
		return contract.Accept(event)
	})
	if err != nil || start.Kind == "" || end.Kind == "" {
		t.Fatal("tool call did not complete", err)
	}
	records, err := log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(records)
	return start, end, string(data)
}

func eventJSON(events ...engine.Event) string {
	data, _ := json.Marshal(events)
	return string(data)
}

func TestToolEventsAreRedactedLikeTheNativeRecords(t *testing.T) {
	command := `curl -H "Authorization: Bearer ` + fakeBearer + `" https://example.test/api`
	arguments, _ := json.Marshal(map[string]string{"command": command})
	start, end, stored := storedToolCall(t, "execute_shell", string(arguments), textResult(`{"job_id":"j","status":{"task":{"status":"succeeded"},"exit_code":0},"output":"Use read_shell_output"}`))
	if strings.Contains(stored, fakeBearer) || !strings.Contains(stored, "[REDACTED]") {
		t.Fatal("fixture precondition: the native store did not redact the command")
	}
	if events := eventJSON(start, end); strings.Contains(events, fakeBearer) || !strings.Contains(string(start.Input), "[REDACTED]") || start.Display.Command != "[REDACTED]" || end.Display.Command != "[REDACTED]" {
		t.Fatalf("command credential reached product events: input=%s display=%+v", start.Input, start.Display)
	}

	matches := `{"literal":true,"matches":[{"path":".env","offset":0,"text":"API_KEY=` + fakeAPIKey + `"},{"path":"main.go","offset":4,"text":"ordinary line"}],"truncated":false}`
	start, end, stored = storedToolCall(t, "search_files", `{"path":".","query":"API_KEY"}`, textResult(matches))
	if strings.Contains(stored, fakeAPIKey) || !strings.Contains(stored, "[REDACTED]") {
		t.Fatal("fixture precondition: the native store did not redact the search hit")
	}
	if events := eventJSON(start, end); strings.Contains(events, fakeAPIKey) {
		t.Fatalf("search credential reached product events: text=%s display=%+v", end.Text, end.Display)
	}
	if !strings.Contains(end.Text, "[REDACTED]") || !strings.Contains(end.Text, "ordinary line") || end.Display.Results[0].Text != "[REDACTED]" || end.Display.Results[1].Text != "ordinary line" || *end.Display.Matches != 2 {
		t.Fatalf("redaction lost ordinary content or the redacted marker: text=%s display=%+v", end.Text, end.Display)
	}

	diff, _ := json.Marshal(map[string]string{"path": "config.env", "old": "TOKEN=old", "new": "client_secret=" + fakeAPIKey})
	start, _, stored = storedToolCall(t, "edit_file", string(diff), textResult(`{"written_bytes":3}`))
	if strings.Contains(stored, fakeAPIKey) || strings.Contains(eventJSON(start), fakeAPIKey) || start.Display.Diff == nil || start.Display.Diff.New != "[REDACTED]" {
		t.Fatalf("edit credential reached product events or the store: display=%+v", start.Display)
	}
}

func TestShellReceiptBecomesStatusNotOutput(t *testing.T) {
	_, end, _ := runNamedToolCall(t, "execute_shell", `{"command":"make test"}`, textResult(`{"job_id":"shell-1","status":{"task":{"status":"failed"},"exit_code":2,"Reaped":true},"input":{"uri":"alva-artifact://in"},"output":"Use read_shell_output with this job_id for complete captured frame references"}`))
	if d := end.Display; !d.OutputInTrace || d.Status != "failed" || d.ExitCode == nil || *d.ExitCode != 2 || d.Command != "make test" {
		t.Fatalf("shell receipt display: %+v", d)
	}
	_, end, _ = runNamedToolCall(t, "execute_shell", `{"command":"sleep 100","run_in_background":true}`, textResult(`{"job_id":"shell-2","status":{"task":{"status":"running"}},"input":{"uri":"alva-artifact://in"},"lifetime":"this_run_only"}`))
	if d := end.Display; !d.OutputInTrace || d.Status != "running" || d.ExitCode != nil {
		t.Fatalf("background receipt display: %+v", d)
	}
	_, end, _ = runNamedToolCall(t, "execute_shell", `{"command":"x"}`, textResult("shell job budget exceeds 8 per run"))
	if d := end.Display; d.OutputInTrace || d.Status != "" {
		t.Fatalf("a non-receipt result was treated as a receipt: %+v", d)
	}
}
