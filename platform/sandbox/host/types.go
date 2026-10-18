// INPUT: context, os, time, kernel/abi, sandbox/abi
// OUTPUT: Grant, Access, Limits, Request, Outcome, Observer, Capabilities, EscalationTarget, Approval, Executor
// POS: [platform / WASM host or guest] Explicit per-job native WASIp1 inputs and host-only capabilities without ambient authority.
package host

import (
	"context"
	"os"
	"time"

	kernelabi "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

type Access string

const (
	ReadOnly  Access = "read_only"
	ReadWrite Access = "read_write"
)

type Grant struct {
	Host   string
	Guest  string
	Access Access
}
type Limits struct {
	// Fuel enables the version-1 Core V2 basic-block instruction meter.
	// Zero leaves it disabled for callers explicitly choosing only deadlines.
	Fuel            uint64
	WallClock       time.Duration
	MemoryBytes     uint64
	OutputBytes     int
	MaxModelCalls   int
	MaxFetchCalls   int
	MaxEscalations  int
	MaxExchanges    int
	MaxIOBytes      uint64
	MaxOutputTokens uint32
}
type Request struct {
	JobID          string
	Module         []byte
	Args           []string
	Grants         []Grant
	AllowedDomains []string
	Context        abi.Context
	Limits         Limits
}
type Outcome struct {
	FuelLimit     uint64
	FuelRemaining uint64
	FuelExhausted bool
	ExitCode      uint32
	Stdout        []byte
	Stderr        []byte
	ModelCalls    int
	ScriptCalls   int
	FetchCalls    int
	Escalations   int
	Exchanges     int
	IOBytes       uint64
	Elapsed       time.Duration
}

// IOEvent is exact raw evidence. Chunk boundaries are not text/redaction
// boundaries. Guest audit is untrusted reporting, separate from host IO proof.
type IOEvent struct {
	JobID     string
	Sequence  uint64
	Time      time.Time
	Kind      string
	Operation uint32
	Data      []byte
}
type Observer interface {
	Observe(context.Context, IOEvent) error
}
type ObserverFunc func(context.Context, IOEvent) error

func (fn ObserverFunc) Observe(ctx context.Context, event IOEvent) error { return fn(ctx, event) }

// EscalationTarget contains a pinned directory handle. HostPath is display-only:
// an executor must not claim confinement if it uses that renameable string as
// cwd. Escalation is explicitly outside WASM; the host owns native policy.
type EscalationTarget struct {
	JobID     string
	Request   abi.EscalationRequest
	Directory *os.Root
	HostPath  string
}
type Approval interface {
	Approve(context.Context, EscalationTarget) (bool, error)
}
type ApprovalFunc func(context.Context, EscalationTarget) (bool, error)

func (fn ApprovalFunc) Approve(ctx context.Context, target EscalationTarget) (bool, error) {
	return fn(ctx, target)
}

type Executor interface {
	Execute(context.Context, EscalationTarget) (abi.EscalationResponse, error)
}
type ExecutorFunc func(context.Context, EscalationTarget) (abi.EscalationResponse, error)

func (fn ExecutorFunc) Execute(ctx context.Context, target EscalationTarget) (abi.EscalationResponse, error) {
	return fn(ctx, target)
}

type Capabilities struct {
	Model            kernelabi.LanguageModel
	Observer         Observer
	AllowUnsandboxed bool
	Approval         Approval
	Executor         Executor
	// Extra provider body fields are denied by default at this untrusted
	// boundary. A trusted host may explicitly delegate them to its model.
	AllowExtraBody bool
	References     ReferenceReader
	History        HistoryReader
}

// Readers are bound to the actual native session by the host, not a guest
// supplied owner/session identifier. No write/storage operation is delegated.
type ReferenceReader interface {
	Read(context.Context, abi.ReferenceRead) (abi.TextPage, error)
	Search(context.Context, abi.ReferenceSearch) ([]abi.TextMatch, error)
}
type HistoryReader interface {
	Page(context.Context, abi.HistoryRequest) (abi.HistoryPage, error)
}
