// INPUT: context, database/sql, encoding/json, errors, time, kernel/abi.
// OUTPUT: storage.ChildCreation, Repository.CreateChild/Parent and Session.SameAuthority.
// POS: [platform / durable storage] Actual child session and causal parent link commit atomically before child execution, without granting history reads.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// SameAuthority compares opaque native capability ownership, not merely a
// caller-controlled session ID. Reopened handles to this same authority match.
func (s *Session) SameAuthority(other abi.ConversationLog) bool {
	actual, ok := other.(*Session)
	return ok && actual != nil && actual.repository == s.repository && actual.owner == s.owner && actual.id == s.id
}

func (r *Repository) CreateChild(ctx context.Context, owner, parentID, childID, runID, callID, scopeID string) (storage.ChildCreation, error) {
	if !validIdentity(owner) || !validIdentity(parentID) || !validIdentity(childID) || !validIdentity(scopeID) || parentID == childID || runID == "" || callID == "" || len(runID) > 256 || len(callID) > 256 {
		return storage.ChildCreation{}, errors.New("explicit bounded actual child and parent invocation identities required")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return storage.ChildCreation{}, storage.ErrClosed
	}
	tx, err := r.db.BeginTx(ctx, nil)
	if err != nil {
		return storage.ChildCreation{}, err
	}
	defer tx.Rollback()
	if err = authorize(ctx, tx, owner, parentID); err != nil {
		return storage.ChildCreation{}, err
	}
	var intentID string
	var intentSequence uint64
	if err = tx.QueryRowContext(ctx, "SELECT id,seq FROM records WHERE session_id=? AND kind='tool_intent' AND run_id=? AND call_id=?", parentID, runID, callID).Scan(&intentID, &intentSequence); err != nil {
		if errors.Is(err, sql.ErrNoRows) {
			return storage.ChildCreation{}, errors.New("parent spawn tool intent is not committed")
		}
		return storage.ChildCreation{}, err
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO sessions(id,owner,parent_id,created_ns) VALUES(?,?,?,?)", childID, owner, parentID, time.Now().UTC().UnixNano()); err != nil {
		return storage.ChildCreation{}, err
	}
	payload, _ := json.Marshal(map[string]string{"parent_session_id": parentID, "child_session_id": childID, "parent_run_id": runID, "spawn_tool_call_id": callID, "child_scope_id": scopeID, "execution": "not_started", "source_notice": "Causal relationship only; this link does not grant parent/child history access and does not prove successful child execution"})
	records, err := appendTx(ctx, tx, parentID, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.agent", Type: "child_session_linked", SchemaVersion: 1, Parent: &abi.RecordReference{SessionID: parentID, RecordID: intentID, Sequence: intentSequence}, Payload: payload}})
	if err != nil {
		return storage.ChildCreation{}, err
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO child_links(child_id,parent_id,link_seq,link_id) VALUES(?,?,?,?)", childID, parentID, records[0].Sequence, records[0].ID); err != nil {
		return storage.ChildCreation{}, err
	}
	if err = tx.Commit(); err != nil {
		return storage.ChildCreation{}, err
	}
	return storage.ChildCreation{Session: &Session{repository: r, owner: owner, id: childID}, Link: abi.RecordReference{SessionID: parentID, RecordID: records[0].ID, Sequence: records[0].Sequence}}, nil
}

// Parent only reads ownership-bound metadata. It never restores a runtime or
// implies authorization to read either participant's conversation.
func (r *Repository) Parent(ctx context.Context, owner, id string) (string, error) {
	if !validIdentity(owner) || !validIdentity(id) {
		return "", storage.ErrNotFound
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return "", storage.ErrClosed
	}
	var parent string
	err := r.db.QueryRowContext(ctx, "SELECT coalesce(parent_id,'') FROM sessions WHERE owner=? AND id=?", owner, id).Scan(&parent)
	if errors.Is(err, sql.ErrNoRows) {
		return "", storage.ErrNotFound
	}
	return parent, err
}
