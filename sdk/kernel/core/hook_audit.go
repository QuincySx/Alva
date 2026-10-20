// INPUT: context, errors, sync/time and kernel/abi
// OUTPUT: Typed generic stage-wide middleware observations for an optional observer
// POS: [L2 kernel runtime] Measures identity/order/inclusive time without owning diagnostic schemas or persistence policy.
package core

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type HookObservation struct {
	MiddlewareName string `json:"middleware_name"`
	LayerIndex     int    `json:"layer_index"`
	DurationMS     int64  `json:"duration_ms"`
	Outcome        string `json:"outcome"`
	ErrorSummary   string `json:"error_summary,omitempty"`
	Timing         string `json:"timing"`
}
type hookBatch struct {
	mu         sync.Mutex
	RunID      string            `json:"run_id"`
	Hook       string            `json:"hook"`
	ToolCallID string            `json:"tool_call_id,omitempty"`
	Calls      []HookObservation `json:"calls"`
}

func newHookBatch(runID, hook, callID string) *hookBatch {
	return &hookBatch{RunID: runID, Hook: hook, ToolCallID: callID}
}
func (b *hookBatch) observe(layer abi.Middleware, index int, started time.Time, err error) {
	if b == nil {
		return
	}
	entry := HookObservation{MiddlewareName: layer.Name(), LayerIndex: index, DurationMS: max(0, time.Since(started).Milliseconds()), Outcome: "ok", Timing: "inclusive"}
	if err != nil {
		entry.Outcome = "error"
		entry.ErrorSummary = "hook_failure"
		var session *abi.SessionError
		if errors.As(err, &session) {
			entry.ErrorSummary = "session_failure"
		} else if abi.OnlyCancellation(err) {
			entry.ErrorSummary = "cancelled"
		} else if onlyToolBlocked(err) {
			entry.ErrorSummary = "intentional_tool_veto"
		}
	}
	b.mu.Lock()
	b.Calls = append(b.Calls, entry)
	b.mu.Unlock()
}
func (b *hookBatch) trace() (HookTrace, bool) {
	if b == nil {
		return HookTrace{}, false
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if len(b.Calls) == 0 {
		return HookTrace{}, false
	}
	return HookTrace{RunID: b.RunID, Hook: b.Hook, ToolCallID: b.ToolCallID, Calls: append([]HookObservation(nil), b.Calls...)}, true
}
func commitHookBatches(ctx context.Context, session abi.ConversationLog, observer DiagnosticObserver, batches ...*hookBatch) {
	if observer == nil {
		return
	}
	cleanup, cancel := settlementContext(ctx)
	defer cancel()
	appendHookBatches(cleanup, session, observer, batches...)
}
func appendHookBatches(ctx context.Context, session abi.ConversationLog, observer DiagnosticObserver, batches ...*hookBatch) {
	if observer == nil {
		return
	}
	var traces []HookTrace
	for _, batch := range batches {
		if trace, ok := batch.trace(); ok {
			traces = append(traces, trace)
		}
	}
	if len(traces) == 0 {
		return
	}
	reportDiagnostic(ctx, observer, session, "middleware hooks", observer.Hooks(ctx, session, traces))
}

// invokeHookPhase only observes actual implemented hooks; missing optional Go
// interfaces aren't fabricated calls. A failed hook stops before-phases and
// after-phases just as before; OnRunEnd uses its explicit always-cleanup loop.
func invokeHookPhase(ctx context.Context, session abi.ConversationLog, observer DiagnosticObserver, runID, hook string, layers []abi.Middleware, reverse bool, invoke func(abi.Middleware) (bool, error)) error {
	batch := newHookBatch(runID, hook, "")
	var hookErr error
	for n := 0; n < len(layers); n++ {
		i := n
		if reverse {
			i = len(layers) - 1 - n
		}
		started := time.Now()
		called, err := invoke(layers[i])
		if called {
			batch.observe(layers[i], i, started, err)
		}
		if err != nil {
			hookErr = err
			break
		}
	}
	commitHookBatches(ctx, session, observer, batch)
	return hookErr
}

type toolHookAudit struct{ before, wrap, after *hookBatch }

func newToolHookAudit(runID, callID string) *toolHookAudit {
	return &toolHookAudit{newHookBatch(runID, "before_tool", callID), newHookBatch(runID, "wrap_tool", callID), newHookBatch(runID, "after_tool", callID)}
}
