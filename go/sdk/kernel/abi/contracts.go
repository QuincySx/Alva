// INPUT: context, encoding/json, time, llm/wire
// OUTPUT: LanguageModel, Tool, ConversationReader, ConversationLog, SessionStore, PersistentSession, Middleware, Event
// POS: [L1 kernel contracts] Minimal execution contracts with typed durable facts and no agent-product policy.
package abi

import (
	"context"
	"encoding/json"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type ModelRequest struct {
	Messages []wire.Message
	Tools    []wire.ToolDefinition
	Config   wire.ModelConfig
}

type LanguageModel interface {
	Stream(context.Context, ModelRequest, func(wire.StreamChunk) error) error
}

// ModelIdentity is optional read-only metadata. Missing identity is unknown, not
// a reason to reject a third-party model or guess its provider from a route.
type ModelIdentity interface {
	ProviderID() string
	ModelID() string
}

type Access string

const (
	AccessRead  Access = "read"
	AccessWrite Access = "write"
)

type ResourceClaim struct {
	Resource string
	Access   Access
}
type Scheduling struct {
	ConcurrentSafe    bool
	Resources         []ResourceClaim
	ExecutionMode     ExecutionMode
	ManagesOwnTimeout bool
}
type ExecutionMode string

const (
	Parallel     ExecutionMode = "parallel"
	SerialGlobal ExecutionMode = "serial_global"
	Coordinator  ExecutionMode = "coordinator"
)

type EffectKind string

const (
	EffectRead    EffectKind = "read"
	EffectWrite   EffectKind = "write"
	EffectExecute EffectKind = "execute"
	EffectNetwork EffectKind = "network"
	EffectUnknown EffectKind = "unknown"
)

type Effect struct {
	Kind        EffectKind
	Description string
	Resources   []ResourceClaim
}

// A registered Tool's identity, schema and semantics must remain stable for its
// lifetime. Publish upgrades through ToolRegistry.Replace; an invocation binds
// one directory generation, and later registry changes affect the next round.
type Tool interface {
	Name() string
	Description() string
	ParametersSchema() json.RawMessage
	Execute(context.Context, ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error)
	Scheduling(wire.ToolCall) Scheduling
	DescribeEffect(wire.ToolCall) Effect
	IsReadOnly(wire.ToolCall) bool
}

type ToolExecutionContext struct {
	SessionID    string
	RunID        string
	CallID       string
	Workspace    string
	Progress     EventSink
	Capabilities bus.Handle
	Scheduling   Scheduling
	Session      ConversationLog
	// ActualTool is the exact resolved implementation selected by the runtime,
	// not a caller-supplied name assertion. Unknown/disallowed calls carry nil.
	ActualTool Tool
}

type RecordKind string

const (
	RecordConversation RecordKind = "conversation"
	RecordModelIntent  RecordKind = "model_intent"
	RecordModelOutcome RecordKind = "model_outcome"
	RecordToolIntent   RecordKind = "tool_intent"
	RecordToolOutcome  RecordKind = "tool_outcome"
	RecordExtension    RecordKind = "extension"
)

type OutcomeStatus string

const (
	OutcomeSucceeded OutcomeStatus = "succeeded"
	OutcomeFailed    OutcomeStatus = "failed"
	OutcomeCancelled OutcomeStatus = "cancelled"
)

type ModelIntent struct {
	RequestID string `json:"request_id"`
}
type ModelOutcome struct {
	RequestID string        `json:"request_id"`
	Status    OutcomeStatus `json:"status"`
	Error     string        `json:"error,omitempty"`
	Usage     *wire.Usage   `json:"usage,omitempty"`
}
type ToolIntent struct {
	RunID string        `json:"run_id"`
	Call  wire.ToolCall `json:"call"`
	// DeclaredCall retains the original model declaration when prepare changed
	// arguments. Call is the frozen effective execution input, never a rewrite
	// of the assistant conversation fact.
	DeclaredCall *wire.ToolCall `json:"declared_call,omitempty"`
	// NoSideEffect records only that Tool.Execute and execution hooks will not
	// be called (prepare supplied a result or aborted the batch). Prepare/plugin
	// IO can itself have effects: this is NOT permission to replay an attempt.
	NoSideEffect bool       `json:"no_side_effect,omitempty"`
	Effect       Effect     `json:"effect"`
	Scheduling   Scheduling `json:"scheduling"`
	ReadOnly     bool       `json:"read_only"`
}
type ToolOutcome struct {
	RunID  string          `json:"run_id"`
	CallID string          `json:"call_id"`
	Status OutcomeStatus   `json:"status"`
	Result wire.ToolResult `json:"result"`
	Error  string          `json:"error,omitempty"`
}

// Exactly one payload must correspond to Kind. Sequence/ID/time are store-owned.
type SessionRecord struct {
	Sequence      uint64           `json:"sequence"`
	ID            string           `json:"id"`
	Time          time.Time        `json:"time"`
	Namespace     string           `json:"namespace"`
	Type          string           `json:"type"`
	SchemaVersion uint32           `json:"schema_version"`
	Parent        *RecordReference `json:"parent,omitempty"`
	Kind          RecordKind       `json:"kind"`
	Payload       json.RawMessage  `json:"payload,omitempty"`
	Message       *wire.Message    `json:"message,omitempty"`
	ModelIntent   *ModelIntent     `json:"model_intent,omitempty"`
	ModelOutcome  *ModelOutcome    `json:"model_outcome,omitempty"`
	ToolIntent    *ToolIntent      `json:"tool_intent,omitempty"`
	ToolOutcome   *ToolOutcome     `json:"tool_outcome,omitempty"`
}

type RecordReference struct {
	SessionID string `json:"session_id"`
	RecordID  string `json:"record_id"`
	Sequence  uint64 `json:"sequence"`
}

type ConversationReader interface {
	ID() string
	Messages(context.Context) ([]wire.Message, error)
}

type ConversationLog interface {
	ConversationReader
	AppendRecords(context.Context, []SessionRecord) ([]SessionRecord, error)
	Flush(context.Context) error
}

type SessionStore interface {
	Records(context.Context, uint64) ([]SessionRecord, error)
}

// AppendRecords atomically commits the entire batch or none. Conversation records
// form the model transcript; intent/outcome records do not enter model context.
type PersistentSession interface {
	ConversationLog
	SessionStore
}

type EventKind string

const (
	EventRunStarted     EventKind = "run_started"
	EventTextDelta      EventKind = "text_delta"
	EventReasoningDelta EventKind = "reasoning_delta"
	EventContentBlock   EventKind = "content_block"
	// EventMessageCommitted is one complete assistant model response after
	// successful atomic append, not a partial stream or proof of run success.
	// It can contain intermediate tool calls; Record names the durable fact.
	EventMessageCommitted EventKind = "message_committed"
	EventToolStarted      EventKind = "tool_started"
	EventToolCompleted    EventKind = "tool_completed"
	EventUsage            EventKind = "usage"
	EventRunCompleted     EventKind = "run_completed"
	EventRunFailed        EventKind = "run_failed"
)

type Event struct {
	Kind         EventKind          `json:"kind"`
	Text         string             `json:"text,omitempty"`
	ContentBlock *wire.ContentBlock `json:"content_block,omitempty"`
	Message      *wire.Message      `json:"message,omitempty"`
	ToolCall     *wire.ToolCall     `json:"tool_call,omitempty"`
	ToolResult   *wire.ToolResult   `json:"tool_result,omitempty"`
	Record       *RecordReference   `json:"record,omitempty"`
	Usage        *wire.Usage        `json:"usage,omitempty"`
	Error        string             `json:"error,omitempty"`
}

type EventSink interface {
	Emit(context.Context, Event) error
}

// ToolFunc is a zero-or-one continuation for this wrapper layer. Repeated,
// concurrent and late calls are rejected; wrappers cannot suppress a protocol
// violation or an actual inner execution failure. Accepted work always joins.
type ToolFunc func(context.Context, ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error)

// ToolBlockedError is an intentional, model-recoverable BeforeTool veto. Other
// middleware errors (including wrapper approval failures) abort the run.
type ToolBlockedError struct{ Reason string }

func (err *ToolBlockedError) Error() string { return err.Reason }

// Optional hooks keep middleware focused; no required no-op implementations.
type Middleware interface{ Name() string }
type BeforeModelHook interface {
	BeforeModel(context.Context, RuntimeContext, *ModelRequest) error
}
type AfterModelHook interface {
	AfterModel(context.Context, RuntimeContext, *wire.Message) error
}
type InputCommittedHook interface {
	InputCommitted(context.Context, RuntimeContext, wire.Message) error
}

// ModelFunc admits sequential attempts within an open wrapper lease. Private
// buffering/sampling may retry before logical output is published. Once any
// logical chunk is published, a later invocation failure cannot be swallowed
// into success. Concurrent or late next calls are rejected; accepted work joins.
type ModelFunc func(context.Context, RuntimeContext, ModelRequest, func(wire.StreamChunk) error) error
type ModelWrapper interface {
	WrapModel(context.Context, RuntimeContext, ModelRequest, func(wire.StreamChunk) error, ModelFunc) error
}

// ModelAdmissionHook checks the final request at each captured LanguageModel
// interface entry, after wrappers. It receives an independent deep copy and may
// only allow or reject; its changes never alter the request sent to the model.
// A rejected attempt performs no model IO and may be retried with a new request.
// This does not inspect opaque transports inside a composite LanguageModel, or
// authorize cached data returned by a short-circuiting wrapper.
type ModelAdmissionHook interface {
	AdmitModel(context.Context, RuntimeContext, ModelRequest) error
}
type BeforeToolHook interface {
	BeforeTool(context.Context, ToolExecutionContext, wire.ToolCall) error
}

// ToolPreparation is a checked union: zero means continue, Arguments replaces
// only arguments, Result short-circuits the remaining preparation chain and
// tool execution. Supplying both is invalid. Call identity is never mutable.
type ToolPreparation struct {
	Arguments json.RawMessage
	Result    *wire.ToolResult
}

// ToolPreparationContext binds preparation to the same implementation that was
// advertised for this invocation. ActualTool is nil for unknown/disallowed
// calls; looking up the mutable Runtime.State registry is not this binding.
type ToolPreparationContext struct {
	Runtime    RuntimeContext
	ActualTool Tool
}

// PrepareTool runs outer-to-inner before effects/scheduling are computed and
// before any tool intent is committed. Inputs and returns are deeply owned.
// Plugins may perform their own IO, but no Tool.Execute is authorized here.
type PrepareToolHook interface {
	PrepareTool(context.Context, ToolPreparationContext, wire.ToolCall) (ToolPreparation, error)
}
type AfterToolHook interface {
	AfterTool(context.Context, ToolExecutionContext, wire.ToolCall, *wire.ToolResult) error
}
type ToolWrapper interface {
	WrapTool(context.Context, ToolExecutionContext, wire.ToolCall, ToolFunc) (wire.ToolResult, error)
}
type RunStartHook interface {
	OnRunStart(context.Context, RuntimeContext) error
}
type RunEndHook interface {
	OnRunEnd(context.Context, RuntimeContext, error) error
}
