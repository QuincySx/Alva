// INPUT: context, encoding/json, errors, sync/time, kernel/abi
// OUTPUT: Private typed stage-wide middleware audit batches and failure-safe durable append
// POS: [L2 kernel runtime] Generic loop observation records identity/order/inclusive time, never prompt/result bodies or private error text.
package core

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

type hookObservation struct {
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
	Calls      []hookObservation `json:"calls"`
}

func newHookBatch(runID, hook, callID string) *hookBatch {
	return &hookBatch{RunID: runID, Hook: hook, ToolCallID: callID}
}
func (b *hookBatch) observe(layer abi.Middleware, index int, started time.Time, err error) {
	if b == nil {
		return
	}
	entry := hookObservation{MiddlewareName: layer.Name(), LayerIndex: index, DurationMS: max(0, time.Since(started).Milliseconds()), Outcome: "ok", Timing: "inclusive"}
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
func (b *hookBatch) record() (abi.SessionRecord, bool) {
	if b == nil {
		return abi.SessionRecord{}, false
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if len(b.Calls) == 0 {
		return abi.SessionRecord{}, false
	}
	data, _ := json.Marshal(struct {
		RunID      string            `json:"run_id"`
		Hook       string            `json:"hook"`
		ToolCallID string            `json:"tool_call_id,omitempty"`
		Calls      []hookObservation `json:"calls"`
	}{b.RunID, b.Hook, b.ToolCallID, b.Calls})
	return abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.kernel.audit", Type: "middleware_hook_batch", SchemaVersion: 1, Payload: data}, true
}
func commitHookBatches(ctx context.Context, session abi.ConversationLog, batches ...*hookBatch) error {
	cleanup, cancel := settlementContext(ctx)
	defer cancel()
	return appendHookBatches(cleanup, session, batches...)
}

// appendHookBatches reuses an already-owned cleanup deadline. Nested tool/end
// settlement must not silently reset the outer five-second cleanup window.
func appendHookBatches(ctx context.Context, session abi.ConversationLog, batches ...*hookBatch) error {
	var records []abi.SessionRecord
	for _, batch := range batches {
		if record, ok := batch.record(); ok {
			records = append(records, record)
		}
	}
	if len(records) == 0 {
		return nil
	}
	_, err := appendRecords(ctx, session, records)
	return err
}

// invokeHookPhase only observes actual implemented hooks; missing optional Go
// interfaces aren't fabricated calls. A failed hook stops before-phases and
// after-phases just as before; OnRunEnd uses its explicit always-cleanup loop.
func invokeHookPhase(ctx context.Context, session abi.ConversationLog, runID, hook string, layers []abi.Middleware, reverse bool, invoke func(abi.Middleware) (bool, error)) error {
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
	return errors.Join(hookErr, commitHookBatches(ctx, session, batch))
}

type toolHookAudit struct{ before, wrap, after *hookBatch }

func newToolHookAudit(runID, callID string) *toolHookAudit {
	return &toolHookAudit{newHookBatch(runID, "before_tool", callID), newHookBatch(runID, "wrap_tool", callID), newHookBatch(runID, "after_tool", callID)}
}
