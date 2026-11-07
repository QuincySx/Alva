// INPUT: Bounded committed Hub history and shared desktop message values
// OUTPUT: Stable-ID Transcript projection with typed message/status rows
// POS: [L7 Hub product] Presentation-only conversion; never a model context or durable fact authority
package hubdesktop

import (
	"github.com/QuincySx/Alva/apps/hub"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"strconv"
)

// Transcript projects bounded product pages. Its rows never become model input.
type Transcript struct {
	Entries     []hub.HistoryEntry
	Rows        []view.Message
	First, Last uint64
	MoreBefore  bool
	tools       map[string]transcriptTool
	origins     map[string]transcriptOrigin
}

// Native identity comes from committed events, never the next-turn selection.
type transcriptOrigin struct {
	sessionID, nativeID string
	ambiguous           bool
}

// Result references and native ownership remain separate from each other and row text.
type transcriptTool struct {
	sessionID string
	nativeID  string
	name      string
	reference engine.Reference
	hasRef    bool
}

func (t *Transcript) rememberTool(id string, entry hub.HistoryEntry) {
	previous := t.tools[id]
	tool := transcriptTool{sessionID: entry.SessionID, nativeID: entry.Event.SessionID, name: entry.Event.Name}
	if tool.name == "" {
		tool.name = previous.name
	}
	if entry.Event.Reference != nil {
		tool.reference, tool.hasRef = *entry.Event.Reference, true
		if tool.nativeID == "" {
			tool.nativeID = tool.reference.SessionID
		}
	}
	// Synthetic cancellation/settlement ends can omit both identity fields. Keep
	// the same call's owner, without presenting its start record as a result.
	if tool.nativeID == "" && entry.AttemptID != "" && previous.sessionID == tool.sessionID {
		tool.nativeID = previous.nativeID
	}
	t.tools[id] = tool
}

func (t *Transcript) Replace(entries []hub.HistoryEntry) {
	t.Entries = append([]hub.HistoryEntry(nil), entries...)
	t.project()
}
func (t *Transcript) Append(entries []hub.HistoryEntry) {
	for _, entry := range entries {
		if entry.Sequence > t.Last {
			t.Entries = append(t.Entries, entry)
		}
	}
	if len(t.Entries) > 512 {
		t.Entries = append([]hub.HistoryEntry(nil), t.Entries[len(t.Entries)-512:]...)
	}
	t.project()
}
func (t *Transcript) Prepend(entries []hub.HistoryEntry) {
	var older []hub.HistoryEntry
	for _, entry := range entries {
		if t.First == 0 || entry.Sequence < t.First {
			older = append(older, entry)
		}
	}
	t.Entries = append(older, t.Entries...)
	if len(t.Entries) > 512 {
		t.Entries = t.Entries[:512]
	}
	t.project()
}
func (t *Transcript) project() {
	t.Rows = nil
	t.First = 0
	t.Last = 0
	t.tools = make(map[string]transcriptTool)
	t.origins = make(map[string]transcriptOrigin)
	for _, entry := range t.Entries {
		if entry.AttemptID == "" || entry.Event.SessionID == "" {
			continue
		}
		origin, exists := t.origins[entry.AttemptID]
		if exists && (origin.sessionID != entry.SessionID || origin.nativeID != entry.Event.SessionID) {
			origin.ambiguous = true
		} else if !exists {
			origin = transcriptOrigin{sessionID: entry.SessionID, nativeID: entry.Event.SessionID}
		}
		t.origins[entry.AttemptID] = origin
	}
	indices := make(map[string]int)
	for _, entry := range t.Entries {
		if t.First == 0 || entry.Sequence < t.First {
			t.First = entry.Sequence
		}
		if entry.Sequence > t.Last {
			t.Last = entry.Sequence
		}
		if entry.Input != "" {
			id := entry.TurnID + "/input"
			if _, exists := indices[id]; !exists {
				indices[id] = len(t.Rows)
				t.Rows = append(t.Rows, view.Message{ID: id, RunID: entry.AttemptID, Role: "You", Text: entry.Input, Status: view.Completed})
			}
			continue
		}
		if entry.Timeline != nil {
			event := entry.Timeline
			t.Rows = append(t.Rows, view.Message{ID: "product/" + strconv.FormatUint(entry.Sequence, 10), Role: "Product · " + event.Kind, Text: event.Text + "\n" + event.FromBranchID + " → " + event.ToBranchID, Status: view.Completed})
			continue
		}
		event := entry.Event
		id := entry.AttemptID + "/" + event.ID
		switch event.Kind {
		case engine.ExecutionActivity:
			if event.Activity != nil {
				a := event.Activity
				known := ""
				if event.SessionID != "" {
					known = " Known native execution identity retained for inspection; this does not say the Agent started."
				}
				t.Rows = append(t.Rows, view.Message{ID: entry.AttemptID + "/activity/" + a.ID, RunID: entry.AttemptID, Role: "Execution activity · " + a.Name, Text: "Committed before activity start; effect=" + string(a.Effect) + ". This is not a Tool.Execute start/result or proof of completion." + known, Status: view.Completed})
			}
		case engine.ReasoningDelta, engine.ContentBlock:
			index, exists := indices[id]
			if !exists {
				index = len(t.Rows)
				indices[id] = index
				role := "Thinking"
				if event.Kind == engine.ContentBlock && event.Name != "reasoning" {
					role = "Content · " + event.Name
				}
				t.Rows = append(t.Rows, view.Message{ID: id, RunID: entry.AttemptID, Role: role, Status: view.Running})
			}
			if event.Kind == engine.ReasoningDelta {
				t.Rows[index].Text += event.Text
			} else {
				t.Rows[index].Text = event.Text
				t.Rows[index].Status = view.Completed
			}
		case engine.MessageDelta, engine.Message:
			index, exists := indices[id]
			if !exists {
				index = len(t.Rows)
				indices[id] = index
				t.Rows = append(t.Rows, view.Message{ID: id, RunID: entry.AttemptID, Role: "Alva", Status: view.Running})
			}
			if event.Kind == engine.Message {
				t.Rows[index].Text = event.Text
				t.Rows[index].Status = view.Completed
			} else {
				t.Rows[index].Text += event.Text
			}
		case engine.ToolStart:
			indices[id] = len(t.Rows)
			t.Rows = append(t.Rows, view.Message{ID: id, RunID: entry.AttemptID, Role: "Tool · " + event.Name, Text: event.Text, Status: view.Running})
			t.rememberTool(id, entry)
		case engine.ToolEnd:
			index, exists := indices[id]
			if !exists {
				index = len(t.Rows)
				indices[id] = index
				t.Rows = append(t.Rows, view.Message{ID: id, RunID: entry.AttemptID, Role: "Tool · " + event.Name})
			}
			t.Rows[index].Text = event.Text
			t.rememberTool(id, entry)
			t.Rows[index].Status = view.Completed
			if event.IsError {
				t.Rows[index].Status = view.Failed
			}
		case engine.Error:
			id := entry.AttemptID + "/error/" + strconv.FormatUint(entry.Sequence, 10)
			t.Rows = append(t.Rows, view.Message{ID: id, RunID: entry.AttemptID, Role: "Status", Text: event.Text, Status: view.Failed})
			if event.Recoverable {
				t.Rows[len(t.Rows)-1].Role = "Retry warning"
			}
		case engine.Completed:
			if event.Text != "" {
				t.Rows = append(t.Rows, view.Message{ID: entry.AttemptID + "/settlement/" + strconv.FormatUint(entry.Sequence, 10), RunID: entry.AttemptID, Role: "Status", Text: event.Text, Status: view.Status(event.Outcome)})
			}
			for i := range t.Rows {
				if t.Rows[i].RunID == entry.AttemptID && t.Rows[i].Role != "You" && t.Rows[i].Status == view.Running {
					t.Rows[i].Status = view.Status(event.Outcome)
				}
			}
		}
	}
}
