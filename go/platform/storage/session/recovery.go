// INPUT: context, database/sql, encoding/json, kernel/abi, llm/wire.
// OUTPUT: RecoveryReport and explicit interrupted-intent settlement without execution.
// POS: [platform / durable storage] Native crash recovery preserves unknown side effects instead of replaying or inventing success.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type RecoveryReport struct{ ModelIntents, ToolIntents, UnknownToolExecutions int }
type pendingIntent struct {
	id                       string
	sequence                 uint64
	requestID, runID, callID string
}

func (r *Repository) Recover(ctx context.Context, owner, id string) (RecoveryReport, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return RecoveryReport{}, ErrClosed
	}
	if r.active[id] {
		return RecoveryReport{}, ErrActive
	}
	return r.recoverLocked(ctx, owner, id)
}

func (r *Repository) recoverLocked(ctx context.Context, owner, id string) (RecoveryReport, error) {
	tx, err := r.db.BeginTx(ctx, nil)
	if err != nil {
		return RecoveryReport{}, err
	}
	defer tx.Rollback()
	if err = authorize(ctx, tx, owner, id); err != nil {
		return RecoveryReport{}, err
	}
	models, err := pending(ctx, tx, id, "model_intent", "model_outcome", true)
	if err != nil {
		return RecoveryReport{}, err
	}
	tools, err := pending(ctx, tx, id, "tool_intent", "tool_outcome", false)
	if err != nil {
		return RecoveryReport{}, err
	}
	var batch []abi.SessionRecord
	for _, intent := range models {
		batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelOutcome, Parent: &abi.RecordReference{SessionID: id, RecordID: intent.id, Sequence: intent.sequence}, ModelOutcome: &abi.ModelOutcome{RequestID: intent.requestID, Status: abi.OutcomeCancelled, Error: "Interrupted before a model outcome was committed; request was not replayed"}}))
	}
	if len(batch) > 0 {
		if _, err = appendTx(ctx, tx, id, batch); err != nil {
			return RecoveryReport{}, err
		}
	}
	for start := 0; start < len(tools); start += 64 {
		end := start + 64
		if end > len(tools) {
			end = len(tools)
		}
		batch = nil
		for _, intent := range tools[start:end] {
			evidence, _ := json.Marshal(map[string]string{"run_id": intent.runID, "call_id": intent.callID, "execution": "unknown", "reason": "Host execution was interrupted before a committed outcome; no side effect was replayed"})
			batch = append(batch, abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.native.recovery", Type: "execution_interrupted", SchemaVersion: 1, Parent: &abi.RecordReference{SessionID: id, RecordID: intent.id, Sequence: intent.sequence}, Payload: evidence})
			result := wire.ToolResult{CallID: intent.callID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Execution outcome unknown after host interruption. The tool may have run; it was not replayed."}}}
			message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
			batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: intent.runID, CallID: intent.callID, Status: abi.OutcomeCancelled, Result: result, Error: "Execution outcome unknown after interruption; not replayed"}}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}))
		}
		if _, err = appendTx(ctx, tx, id, batch); err != nil {
			return RecoveryReport{}, err
		}
	}
	if err = tx.Commit(); err != nil {
		return RecoveryReport{}, err
	}
	return RecoveryReport{ModelIntents: len(models), ToolIntents: len(tools), UnknownToolExecutions: len(tools)}, nil
}

func pending(ctx context.Context, tx *sql.Tx, id, intentKind, outcomeKind string, model bool) ([]pendingIntent, error) {
	condition := "o.run_id=i.run_id AND o.call_id=i.call_id"
	if model {
		condition = "o.request_id=i.request_id"
	}
	query := "SELECT i.id,i.seq,coalesce(i.request_id,''),coalesce(i.run_id,''),coalesce(i.call_id,'') FROM records i WHERE i.session_id=? AND i.kind=? AND NOT EXISTS(SELECT 1 FROM records o WHERE o.session_id=i.session_id AND o.kind=? AND " + condition + ") ORDER BY i.seq LIMIT 129"
	rows, err := tx.QueryContext(ctx, query, id, intentKind, outcomeKind)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var intents []pendingIntent
	for rows.Next() {
		var intent pendingIntent
		if err = rows.Scan(&intent.id, &intent.sequence, &intent.requestID, &intent.runID, &intent.callID); err != nil {
			return nil, err
		}
		intents = append(intents, intent)
	}
	if err = rows.Err(); err != nil {
		return nil, err
	}
	if len(intents) > 128 {
		return nil, errors.New("interrupted-intent recovery budget exceeded; no execution started")
	}
	return intents, nil
}
