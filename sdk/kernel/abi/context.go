// INPUT: context, time, llm/wire
// OUTPUT: ContextSource, ContextSystem, ContextHooks, ContextHandle, ContextApply and lifecycle values
// POS: [L1 kernel contracts] Context lifecycle mechanism contracts separated from compression and retrieval policy.
package abi

import (
	"context"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"time"
)

type ContextLayer string

const (
	AlwaysPresent ContextLayer = "always_present"
	OnDemand      ContextLayer = "on_demand"
	RuntimeInject ContextLayer = "runtime_inject"
	MemoryLayer   ContextLayer = "memory"
)

type Priority int

const (
	Disposable Priority = iota
	Low
	Normal
	High
	Critical
)

type ContextMetadata struct {
	Layer                                                     ContextLayer
	Priority                                                  Priority
	EstimatedTokens                                           int
	Compacted                                                 bool
	ExternalizedPath, ReplacementSummary, SourceAgent, Origin string
	CreatedAt                                                 time.Time
	CreatedAtTurn                                             uint64
	Excluded                                                  bool
}
type ContextEntry struct {
	ID       string
	Message  wire.Message
	Metadata ContextMetadata
}
type LayerStats struct {
	Tokens, Entries int
	Percentage      float64
}
type EntrySnapshot struct {
	ID              string
	Layer           ContextLayer
	Priority        Priority
	EstimatedTokens int
	Origin          string
	AgeTurns        uint64
	Preview         string
}
type ContextSnapshot struct {
	TotalTokens, BudgetTokens, ModelWindow int
	UsageRatio                             float64
	Layers                                 map[ContextLayer]LayerStats
	Entries                                []EntrySnapshot
	ToolPatterns                           []ToolPattern
}
type ToolPattern struct {
	ToolName                                          string
	CallCount, AverageResultTokens, TotalResultTokens int
}
type BudgetInfo struct {
	ModelWindow, BudgetTokens, UsedTokens, RemainingTokens int
	UsageRatio                                             float64
}
type MemoryFact struct {
	ID, Text, Fingerprint, Category, SourceSession string
	Confidence                                     float64
	CreatedAt, LastAccessedAt                      time.Time
	AccessCount                                    uint64
}
type SelectorKind string

const (
	FromStart SelectorKind = "from_start"
	ToEnd     SelectorKind = "to_end"
	ByIndex   SelectorKind = "by_index"
	ByID      SelectorKind = "by_id"
)

type MessageSelector struct {
	Kind  SelectorKind
	Index int
	ID    string
}
type MessageRange struct{ From, To MessageSelector }
type PromptSection struct {
	ID, Content string
	Priority    Priority
}
type InjectionKind string

const (
	InjectSystem  InjectionKind = "system"
	InjectSkill   InjectionKind = "skill"
	InjectMessage InjectionKind = "message"
	InjectRuntime InjectionKind = "runtime"
	InjectMemory  InjectionKind = "memory"
)

type Injection struct {
	Kind            InjectionKind
	Layer           ContextLayer
	Priority        Priority
	Section         *PromptSection
	SkillName, Text string
	Message         *wire.Message
	Memory          []MemoryFact
}
type IngestKind string

const (
	IngestKeep   IngestKind = "keep"
	IngestSkip   IngestKind = "skip"
	IngestModify IngestKind = "modify"
)

type IngestAction struct {
	Kind     IngestKind
	Message  *wire.Message
	Priority *Priority
}
type CompressionKind string

const (
	CompressWindow      CompressionKind = "window"
	CompressSummary     CompressionKind = "summary"
	CompressToolResult  CompressionKind = "tool_result"
	CompressExternalize CompressionKind = "externalize"
	CompressPriority    CompressionKind = "priority"
)

type CompressAction struct {
	Kind                     CompressionKind
	KeepRecent               int
	Range                    MessageRange
	Hints                    []string
	MessageID, Summary, Path string
	Priority                 Priority
}
type TokenCounter interface {
	CountTokens(string) int
	ContextWindow() int
}
type ContextHooks interface{ Name() string }

// ContextBootstrapHook starts the current run's context and is called again
// when that run enters a different session. A reused hook must support repeated
// startup. Bootstrap and Dispose are not one-to-one per session.
type ContextBootstrapHook interface {
	Bootstrap(context.Context, ContextHandle, string) error
}
type ContextIngestHook interface {
	Ingest(context.Context, ContextHandle, string, ContextEntry) (IngestAction, error)
}
type ContextMessageHook interface {
	OnMessage(context.Context, ContextHandle, string, wire.Message) ([]Injection, error)
}
type ContextAssembleHook interface {
	Assemble(context.Context, ContextHandle, string, []ContextEntry, int) ([]ContextEntry, error)
}
type ContextBudgetHook interface {
	OnBudgetExceeded(context.Context, ContextHandle, string, ContextSnapshot) ([]CompressAction, error)
}
type ContextAfterTurnHook interface {
	AfterTurn(context.Context, ContextHandle, string) error
}

// ContextDisposeHook cleans up once per Run, including failed/partial startup,
// with the bounded cleanup context. It is not an Agent-lifetime or per-session
// close. Stateful implementations must isolate ownership between agents/runs;
// sharing an instance does not grant permission to close another run's work.
type ContextDisposeHook interface{ Dispose(context.Context) error }

// ContextHandle is the working-set state consumed by context lifecycle hooks.
// File access, memory services, summarization and externalization are L3
// capabilities; implementing this interface grants none of those capabilities.
type ContextHandle interface {
	Snapshot(string) ContextSnapshot
	Budget(string) BudgetInfo
	Entries(string) []ContextEntry
	ReadMessage(string, string) (ContextEntry, bool)
	InjectMessage(string, ContextLayer, wire.Message) error
	TagPriority(string, string, Priority) error
	TagExclude(string, string) error
	AdvanceTurn(string)
}

// ContextSource builds an owned model working view from the actual bound log.
// It must include the latest committed input and preserve tool protocol groups.
// A source may use bounded native queries; it must not change the original log,
// silently fall back after failure, or treat a session ID as an access grant.
type ContextSource interface {
	Load(context.Context, ConversationReader) ([]wire.Message, error)
}

type ContextApply interface {
	ApplyInjections(context.Context, []Injection, *[]string, *[]wire.Message) error
	ApplyCompressions(context.Context, []CompressAction, *[]wire.Message, ContextHandle, string) error
}
type ContextSystem struct {
	Hooks        ContextHooks
	Handle       ContextHandle
	Apply        ContextApply
	Counter      TokenCounter
	BudgetTokens int
}
