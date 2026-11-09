// INPUT: context, database/sql, encoding/json, hub PendingInput bounds
// OUTPUT: hub_pending_inputs schema (v10) and the durable queued/interjected input operations of hub.PendingStore
// POS: [L7 Hub product storage] Per-conversation input queue: at most 8 entries of 16 KiB, persisted so a crash never drops or resends a message silently
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

// pendingInputsSchema creates the queue table in the opening transaction.
func pendingInputsSchema(ctx context.Context, tx *sql.Tx) error {
	for _, statement := range []string{
		`CREATE TABLE IF NOT EXISTS hub_pending_inputs(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),pos INTEGER NOT NULL,state TEXT NOT NULL,data BLOB NOT NULL)`,
		`CREATE INDEX IF NOT EXISTS hub_pending_session ON hub_pending_inputs(session,pos)`,
	} {
		if _, err := tx.ExecContext(ctx, statement); err != nil {
			return err
		}
	}
	return nil
}

func readPending(ctx context.Context, tx *sql.Tx, session, id string) (hub.PendingInput, error) {
	var item hub.PendingInput
	value, err := read[hub.PendingInput](ctx, tx, `SELECT data FROM hub_pending_inputs WHERE id=? AND session=?`, id, session)
	if errors.Is(err, sql.ErrNoRows) {
		return item, hub.ErrPendingGone
	}
	return value, err
}

func writePending(ctx context.Context, tx *sql.Tx, item hub.PendingInput) error {
	data, err := body(item)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_pending_inputs SET state=?,data=? WHERE id=? AND session=?`, item.State, data, item.ID, item.SessionID)
	return err
}

// QueueInput appends an input at the end of the queue in the given state.
func (s *Store) QueueInput(ctx context.Context, id, text string, state hub.PendingState) (item hub.PendingInput, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = hub.ValidPendingText(text); err != nil {
		return item, err
	}
	if state != hub.PendingQueued && state != hub.PendingInterjecting {
		return item, errors.New("a new input is queued or interjecting")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		_, readErr := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if readErr != nil {
			return readErr
		}
		var count int
		var last int64
		if readErr = tx.QueryRowContext(ctx, `SELECT count(*),coalesce(max(pos),0) FROM hub_pending_inputs WHERE session=?`, id).Scan(&count, &last); readErr != nil {
			return readErr
		}
		if count >= hub.MaxPendingInputs {
			return hub.ErrPendingFull
		}
		newID, idErr := hub.NewID("input-")
		if idErr != nil {
			return idErr
		}
		item = hub.PendingInput{ID: newID, SessionID: id, Text: text, State: state, CreatedAt: now()}
		data, bodyErr := body(item)
		if bodyErr != nil {
			return bodyErr
		}
		_, insertErr := tx.ExecContext(ctx, `INSERT INTO hub_pending_inputs(id,session,pos,state,data) VALUES(?,?,?,?,?)`, newID, id, last+1, state, data)
		return insertErr
	})
	return item, err
}

func (s *Store) PendingInputs(ctx context.Context, id string) (items []hub.PendingInput, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if _, err = read[hub.Session](ctx, s.list, liveSessionQuery, id); err != nil {
		return nil, err
	}
	rows, err := s.list.QueryContext(ctx, `SELECT pos,data FROM hub_pending_inputs WHERE session=? ORDER BY pos LIMIT 9`, id)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var pos int64
		var data []byte
		var item hub.PendingInput
		if err = rows.Scan(&pos, &data); err != nil {
			return nil, err
		}
		if len(data) > 64<<10 || json.Unmarshal(data, &item) != nil {
			return nil, errors.New("queued input is corrupt")
		}
		item.Position = pos
		items = append(items, item)
	}
	return items, rows.Err()
}

// EditPendingInput replaces a queued input's text; one already interjecting
// or uncertain is not editable (it may already be with the model).
func (s *Store) EditPendingInput(ctx context.Context, id, inputID, text string) (item hub.PendingInput, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = hub.ValidPendingText(text); err != nil {
		return item, err
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := read[hub.Session](ctx, tx, liveSessionQuery, id); e != nil {
			return e
		}
		current, e := readPending(ctx, tx, id, inputID)
		if e != nil {
			return e
		}
		if current.State != hub.PendingQueued {
			return hub.ErrPendingGone
		}
		current.Text, current.Error = text, ""
		item = current
		return writePending(ctx, tx, current)
	})
	return item, err
}

// CancelPendingInput deletes a queued or uncertain input.
func (s *Store) CancelPendingInput(ctx context.Context, id, inputID string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := readPending(ctx, tx, id, inputID)
		if e != nil {
			return e
		}
		if current.State == hub.PendingInterjecting {
			return hub.ErrPendingGone // already with the engine: it cannot be retracted
		}
		_, e = tx.ExecContext(ctx, `DELETE FROM hub_pending_inputs WHERE id=? AND session=?`, inputID, id)
		return e
	})
}

// RequeuePendingInput moves an interjecting input to the queue head.
func (s *Store) RequeuePendingInput(ctx context.Context, id, inputID, reason string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		return requeueInterjection(ctx, tx, id, inputID, reason)
	})
}

func requeueInterjection(ctx context.Context, tx *sql.Tx, session, inputID, reason string) error {
	current, err := readPending(ctx, tx, session, inputID)
	if err != nil {
		return err
	}
	if current.State != hub.PendingInterjecting {
		return nil
	}
	var first int64
	if err = tx.QueryRowContext(ctx, `SELECT coalesce(min(pos),1) FROM hub_pending_inputs WHERE session=?`, session).Scan(&first); err != nil {
		return err
	}
	current.State, current.Error = hub.PendingQueued, reason
	data, err := body(current)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_pending_inputs SET state=?,pos=?,data=? WHERE id=? AND session=?`, current.State, first-1, data, inputID, session)
	return err
}

// requeueAllInterjections runs in the transaction that ends a Turn: every
// interjection without a delivery receipt goes back to the queue head, in its
// original order.
func requeueAllInterjections(ctx context.Context, tx *sql.Tx, session string) error {
	rows, err := tx.QueryContext(ctx, `SELECT id FROM hub_pending_inputs WHERE session=? AND state=? ORDER BY pos DESC`, session, hub.PendingInterjecting)
	if err != nil {
		return err
	}
	var ids []string
	for rows.Next() {
		var id string
		if err = rows.Scan(&id); err != nil {
			break
		}
		ids = append(ids, id)
	}
	if err = errors.Join(err, rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, id := range ids {
		if err = requeueInterjection(ctx, tx, session, id, "插话未在本轮送达，已转为排队"); err != nil {
			return err
		}
	}
	return nil
}

// NotePendingError records why a dispatch did not start.
func (s *Store) NotePendingError(ctx context.Context, id, inputID, reason string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	if len(reason) > 512 {
		reason = reason[:512]
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := readPending(ctx, tx, id, inputID)
		if e != nil {
			return e
		}
		current.Error = reason
		return writePending(ctx, tx, current)
	})
}

// recoverPending marks inputs that were with an engine when the application
// exited: delivery is unknown, so they are never resent automatically.
func recoverPending(ctx context.Context, tx *sql.Tx) error {
	rows, err := tx.QueryContext(ctx, `SELECT session,id FROM hub_pending_inputs WHERE state=?`, hub.PendingInterjecting)
	if err != nil {
		return err
	}
	type key struct{ session, id string }
	var keys []key
	for rows.Next() {
		var k key
		if err = rows.Scan(&k.session, &k.id); err != nil {
			break
		}
		keys = append(keys, k)
	}
	if err = errors.Join(err, rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, k := range keys {
		item, e := readPending(ctx, tx, k.session, k.id)
		if e != nil {
			return e
		}
		item.State, item.Error = hub.PendingUncertain, "应用退出时此消息可能已送达模型，不会自动重发"
		if e = writePending(ctx, tx, item); e != nil {
			return e
		}
	}
	return nil
}

// deliverPending is the delivery receipt: the input reached the model, so it
// leaves the queue in the transaction that records the receipt event.
func deliverPending(ctx context.Context, tx *sql.Tx, session, inputID string) error {
	_, err := tx.ExecContext(ctx, `DELETE FROM hub_pending_inputs WHERE id=? AND session=?`, inputID, session)
	return err
}
