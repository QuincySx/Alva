// INPUT: context, database/sql, encoding/json, errors/utf8, kernel/abi and the shared record metadata scanner.
// OUTPUT: Owner-scoped LatestRecord metadata and atomic compare-and-append snapshots with optional typed record batches.
// POS: [platform / durable storage] Durable optional-capability state is restored from the same typed fact authority without reading unrelated history.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// LatestRecord reads only metadata for the exact requested typed stream.
// Its reference remains session-owned; it is not a cross-session grant.
func (r *Repository) LatestRecord(ctx context.Context, owner, id, namespace, kind string) (storage.RecordEntry, bool, error) {
	if namespace == "" || kind == "" || len(namespace) > 128 || len(kind) > 128 || !utf8.ValidString(namespace+kind) {
		return storage.RecordEntry{}, false, errors.New("bounded exact record namespace/type required")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return storage.RecordEntry{}, false, storage.ErrClosed
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return storage.RecordEntry{}, false, err
	}
	// LEFT JOIN deliberately preserves a corrupt record row: missing/foreign
	// artifact metadata must fail its scan, not masquerade as no snapshot.
	entry, err := scanRecordEntry(r.db.QueryRowContext(ctx, `SELECT `+recordColumns+` FROM records r LEFT JOIN artifacts a ON a.id=r.artifact_id AND a.session_id=r.session_id WHERE r.session_id=? AND r.namespace=? AND r.type=? ORDER BY r.seq DESC LIMIT 1`, id, namespace, kind))
	if errors.Is(err, sql.ErrNoRows) {
		return storage.RecordEntry{}, false, nil
	}
	if err != nil {
		return storage.RecordEntry{}, false, err
	}
	return entry, true, nil
}

// AppendExtensionSnapshot is optional native optimistic concurrency, not a new
// kernel requirement. The previous exact stream sequence and the typed append
// are checked in one transaction. Stale caches cannot overwrite newer facts.
func (s *Session) AppendExtensionSnapshot(ctx context.Context, namespace, kind string, expected uint64, payload json.RawMessage, additional ...abi.SessionRecord) (abi.SessionRecord, error) {
	r := s.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return abi.SessionRecord{}, storage.ErrClosed
	}
	tx, err := r.db.BeginTx(ctx, nil)
	if err != nil {
		return abi.SessionRecord{}, err
	}
	defer tx.Rollback()
	if err = authorize(ctx, tx, s.owner, s.id); err != nil {
		return abi.SessionRecord{}, err
	}
	var current uint64
	if err = tx.QueryRowContext(ctx, "SELECT COALESCE(MAX(seq),0) FROM records WHERE session_id=? AND namespace=? AND type=?", s.id, namespace, kind).Scan(&current); err != nil {
		return abi.SessionRecord{}, err
	}
	if current != expected {
		return abi.SessionRecord{}, storage.ErrRevisionConflict
	}
	batch := []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: namespace, Type: kind, SchemaVersion: 1, Payload: payload}}
	for _, record := range additional {
		if record.Namespace == namespace && record.Type == kind {
			return abi.SessionRecord{}, errors.New("additional records cannot replace the compared snapshot stream")
		}
		batch = append(batch, record)
	}
	records, err := appendTx(ctx, tx, s.id, batch)
	if err != nil {
		return abi.SessionRecord{}, err
	}
	if err = tx.Commit(); err != nil {
		return abi.SessionRecord{}, err
	}
	return records[0], nil
}
