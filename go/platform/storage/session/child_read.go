// INPUT: context, database/sql, encoding/json, errors, kernel/abi.
// OUTPUT: ChildLink, Repository.ResolveChildLink and ParentLink metadata-only typed ancestry evidence.
// POS: [platform / durable storage] Verifies one bounded committed causal link without restoring sessions or reading parent tool/conversation bodies.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

type ChildLink struct {
	ParentID string              `json:"parent_id"`
	ChildID  string              `json:"child_id"`
	Link     abi.RecordReference `json:"link"`
}

// ResolveChildLink proves only the exact direct relation. The product must
// separately bind its claimed root and limit the hop chain; this method never
// grants model access to any participant's text or interprets arbitrary URIs.
func (r *Repository) ResolveChildLink(ctx context.Context, owner, parentID string, sequence uint64) (ChildLink, error) {
	if !validIdentity(owner) || !validIdentity(parentID) || sequence == 0 {
		return ChildLink{}, ErrNotFound
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ChildLink{}, ErrClosed
	}
	return r.resolveChildLinkLocked(ctx, owner, parentID, sequence)
}
func (r *Repository) resolveChildLinkLocked(ctx context.Context, owner, parentID string, sequence uint64) (ChildLink, error) {
	if err := authorize(ctx, r.db, owner, parentID); err != nil {
		return ChildLink{}, err
	}
	var id, key, kind, namespace, recordType, parentJSON string
	var version uint32
	err := r.db.QueryRowContext(ctx, "SELECT id,artifact_id,kind,namespace,type,schema_version,coalesce(parent_json,'') FROM records WHERE session_id=? AND seq=?", parentID, sequence).Scan(&id, &key, &kind, &namespace, &recordType, &version, &parentJSON)
	if errors.Is(err, sql.ErrNoRows) {
		return ChildLink{}, ErrNotFound
	}
	if err != nil {
		return ChildLink{}, err
	}
	if kind != string(abi.RecordExtension) || namespace != "alva.agent" || recordType != "child_session_linked" || version != 1 {
		return ChildLink{}, errors.New("target is not a supported typed child link")
	}
	var artifactOwner, artifactKind string
	var artifactBytes int64
	if err = r.db.QueryRowContext(ctx, "SELECT session_id,kind,bytes FROM artifacts WHERE id=?", key).Scan(&artifactOwner, &artifactKind, &artifactBytes); err != nil {
		return ChildLink{}, err
	}
	if artifactOwner != parentID || artifactKind != "typed-record-json" || artifactBytes > 16<<10 {
		return ChildLink{}, errors.New("child link artifact is outside its owned bounded record authority")
	}
	data, err := r.readFullLocked(ctx, key, 16<<10)
	if err != nil {
		return ChildLink{}, err
	}
	var record abi.SessionRecord
	if err = json.Unmarshal(data, &record); err != nil {
		return ChildLink{}, err
	}
	if err = record.Validate(); err != nil {
		return ChildLink{}, err
	}
	if record.ID != id || record.Sequence != sequence || record.Kind != abi.RecordExtension || record.Namespace != namespace || record.Type != recordType || record.SchemaVersion != version || record.Parent == nil {
		return ChildLink{}, errors.New("child link record/index identity mismatch")
	}
	var indexedParent abi.RecordReference
	if json.Unmarshal([]byte(parentJSON), &indexedParent) != nil || indexedParent != *record.Parent || indexedParent.SessionID != parentID || indexedParent.Sequence == 0 || indexedParent.Sequence >= sequence {
		return ChildLink{}, errors.New("child causal parent index mismatch")
	}
	var payload struct {
		ParentID string `json:"parent_session_id"`
		ChildID  string `json:"child_session_id"`
		RunID    string `json:"parent_run_id"`
		CallID   string `json:"spawn_tool_call_id"`
		ScopeID  string `json:"child_scope_id"`
	}
	if err = json.Unmarshal(record.Payload, &payload); err != nil {
		return ChildLink{}, err
	}
	if payload.ParentID != parentID || !validIdentity(payload.ChildID) || payload.ChildID == parentID || !validIdentity(payload.ScopeID) || payload.RunID == "" || payload.CallID == "" {
		return ChildLink{}, errors.New("invalid typed child-link payload")
	}
	var intentID, runID, callID, intentKind, intentNamespace, intentType string
	var intentVersion uint32
	err = r.db.QueryRowContext(ctx, "SELECT id,kind,namespace,type,schema_version,coalesce(run_id,''),coalesce(call_id,'') FROM records WHERE session_id=? AND seq=?", parentID, indexedParent.Sequence).Scan(&intentID, &intentKind, &intentNamespace, &intentType, &intentVersion, &runID, &callID)
	if err != nil {
		return ChildLink{}, err
	}
	if intentID != indexedParent.RecordID || intentKind != string(abi.RecordToolIntent) || intentNamespace != "alva.kernel" || intentType != "tool_intent" || intentVersion != 1 || runID != payload.RunID || callID != payload.CallID {
		return ChildLink{}, errors.New("child link does not match its actual committed parent tool intent")
	}
	var actualParent string
	err = r.db.QueryRowContext(ctx, "SELECT coalesce(parent_id,'') FROM sessions WHERE owner=? AND id=?", owner, payload.ChildID).Scan(&actualParent)
	if errors.Is(err, sql.ErrNoRows) {
		return ChildLink{}, ErrNotFound
	}
	if err != nil {
		return ChildLink{}, err
	}
	if actualParent != parentID {
		return ChildLink{}, errors.New("child metadata differs from typed direct-parent link")
	}
	return ChildLink{ParentID: parentID, ChildID: payload.ChildID, Link: abi.RecordReference{SessionID: parentID, RecordID: id, Sequence: sequence}}, nil
}

// ParentLink uses only a small auxiliary index and re-verifies the original
// committed link. Metadata-only Create(parentID) and older unindexed facts do
// not magically become typed ancestry grants, and no history scan is used.
func (r *Repository) ParentLink(ctx context.Context, owner, childID string) (ChildLink, error) {
	if !validIdentity(owner) || !validIdentity(childID) {
		return ChildLink{}, ErrNotFound
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ChildLink{}, ErrClosed
	}
	if err := authorize(ctx, r.db, owner, childID); err != nil {
		return ChildLink{}, err
	}
	var parentID, id string
	var sequence uint64
	err := r.db.QueryRowContext(ctx, "SELECT parent_id,link_seq,link_id FROM child_links WHERE child_id=?", childID).Scan(&parentID, &sequence, &id)
	if errors.Is(err, sql.ErrNoRows) {
		return ChildLink{}, ErrNotFound
	}
	if err != nil {
		return ChildLink{}, err
	}
	link, err := r.resolveChildLinkLocked(ctx, owner, parentID, sequence)
	if err != nil {
		return ChildLink{}, err
	}
	if link.ChildID != childID || link.Link.RecordID != id {
		return ChildLink{}, errors.New("child reverse index differs from authoritative typed link")
	}
	return link, nil
}
