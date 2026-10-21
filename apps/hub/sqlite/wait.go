// INPUT: Explicit product wait controls and the existing Hub SQLite transaction authority.
// OUTPUT: WaitStore implementation with same-Turn/new-Attempt resume and reviewed abandonment.
// POS: [L7 storage] Persists product routing only; native checkpoint bodies remain in supplied session storage.
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *Store) SetDurableWaits(ctx context.Context, id string, enabled bool) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		d, e := detail(ctx, tx, id)
		if e != nil {
			return e
		}
		if hub.BlocksInput(d.Session.Status) {
			return errors.New("finish or abandon the active Turn before changing waits")
		}
		settings, e := settings(ctx, tx, id)
		if e != nil {
			return e
		}
		settings.DurableWaits = enabled
		return saveSettings(ctx, tx, settings)
	})
}
func currentTurn(ctx context.Context, tx *sql.Tx, id string) (hub.Turn, error) {
	return read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE session=? ORDER BY seq DESC LIMIT 1`, id)
}
func branchTurn(ctx context.Context, tx *sql.Tx, id, branch string) (hub.Turn, error) {
	return read[hub.Turn](ctx, tx, `SELECT t.data FROM hub_turns t JOIN hub_attempts a ON a.turn=t.id WHERE t.session=? AND a.session=? AND json_extract(a.data,'$.branch_id')=? ORDER BY t.seq DESC LIMIT 1`, id, id, branch)
}
func (s *Store) BranchTurn(ctx context.Context, id, branch string) (v hub.Turn, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if _, e := detail(ctx, tx, id); e != nil {
			return e
		}
		var e error
		v, e = branchTurn(ctx, tx, id, branch)
		return e
	})
	return
}
func (s *Store) BeginWaitResume(ctx context.Context, id, branchID, turnID string, revision uint64) (claim hub.Claim, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if revision == 0 {
		return claim, errors.New("reviewed checkpoint revision required")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		d, e := detail(ctx, tx, id)
		if e != nil {
			return e
		}
		t, e := currentTurn(ctx, tx, id)
		if e != nil {
			return e
		}
		if d.Session.Status != engine.Waiting || d.Session.ActiveBranchID != branchID || t.ID != turnID || t.Status != engine.Waiting {
			return errors.New("waiting Turn/branch changed before continuation admission")
		}
		previous, e := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=? AND session=?`, t.ActiveAttemptID, id)
		if e != nil {
			return e
		}
		b, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, branchID, id)
		if e != nil {
			return e
		}
		if previous.Status != engine.Waiting || previous.Wait == nil || previous.Wait.Validate() != nil || revision < previous.Wait.Revision || previous.BranchID != b.ID || previous.NativeSessionID == "" || previous.NativeSessionID != b.NativeSessionID || hub.RouteKey(previous.Target) != hub.RouteKey(b.Target) {
			return errors.New("waiting attempt no longer matches its actual native branch")
		}
		settings, e := settings(ctx, tx, id)
		if e != nil {
			return e
		}
		if !settings.DurableWaits {
			return errors.New("durable waits are disabled for this session")
		}
		t.Status, t.EndedAt = hub.Running, 0
		a, e := beginAttempt(ctx, tx, d.Session, b, &t)
		if e != nil {
			return e
		}
		d.Session.Status, d.Session.UpdatedAt = hub.Running, now()
		if e = writeSession(ctx, tx, d.Session); e != nil {
			return e
		}
		claim = hub.Claim{Session: d.Session, Branch: b, Turn: t, Attempt: a, WaitRevision: revision}
		return timeline(ctx, tx, id, t.ID, a.ID, hub.TimelineEvent{Kind: "wait_resumed", Text: "Explicit same-Turn continuation from a reviewed native checkpoint; original input/tools are not replayed"})
	})
	return
}
func (s *Store) AbandonWaitingTurn(ctx context.Context, id, branchID, turnID string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		d, e := detail(ctx, tx, id)
		if e != nil {
			return e
		}
		t, e := branchTurn(ctx, tx, id, branchID)
		if e != nil {
			return e
		}
		if d.Session.ActiveBranchID != branchID || t.ID != turnID || d.Session.Status == hub.Running {
			return errors.New("abandonment target is no longer the reviewed idle Turn")
		}
		if t.Status != engine.Waiting {
			return timeline(ctx, tx, id, t.ID, t.ActiveAttemptID, hub.TimelineEvent{Kind: "wait_abandoned", Text: "Reviewed native checkpoint reset after interrupted execution; prior product outcome and effects are retained, external work was not cancelled"})
		} // Reconcile only the product waiting edge after a native reset.
		t.Status, t.EndedAt = engine.Interrupted, now()
		d.Session.Status, d.Session.UpdatedAt = engine.Interrupted, now()
		if e = writeTurn(ctx, tx, t); e != nil {
			return e
		}
		if e = writeSession(ctx, tx, d.Session); e != nil {
			return e
		}
		if e = settleWorkerTask(ctx, tx, t); e != nil {
			return e
		}
		if e = settleAutomation(ctx, tx, t); e != nil {
			return e
		}
		return timeline(ctx, tx, id, t.ID, t.ActiveAttemptID, hub.TimelineEvent{Kind: "wait_abandoned", Text: "Reviewed native checkpoint reset; effects were not undone and external work was not cancelled"})
	})
}

var _ hub.WaitStore = (*Store)(nil)
