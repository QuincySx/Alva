// INPUT: context, encoding/json, errors, fmt
// OUTPUT: Runtime, Request, Event, Capabilities, PermissionDecision and stream validation
// POS: [L4.5 engine contract] Independent multi-engine execution contract without SDK or Hub product policy
package engine

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
)

var ErrUnsupported = errors.New("engine capability unsupported")
var ErrSessionNotFound = errors.New("engine session not found")

type Capabilities struct {
	Streaming          bool `json:"streaming"`
	Resume             bool `json:"resume"`
	Cancel             bool `json:"cancel"`
	PermissionCallback bool `json:"permission_callback"`
	ToolControl        bool `json:"tool_control"`
}
type Request struct {
	OwnerID       string
	Prompt        string
	ResumeSession string
	Workspace     string
	Model         string
	Provider      string
	AccountSource string
	BillingRoute  string
	SystemPrompt  string
	MaxTurns      int
	// ReportActivity is rebound by an adapter to its actual event sink. A
	// harness factory must await it BEFORE starting native build/prepare work;
	// ToolIntent.NoSideEffect says nothing about those separate activities.
	ReportActivity func(Activity) error `json:"-"`
	// RetainWorkspace extends the actual admitted workspace lease for an
	// explicitly owned native background resource. Call only before Execute
	// returns; release only after physical cleanup. It grants no Tool rights,
	// is never serialized, and is absent for opaque external engines.
	RetainWorkspace func() (func(), error) `json:"-"`
}
type PermissionDecision struct {
	Allow        bool
	UpdatedInput json.RawMessage
	Reason       string
}
type Runtime interface {
	Execute(context.Context, Request, func(Event) error) error
	Cancel(context.Context, string) error
	RespondPermission(context.Context, string, string, PermissionDecision) error
	Capabilities() Capabilities
}
type Kind string

const (
	SessionStarted    Kind = "session_started"
	ExecutionActivity Kind = "execution_activity"
	MessageDelta      Kind = "message_delta"
	Message           Kind = "message"
	ReasoningDelta    Kind = "reasoning_delta"
	ContentBlock      Kind = "content_block"
	ToolStart         Kind = "tool_start"
	ToolEnd           Kind = "tool_end"
	PermissionRequest Kind = "permission_request"
	Error             Kind = "error"
	Completed         Kind = "completed"
)

type Outcome string

const (
	Succeeded   Outcome = "completed"
	Failed      Outcome = "failed"
	Cancelled   Outcome = "cancelled"
	Interrupted Outcome = "interrupted"
)

type FailureKind string

const (
	FailureStartup   FailureKind = "startup"
	FailureRateLimit FailureKind = "rate_limit"
	FailureAuth      FailureKind = "authentication"
	FailureProtocol  FailureKind = "protocol"
	FailureExecution FailureKind = "execution"
	FailureCancelled FailureKind = "cancelled"
)

type Effect string

const (
	EffectRead    Effect = "read"
	EffectWrite   Effect = "write"
	EffectNone    Effect = "none"
	EffectExecute Effect = "execute"
	EffectNetwork Effect = "network"
	EffectUnknown Effect = "unknown"
)

type Activity struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	Effect Effect `json:"effect"`
}

type Reference struct {
	SessionID string `json:"session_id"`
	RecordID  string `json:"record_id,omitempty"`
	Sequence  uint64 `json:"sequence,omitempty"`
	URI       string `json:"uri,omitempty"`
}
type Usage struct {
	InputTokens  uint64 `json:"input_tokens"`
	OutputTokens uint64 `json:"output_tokens"`
}
type Event struct {
	Kind        Kind            `json:"kind"`
	SessionID   string          `json:"session_id,omitempty"`
	ID          string          `json:"id,omitempty"`
	Name        string          `json:"name,omitempty"`
	Text        string          `json:"text,omitempty"`
	Input       json.RawMessage `json:"input,omitempty"`
	Reference   *Reference      `json:"reference,omitempty"`
	Effect      Effect          `json:"effect,omitempty"`
	IsError     bool            `json:"is_error,omitempty"`
	Recoverable bool            `json:"recoverable,omitempty"`
	Failure     FailureKind     `json:"failure,omitempty"`
	Outcome     Outcome         `json:"outcome,omitempty"`
	Usage       *Usage          `json:"usage,omitempty"`
	Activity    *Activity       `json:"activity,omitempty"`
}

// StreamContract is a consumer-side invariant checker, not an execution owner.
// Completed is the only terminal edge; fatal Error may only be followed by it.
type StreamContract struct {
	Started, Terminal, Fatal bool
	NativeSessionID          string
	tools                    map[string]bool
}

// Clone permits validate-before-commit consumers to publish state only after
// their durable observation succeeds. The mutable tool set must not be shared.
func (s StreamContract) Clone() StreamContract {
	copy := s
	copy.tools = make(map[string]bool, len(s.tools))
	for id, open := range s.tools {
		copy.tools[id] = open
	}
	return copy
}

func (s *StreamContract) Accept(event Event) error {
	if s.Terminal {
		return errors.New("engine emitted after completion")
	}
	if s.Fatal && event.Kind != Completed {
		return errors.New("engine emitted after fatal error")
	}
	if len(event.Text) > 64<<10 || len(event.Input) > 16<<10 {
		return errors.New("engine event requires a bounded reference")
	}
	if event.SessionID != "" {
		if s.NativeSessionID != "" && event.SessionID != s.NativeSessionID {
			return errors.New("engine changed known native identity")
		}
		s.NativeSessionID = event.SessionID
	}
	switch event.Kind {
	case ExecutionActivity:
		if event.Activity == nil || event.Activity.ID == "" || event.Activity.Name == "" || len(event.Activity.ID) > 128 || len(event.Activity.Name) > 128 {
			return errors.New("bounded typed execution activity identity required")
		}
		switch event.Activity.Effect {
		case EffectNone, EffectRead, EffectWrite, EffectExecute, EffectNetwork, EffectUnknown:
		default:
			return errors.New("unknown activity effect")
		}
	case SessionStarted:
		if s.Started || event.SessionID == "" {
			return errors.New("engine native identity is missing or repeated")
		}
		s.Started = true
	case MessageDelta, Message, ReasoningDelta, ContentBlock:
		if event.ID == "" {
			return errors.New("message identity missing")
		}
		if event.Kind == ContentBlock && event.Name == "" {
			return errors.New("content kind missing")
		}
	case ToolStart:
		if s.tools == nil {
			s.tools = make(map[string]bool)
		}
		if event.ID == "" || event.Name == "" || s.tools[event.ID] {
			return errors.New("tool identity missing or already open")
		}
		s.tools[event.ID] = true
	case ToolEnd:
		if !s.tools[event.ID] {
			return errors.New("tool ended without start")
		}
		delete(s.tools, event.ID)
	case PermissionRequest:
		if event.ID == "" || event.Name == "" {
			return errors.New("permission identity missing")
		}
	case Error:
		if event.Text == "" {
			return errors.New("error detail missing")
		}
		s.Fatal = !event.Recoverable
	case Completed:
		if len(s.tools) != 0 {
			return errors.New("completion left tools running")
		}
		if event.Outcome != Succeeded && event.Outcome != Failed && event.Outcome != Cancelled && event.Outcome != Interrupted {
			return errors.New("unknown completion outcome")
		}
		if s.Fatal && event.Outcome == Succeeded {
			return errors.New("fatal engine falsely completed successfully")
		}
		s.Terminal = true
	default:
		return fmt.Errorf("unknown engine event %q", event.Kind)
	}
	return nil
}
