// INPUT: context, encoding/json, errors, log/slog, agent/core and kernel/abi/bus/core.
// OUTPUT: Optional telemetry Plugin with explicit error reporting and body-free durable observations.
// POS: [L4 extension] Diagnostic persistence is best effort; execution facts remain owned by the kernel log.
package telemetry

import (
	"context"
	"encoding/json"
	"log/slog"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
)

type Failure struct {
	SessionID, Operation string
	Err                  error
}
type Plugin struct {
	report func(context.Context, Failure)
}

// New uses the process logger if report is nil. It reports only fixed metadata
// by default, so private storage/transport error text does not leak to logs.
func New(report func(context.Context, Failure)) *Plugin {
	if report == nil {
		report = func(ctx context.Context, f Failure) {
			slog.WarnContext(ctx, "optional Agent telemetry failed", "session", f.SessionID, "operation", f.Operation)
		}
	}
	return &Plugin{report: report}
}
func (*Plugin) Name() string { return "telemetry" }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	return bus.Provide[kernel.DiagnosticObserver](r.BusWriter(), p)
}
func (p *Plugin) ReportFailure(ctx context.Context, id, operation string, err error) {
	p.report(ctx, Failure{id, operation, err})
}
func (*Plugin) ModelStart(ctx context.Context, log abi.ConversationLog, o kernel.ModelObservation) error {
	record, err := modelStartAudit(o.RunID, o.RequestID, o.SystemPrompt, o.Model, o.Request)
	if err != nil {
		return err
	}
	return appendDiagnostic(ctx, log, []abi.SessionRecord{record})
}
func (*Plugin) ModelEnd(ctx context.Context, log abi.ConversationLog, o kernel.StreamObservation) error {
	return appendDiagnostic(ctx, log, streamRecords(o))
}
func (*Plugin) Hooks(ctx context.Context, log abi.ConversationLog, traces []kernel.HookTrace) error {
	records := make([]abi.SessionRecord, 0, len(traces))
	for _, trace := range traces {
		data, err := json.Marshal(trace)
		if err != nil {
			return err
		}
		records = append(records, abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.kernel.audit", Type: "middleware_hook_batch", SchemaVersion: 1, Payload: data})
	}
	return appendDiagnostic(ctx, log, records)
}
func appendDiagnostic(ctx context.Context, log abi.ConversationLog, records []abi.SessionRecord) error {
	if len(records) == 0 {
		return nil
	}
	_, err := log.AppendRecords(ctx, records)
	if err != nil {
		return &abi.SessionError{Operation: "append optional telemetry", SessionID: log.ID(), Err: err}
	}
	return nil
}

var _ kernel.DiagnosticObserver = (*Plugin)(nil)
