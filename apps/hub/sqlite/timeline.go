// INPUT: Product transition values and the same SQLite transaction that owns routing
// OUTPUT: Append-only typed product timeline entries and durable bounded handoff receipts
// POS: [L7 Hub product] Product shared clock, not a duplicate native execution log
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// RecordApprovalSettled appends one settled-approval line to the attempt. The
// call's ToolDisplay is taken from that call's own committed tool_start entry,
// which is already redacted and bounded; a missing call (a child session's)
// simply leaves the line to show the declared effect. More than
// hub.MaxApprovalSettledPerAttempt lines per attempt are not stored.
func (s *Store) RecordApprovalSettled(ctx context.Context, id, attemptID string, settled hub.ApprovalSettled) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		attempt, e := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=? AND session=?`, attemptID, id)
		if e != nil {
			return e
		}
		// The count lives in the attempt record, so no event scan is needed.
		if attempt.ApprovalLines >= hub.MaxApprovalSettledPerAttempt {
			return nil
		}
		settled.Tool = nil
		if settled.CallID != "" {
			var data []byte
			e = tx.QueryRowContext(ctx, `SELECT data FROM hub_events WHERE attempt=? AND json_extract(data,'$.event.kind')=? AND json_extract(data,'$.event.id')=? ORDER BY seq DESC LIMIT 1`, attempt.ID, string(engine.ToolStart), settled.CallID).Scan(&data)
			if e != nil && !errors.Is(e, sql.ErrNoRows) {
				return e
			}
			if e == nil {
				var entry hub.HistoryEntry
				if len(data) <= 128<<10 && json.Unmarshal(data, &entry) == nil && entry.Event.Display != nil {
					settled.Tool = engine.BoundToolDisplay(*entry.Event.Display)
				}
			}
		}
		if e = timeline(ctx, tx, id, attempt.TurnID, attempt.ID, hub.TimelineEvent{Kind: hub.TimelineApprovalSettled, Text: "Approval settled by the user", Approval: &settled}); e != nil {
			return e
		}
		attempt.ApprovalLines++
		return writeAttempt(ctx, tx, attempt)
	})
}

var _ hub.ApprovalSettledStore = (*Store)(nil)

func (s *Store) ReadHandoff(ctx context.Context, id, handoffID string) (providerResult0 *hub.Handoff, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var value hub.Handoff
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		value, err = read[hub.Handoff](ctx, tx, `SELECT data FROM hub_handoffs WHERE id=? AND session=?`, handoffID, id)
		if err != nil {
			return err
		}
		if value.ID != handoffID || value.SessionID != id {
			return errors.New("handoff ownership is corrupt")
		}
		_, err = hub.EncodeHandoff(&value)
		return err
	})
	if err != nil {
		return nil, err
	}
	return &value, nil
}

func timeline(ctx context.Context, tx *sql.Tx, id, turn, attempt string, event hub.TimelineEvent) error {
	data, err := body(hub.HistoryEntry{SessionID: id, TurnID: turn, AttemptID: attempt, Time: now(), Timeline: &event})
	if err != nil {
		return err
	}
	var turnValue, attemptValue any
	if turn != "" {
		turnValue = turn
	}
	if attempt != "" {
		attemptValue = attempt
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, id, turnValue, attemptValue, data)
	return err
}
func saveHandoff(ctx context.Context, tx *sql.Tx, id, turn, attempt string, value *hub.Handoff) error {
	if value == nil {
		return nil
	}
	data, err := body(value)
	if err != nil {
		return err
	}
	if _, err = tx.ExecContext(ctx, `INSERT INTO hub_handoffs VALUES(?,?,?)`, value.ID, id, data); err != nil {
		return err
	}
	return timeline(ctx, tx, id, turn, attempt, hub.TimelineEvent{Kind: "handoff", Text: value.Processing, FromBranchID: value.SourceBranchID, ToBranchID: value.TargetBranchID, HandoffID: value.ID})
}
