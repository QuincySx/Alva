// INPUT: Existing companion round snapshots, revision-bound progress and deterministic authored-message receipts.
// OUTPUT: Atomic finite-round cursor updates and idempotent publication without replaying a member Turn.
// POS: [L7 Hub storage] Waiting and interrupted rounds retain their original member/session/Turn authority until explicit resolution.
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
)

func companionRound(ctx context.Context, q querier, owner, id string) (hub.CompanionRound, error) {
	r, err := read[hub.CompanionRound](ctx, q, `SELECT data FROM hub_companion_rounds WHERE owner=? AND id=?`, owner, id)
	if err != nil {
		return r, err
	}
	if r.ID != id || r.OwnerID != owner || r.Revision == 0 || r.NextMember < 0 || r.NextMember > len(r.Members) {
		return hub.CompanionRound{}, errors.New("companion round progress is corrupt")
	}
	return r, nil
}
func (s *Store) CompanionRound(ctx context.Context, owner, id string) (out hub.CompanionRound, err error) {
	defer func() { err = normalizeStoreError(err) }()
	return companionRound(ctx, s.db, owner, id)
}
func writeCompanionRound(ctx context.Context, tx *sql.Tx, r hub.CompanionRound) error {
	b, err := body(r)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_companion_rounds SET status=?,data=? WHERE owner=? AND id=?`, r.Status, b, r.OwnerID, r.ID)
	return err
}
func sameCompanionRound(a, b hub.CompanionRound) bool {
	return a.ID == b.ID && a.OwnerID == b.OwnerID && a.GroupID == b.GroupID && a.GroupRevision == b.GroupRevision && a.Prompt == b.Prompt && a.CreatedAt == b.CreatedAt && reflect.DeepEqual(a.Members, b.Members)
}
func (s *Store) SaveCompanionRoundProgress(ctx context.Context, owner string, next hub.CompanionRound, expected uint64) (out hub.CompanionRound, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		old, e := companionRound(ctx, tx, owner, next.ID)
		if e != nil {
			return e
		}
		if old.Revision != expected || !sameCompanionRound(old, next) || next.NextMember != old.NextMember {
			return hub.ErrCompanionConflict
		}
		if old.Status == "completed" || old.Status == "cancelled" || old.Status == "failed" {
			return hub.ErrCompanionConflict
		}
		switch next.Status {
		case "running", "waiting", "interrupted", "unknown":
		default:
			return errors.New("invalid live round progress")
		}
		switch next.Stage {
		case "ready", "admitting", "executing", "waiting", "publishing", "unknown":
		default:
			return errors.New("invalid round stage")
		}
		next.OwnerID = owner
		next.Revision = expected + 1
		next.EndedAt = 0
		if e = writeCompanionRound(ctx, tx, next); e != nil {
			return e
		}
		out = next
		return nil
	})
	return out, err
}
func (s *Store) PublishCompanionReply(ctx context.Context, owner, id string, expected uint64, message hub.CompanionMessage) (out hub.CompanionRound, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		r, e := companionRound(ctx, tx, owner, id)
		if e != nil {
			return e
		}
		if r.Revision != expected || r.Status != "running" || r.NextMember >= len(r.Members) || r.Stage != "publishing" {
			return hub.ErrCompanionConflict
		}
		c := r.Members[r.NextMember]
		if message.ID != r.ID+"-"+c.CompanionID || message.RoundID != r.ID || message.CompanionID != c.CompanionID || message.SessionID != c.SessionID || message.TurnID != r.ActiveTurnID || message.GroupID != r.GroupID || message.GroupRevision != r.GroupRevision {
			return hub.ErrCompanionConflict
		}
		if e = appendCompanionMessage(ctx, tx, owner, message); e != nil {
			return e
		}
		r.NextMember++
		r.Revision++
		r.Stage = "ready"
		r.ActiveTurnID = ""
		r.ActiveBranchID = ""
		r.BeforeTurnID = ""
		r.PendingPromptSHA256 = ""
		if e = writeCompanionRound(ctx, tx, r); e != nil {
			return e
		}
		out = r
		return nil
	})
	return out, err
}
