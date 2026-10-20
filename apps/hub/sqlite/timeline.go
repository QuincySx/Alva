// INPUT: Product transition values and the same SQLite transaction that owns routing
// OUTPUT: Append-only typed product timeline entries and durable bounded handoff receipts
// POS: [L7 Hub product] Product shared clock, not a duplicate native execution log
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

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
