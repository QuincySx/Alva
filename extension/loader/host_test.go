// INPUT: Actual native SQLite sessions past the 4096-record and 64 MiB whole-history budgets, short in-flight sessions and AEP state callbacks
// OUTPUT: Bounded recent-window get_messages/count_tokens regressions with truncation markers, no poisoning and unchanged short-session pages
// POS: [L4 extension] Proves AEP state reads never load the whole native session while keeping complete short-session results.
package loader

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/protocol/aep"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// newStatePlugin binds a Plugin to an actual native session with one live
// event handle; state callbacks never touch the AEP child process.
func newStatePlugin(t *testing.T) (*Plugin, storage.Session) {
	t.Helper()
	repo, err := session.Open(context.Background(), filepath.Join(t.TempDir(), "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	log, err := repo.Create(context.Background(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	refs, err := repo.BoundReferences(context.Background(), "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	p := &Plugin{config: PluginConfig{Repository: repo, Owner: "owner", NativeID: "native", Workspace: t.TempDir()}, log: log, refs: refs, gate: make(chan struct{}, 1), subscribed: map[string]bool{}}
	p.handle = &stateHandle{id: "s-1", runID: "run"}
	return p, log
}

func appendConversation(t *testing.T, log storage.Session, messages ...wire.Message) {
	t.Helper()
	for len(messages) > 0 {
		batch := make([]abi.SessionRecord, 0, 500)
		bytes := 0
		// Stay well inside the store's per-batch record and byte budgets.
		for len(messages) > 0 && len(batch) < 500 && bytes < 16<<20 {
			message := messages[0]
			messages = messages[1:]
			bytes += len(message.Text())
			batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}))
		}
		if _, err := log.AppendRecords(t.Context(), batch); err != nil {
			t.Fatal(err)
		}
	}
}

func alternating(prefix string, count int, size int) []wire.Message {
	out := make([]wire.Message, count)
	for i := range out {
		role := wire.RoleUser
		if i%2 == 1 {
			role = wire.RoleAssistant
		}
		text := fmt.Sprintf("%s-%d", prefix, i)
		if size > len(text) {
			text += strings.Repeat("x", size-len(text))
		}
		out[i] = wire.TextMessage(role, text)
	}
	return out
}

type statePage struct {
	Messages       []aep.MessageDTO `json:"messages"`
	Offset         int              `json:"offset"`
	NextOffset     int              `json:"next_offset"`
	HasMore        bool             `json:"has_more"`
	Truncated      bool             `json:"truncated"`
	OmittedAtLeast int              `json:"omitted_at_least"`
	Tokens         int              `json:"tokens"`
	Counted        int              `json:"counted_messages"`
}

func stateRead(t *testing.T, p *Plugin, method string, params map[string]any) statePage {
	t.Helper()
	params["handle"] = "s-1"
	raw, _ := json.Marshal(params)
	data, err := p.Handle(t.Context(), method, raw)
	if err != nil {
		t.Fatal(method, err)
	}
	var page statePage
	if err := json.Unmarshal(data, &page); err != nil {
		t.Fatal(err)
	}
	return page
}

func dtoText(t *testing.T, dto aep.MessageDTO) string {
	t.Helper()
	message, err := aep.MessageFromDTO(dto)
	if err != nil {
		t.Fatal(err)
	}
	return message.Text()
}

func TestStateMessagesServeBoundedRecentWindowInLongSession(t *testing.T) {
	p, log := newStatePlugin(t)
	// 66 one-MiB messages exceed the 64 MiB whole-history budget, then 4100
	// small ones push the session past 4096 records.
	appendConversation(t, log, alternating("large", 66, 1<<20)...)
	appendConversation(t, log, alternating("small", 4100, 0)...)
	if _, err := log.Messages(t.Context()); err == nil {
		t.Fatal("fixture still fits the whole-history read that used to poison the Plugin")
	}
	first := stateRead(t, p, "host/state.get_messages", map[string]any{"limit": 16})
	if len(first.Messages) != 16 || first.Offset != 0 || first.NextOffset != 16 || !first.HasMore || !first.Truncated || first.OmittedAtLeast < 1 {
		t.Fatalf("recent window page not marked bounded: %+v", first)
	}
	if text := dtoText(t, first.Messages[0]); text != fmt.Sprintf("small-%d", 4100-stateTailMessages) {
		t.Fatal("window does not start at the oldest retained recent message", text)
	}
	last := stateRead(t, p, "host/state.get_messages", map[string]any{"limit": 16, "offset": stateTailMessages - 16})
	if len(last.Messages) != 16 || last.HasMore || !last.Truncated || dtoText(t, last.Messages[15]) != "small-4099" {
		t.Fatalf("window lost the newest messages: %+v", last)
	}
	tokens := stateRead(t, p, "host/state.count_tokens", map[string]any{})
	if tokens.Counted != stateTailMessages || !tokens.Truncated || tokens.OmittedAtLeast < 1 || tokens.Tokens <= 0 {
		t.Fatalf("token estimate scope not reported: %+v", tokens)
	}
	if err := p.healthy(); err != nil {
		t.Fatal("bounded state reads poisoned the Plugin", err)
	}
}

func TestStateMessagesWindowStopsAtByteBudget(t *testing.T) {
	p, log := newStatePlugin(t)
	appendConversation(t, log, alternating("small", 2, 0)...)
	appendConversation(t, log, alternating("wide", 3, 3<<20)...)
	tokens := stateRead(t, p, "host/state.count_tokens", map[string]any{})
	if tokens.Counted != 2 || !tokens.Truncated || tokens.OmittedAtLeast != 3 {
		t.Fatalf("8 MiB byte budget not enforced: %+v", tokens)
	}
	page := stateRead(t, p, "host/state.get_messages", map[string]any{"limit": 16})
	if len(page.Messages) != 2 || page.HasMore || !page.Truncated || page.OmittedAtLeast != 3 {
		t.Fatalf("byte-bounded page: %+v", page)
	}
	for _, dto := range page.Messages {
		if len(dto.Content) != 1 || dto.Content[0].Reference == nil {
			t.Fatal("large recent message not returned as a reference", dto)
		}
	}
	if err := p.healthy(); err != nil {
		t.Fatal(err)
	}
}

func TestStateMessagesShortSessionUnchangedIncludingInFlightToolCall(t *testing.T) {
	p, log := newStatePlugin(t)
	done := wire.ToolCall{ID: "done", Name: "tool", Arguments: json.RawMessage(`{"n":1}`)}
	pending := wire.ToolCall{ID: "pending", Name: "tool", Arguments: json.RawMessage(`{"n":2}`)}
	result := wire.ToolResult{CallID: "done", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "finished"}}}
	messages := []wire.Message{
		wire.TextMessage(wire.RoleUser, "first question"),
		{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &done}}},
		{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}},
		wire.TextMessage(wire.RoleAssistant, "first answer"),
		wire.TextMessage(wire.RoleUser, "second question"),
		// Committed before execution; after_tool_call observes it unresolved.
		{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &pending}}},
	}
	appendConversation(t, log, messages...)
	complete, err := log.Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	for _, request := range []struct{ limit, offset int }{{10, 0}, {2, 0}, {2, 4}, {16, 6}} {
		page := stateRead(t, p, "host/state.get_messages", map[string]any{"limit": request.limit, "offset": request.offset})
		start := min(request.offset, len(complete))
		end := min(start+request.limit, len(complete))
		want := make([]aep.MessageDTO, 0, end-start)
		for _, message := range complete[start:end] {
			dto, err := aep.MessageToDTO(message)
			if err != nil {
				t.Fatal(err)
			}
			want = append(want, dto)
		}
		got, _ := json.Marshal(page.Messages)
		expected, _ := json.Marshal(want)
		if string(got) != string(expected) || page.Offset != start || page.NextOffset != end || page.HasMore != (end < len(complete)) || page.Truncated || page.OmittedAtLeast != 0 {
			t.Fatalf("short-session page %+v changed:\n got %s\nwant %s", request, got, expected)
		}
	}
	tokens := stateRead(t, p, "host/state.count_tokens", map[string]any{})
	if tokens.Counted != len(complete) || tokens.Truncated || tokens.OmittedAtLeast != 0 {
		t.Fatalf("short-session estimate scope: %+v", tokens)
	}
	if err := p.healthy(); err != nil {
		t.Fatal(err)
	}
}
