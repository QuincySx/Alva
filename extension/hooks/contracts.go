// INPUT: context, encoding/json, time, agent/core, native/process, shared storage contracts, llm/wire.
// OUTPUT: Event, Entry, Config, Activity, Input, Output and Receipt for explicitly approved lifecycle commands.
// POS: [L4 extension] Hook configuration and typed execution evidence stay outside SDK and engine-specific product contracts.
package hooks

import (
	"context"
	"encoding/json"
	"time"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Storage is the capability contract supplied by the outer assembly.
type Storage interface {
	storage.Journal
}

type Event string

const (
	PreToolUse         Event = "PreToolUse"
	PostToolUse        Event = "PostToolUse"
	PostToolUseFailure Event = "PostToolUseFailure"
	SessionStart       Event = "SessionStart"
	SessionEnd         Event = "SessionEnd"
	Stop               Event = "Stop"
	UserPromptSubmit   Event = "UserPromptSubmit"
	Notification       Event = "Notification"
)

func (event Event) valid() bool {
	switch event {
	case PreToolUse, PostToolUse, PostToolUseFailure, SessionStart, SessionEnd, Stop, UserPromptSubmit, Notification:
		return true
	}
	return false
}

type Entry struct {
	ID                 string
	Event              Event
	Matcher            string
	Process            process.Config
	Timeout            time.Duration
	ApprovedHostAccess bool
}
type Activity struct {
	ID, Name, SessionID, RunID, CallID string
	Effect                             abi.EffectKind
}
type Config struct {
	Repository            Storage
	Owner, SessionID, Cwd string
	Entries               []Entry
	Locks                 *core.ToolLockRegistry
	// Called before every actual start. Products adapt this to their durable
	// activity admission; direct harnesses supply their own explicit reporter.
	Report func(context.Context, Activity) error
}
type Input struct {
	Event        Event            `json:"hook_event"`
	SessionID    string           `json:"session_id"`
	RunID        string           `json:"run_id"`
	Cwd          string           `json:"cwd"`
	ToolName     string           `json:"tool_name,omitempty"`
	CallID       string           `json:"tool_call_id,omitempty"`
	ToolInput    json.RawMessage  `json:"tool_input,omitempty"`
	ToolResponse *wire.ToolResult `json:"tool_response,omitempty"`
	Error        string           `json:"error,omitempty"`
	Prompt       string           `json:"prompt,omitempty"`
}
type Output struct {
	Continue           *bool           `json:"continue,omitempty"`
	StopReason         string          `json:"stop_reason,omitempty"`
	AdditionalContext  string          `json:"additional_context,omitempty"`
	PermissionDecision string          `json:"permission_decision,omitempty"`
	UpdatedInput       json.RawMessage `json:"updated_input,omitempty"`
}
type Receipt struct {
	ID           string          `json:"id"`
	HookID       string          `json:"hook_id"`
	Event        Event           `json:"event"`
	Status       string          `json:"status"`
	Error        string          `json:"error,omitempty"`
	ExitCode     int             `json:"exit_code"`
	Started      bool            `json:"started"`
	Reaped       bool            `json:"reaped"`
	Complete     bool            `json:"complete"`
	Input        wire.Reference  `json:"input"`
	Stdout       wire.Reference  `json:"stdout"`
	Stderr       wire.Reference  `json:"stderr"`
	Context      *wire.Reference `json:"additional_context,omitempty"`
	SourceNotice string          `json:"source_notice"`
}
