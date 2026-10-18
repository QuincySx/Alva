// INPUT:  errors, fmt, strings
// OUTPUT: Model, Message, Status and bounded presentation state transitions
// POS: [L6 application]    Go native foundation presentation state without framework or runtime ownership
package desktop

import (
	"errors"
	"fmt"
	"strings"
)

// Status describes the observed state of one runtime-owned execution.
type Status string

const (
	Idle      Status = "idle"
	Running   Status = "running"
	Stopping  Status = "stopping"
	Completed Status = "completed"
	Cancelled Status = "cancelled"
	Failed    Status = "failed"
)

const MaxDraftBytes = 32 * 1024

// Message is a presentation row, not a durable execution record.
type Message struct {
	ID     string
	RunID  string
	Role   string
	Text   string
	Status Status
	Error  string
}

// Model is confined to the UI thread. Runtime state remains in host.Controller.
// The bounded transcript is explicitly temporary and never sent as model history;
// the retained SDK session, not this view, owns in-process conversation context.
type Model struct {
	Draft       string
	Messages    []Message
	ActiveRunID string
	Status      Status
	Error       string
	Notice      string
	Demo        bool
	RouteLabel  string
	lastSeq     uint64
	maxRows     int
}

func NewModel(demo bool) *Model {
	return &Model{Demo: demo, Status: Idle, maxRows: 64}
}

func (m *Model) CanSend() bool {
	return m.ActiveRunID == "" && strings.TrimSpace(m.Draft) != "" && len(m.Draft) <= MaxDraftBytes
}

// Prompt validates without consuming the draft: an admission failure must not lose input.
func (m *Model) Prompt() (string, error) {
	if m.ActiveRunID != "" {
		return "", errors.New("a run is already active")
	}
	if strings.TrimSpace(m.Draft) == "" {
		return "", errors.New("enter a message first")
	}
	if len(m.Draft) > MaxDraftBytes {
		return "", fmt.Errorf("message exceeds %d KiB; use a durable session for file references", MaxDraftBytes/1024)
	}
	return m.Draft, nil
}

// Admitted starts presentation only after the runtime accepted the prompt.
func (m *Model) Admitted(runID, prompt string) {
	m.ActiveRunID, m.Status, m.Error, m.lastSeq = runID, Running, "", 0
	m.Draft = ""
	m.Messages = append(m.Messages,
		Message{ID: runID + "/user", RunID: runID, Role: "You", Text: prompt, Status: Completed},
		Message{ID: runID + "/assistant", RunID: runID, Role: "Alva", Status: Running},
	)
	if len(m.Messages) > m.maxRows {
		kept := append([]Message(nil), m.Messages[len(m.Messages)-m.maxRows:]...)
		m.Messages = kept
		m.Notice = "Earlier presentation rows were evicted. This foundation does not persist or restore chat history."
	}
}

// Observe replaces the active reply from the bounded runtime snapshot. This also
// reconciles a disconnected/slow subscription without replaying any work.
func (m *Model) Observe(runID string, seq uint64, text string, status Status, failure string) bool {
	if runID != m.ActiveRunID || seq < m.lastSeq {
		return false
	}
	if m.Status == Stopping && status == Running {
		status = Stopping // progress after Stop is not a cancellation acknowledgement
	}
	m.lastSeq, m.Status, m.Error = seq, status, failure
	for i := len(m.Messages) - 1; i >= 0; i-- {
		row := &m.Messages[i]
		if row.RunID == runID && row.Role == "Alva" {
			row.Text, row.Status, row.Error = text, status, failure
			break
		}
	}
	if status == Completed || status == Cancelled || status == Failed {
		m.ActiveRunID = ""
	}
	return true
}

func (m *Model) RequestStop() string {
	if m.ActiveRunID == "" || m.Status == Stopping {
		return ""
	}
	m.Status = Stopping
	return m.ActiveRunID
}

func (m *Model) FailAdmission(err error) { m.Error = err.Error() }

func (m *Model) StatusText() string {
	switch m.Status {
	case Running:
		return "Running — execution is owned by the runtime"
	case Stopping:
		return "Stopping — waiting for runtime acknowledgement"
	case Completed:
		return "Completed"
	case Cancelled:
		return "Stopped — partial output retained"
	case Failed:
		return "Failed — no automatic retry or fallback"
	default:
		return "Ready — nothing runs until you send"
	}
}
