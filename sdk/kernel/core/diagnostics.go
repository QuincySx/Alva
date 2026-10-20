// INPUT: context and kernel/abi.
// OUTPUT: Optional DiagnosticObserver and owned generic model/hook observations.
// POS: [L2 kernel runtime] Reports optional observations without making their persistence execution authority.
package core

import (
	"context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// DiagnosticObserver is optionally provided by an outer telemetry Plugin over
// Bus. DefaultAgentLoop is its consumer; absence means no diagnostic persistence.
// Known execution dependencies do not use this optional discovery path.
// Implementations must support bounded contexts and report errors without
// changing execution decisions. Execution intents/outcomes never use this port.
type DiagnosticObserver interface {
	ModelStart(context.Context, abi.ConversationLog, ModelObservation) error
	ModelEnd(context.Context, abi.ConversationLog, StreamObservation) error
	Hooks(context.Context, abi.ConversationLog, []HookTrace) error
	ReportFailure(context.Context, string, string, error)
}
type ModelObservation struct {
	RunID, RequestID, SystemPrompt string
	Model                          abi.LanguageModel
	Request                        abi.ModelRequest
}
type HookTrace struct {
	RunID      string            `json:"run_id"`
	Hook       string            `json:"hook"`
	ToolCallID string            `json:"tool_call_id,omitempty"`
	Calls      []HookObservation `json:"calls"`
}

func reportDiagnostic(ctx context.Context, observer DiagnosticObserver, session abi.ConversationLog, operation string, err error) {
	if err != nil {
		observer.ReportFailure(ctx, session.ID(), operation, err)
	}
}
