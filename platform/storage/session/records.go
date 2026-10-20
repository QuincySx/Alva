// INPUT: context, database/sql, encoding/json, kernel/abi, llm/wire.
// OUTPUT: Atomic Session append/runtime reads and bounded Repository Page/Search metadata.
// POS: [platform / durable storage] Typed execution fact authority with Inspector paths independent from runtime restoration.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const MaxBatchRecords = 2 + 3*wire.MaxStreamContentBlocks
const maxRuntimeRecords = 4096
const maxRuntimeBytes = 64 << 20

// Account for retained decoded record/key overhead as well as stored bytes.
const runtimeRecordOverhead = 512

func (s *Session) AppendRecords(ctx context.Context, input []abi.SessionRecord) ([]abi.SessionRecord, error) {
	r := s.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, storage.ErrClosed
	}
	tx, err := r.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()
	if err = authorize(ctx, tx, s.owner, s.id); err != nil {
		return nil, err
	}
	committed, err := appendTx(ctx, tx, s.id, input)
	if err != nil {
		return nil, err
	}
	if err = tx.Commit(); err != nil {
		return nil, err
	}
	return committed, nil
}

func appendTx(ctx context.Context, tx *sql.Tx, id string, input []abi.SessionRecord) ([]abi.SessionRecord, error) {
	if err := validateRecordBatchBudget(input); err != nil {
		return nil, err
	}
	owned := abi.CloneRecords(input)
	for index := range owned {
		if err := redaction.RedactRecord(&owned[index]); err != nil {
			return nil, err
		}
		record := owned[index]
		if err := record.Validate(); err != nil {
			return nil, err
		}
		if record.ID != "" || record.Sequence != 0 || !record.Time.IsZero() {
			return nil, errors.New("record identity is store-owned")
		}
	}
	var next uint64
	if err := tx.QueryRowContext(ctx, "SELECT next_seq FROM sessions WHERE id=?", id).Scan(&next); err != nil {
		return nil, err
	}
	for index := range owned {
		record := &owned[index]
		if record.Parent != nil {
			if record.Parent.SessionID != id || record.Parent.Sequence == 0 || record.Parent.Sequence >= next {
				return nil, errors.New("causal parent is outside committed/staged session")
			}
			var parentID string
			if err := tx.QueryRowContext(ctx, "SELECT id FROM records WHERE session_id=? AND seq=?", id, record.Parent.Sequence).Scan(&parentID); err != nil {
				return nil, err
			}
			if parentID != record.Parent.RecordID {
				return nil, errors.New("causal parent identity mismatch")
			}
		}
		var requestID, runID, callID, status any
		switch record.Kind {
		case abi.RecordModelIntent:
			requestID = record.ModelIntent.RequestID
		case abi.RecordModelOutcome:
			requestID = record.ModelOutcome.RequestID
			status = record.ModelOutcome.Status
			var found int
			if err := tx.QueryRowContext(ctx, "SELECT 1 FROM records WHERE session_id=? AND kind='model_intent' AND request_id=?", id, requestID).Scan(&found); err != nil {
				if !errors.Is(err, sql.ErrNoRows) {
					return nil, err
				}
				return nil, errors.New("model outcome has no matching intent")
			}
		case abi.RecordToolIntent:
			runID = record.ToolIntent.RunID
			callID = record.ToolIntent.Call.ID
		case abi.RecordToolOutcome:
			runID = record.ToolOutcome.RunID
			callID = record.ToolOutcome.CallID
			status = record.ToolOutcome.Status
			var found int
			if err := tx.QueryRowContext(ctx, "SELECT 1 FROM records WHERE session_id=? AND kind='tool_intent' AND run_id=? AND call_id=?", id, runID, callID).Scan(&found); err != nil {
				if !errors.Is(err, sql.ErrNoRows) {
					return nil, err
				}
				return nil, errors.New("tool outcome has no matching intent")
			}
		}
		record.Sequence = next
		record.ID = fmt.Sprintf("%s:%d", id, next)
		record.Time = time.Now().UTC()
		if record.Message != nil && record.Message.ID == "" {
			record.Message.ID = record.ID
		}
		if record.Parent == nil && next > 1 {
			var previous string
			if err := tx.QueryRowContext(ctx, "SELECT id FROM records WHERE session_id=? AND seq=?", id, next-1).Scan(&previous); err != nil {
				return nil, err
			}
			record.Parent = &abi.RecordReference{SessionID: id, RecordID: previous, Sequence: next - 1}
		}
		encoded, err := json.Marshal(record)
		if err != nil {
			return nil, err
		}
		info, err := putArtifactTx(ctx, tx, id, storage.ArtifactInput{Kind: "typed-record-json", Name: record.ID, MediaType: "application/json", Processing: "Complete committed typed record JSON; inspection only reads requested byte ranges, never resumes execution", Data: encoded, Text: true}, maxRecordBytes, nil)
		if err != nil {
			return nil, err
		}
		var parent any
		if record.Parent != nil {
			data, _ := json.Marshal(record.Parent)
			parent = string(data)
		}
		_, err = tx.ExecContext(ctx, "INSERT INTO records(session_id,seq,id,time_ns,kind,namespace,type,schema_version,parent_json,summary,artifact_id,request_id,run_id,call_id,status) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)", id, next, record.ID, record.Time.UnixNano(), record.Kind, record.Namespace, record.Type, record.SchemaVersion, parent, summary(*record), info.ID, requestID, runID, callID, status)
		if err != nil {
			return nil, fmt.Errorf("append typed record: %w", err)
		}
		next++
	}
	if _, err := tx.ExecContext(ctx, "UPDATE sessions SET next_seq=? WHERE id=?", next, id); err != nil {
		return nil, err
	}
	return abi.CloneRecords(owned), nil
}

func summary(record abi.SessionRecord) string {
	text := string(record.Kind)
	switch {
	case record.Message != nil:
		text = string(record.Message.Role) + ": " + record.Message.Text()
	case record.ToolIntent != nil:
		text = record.ToolIntent.Call.Name + " " + record.ToolIntent.Call.ID
	case record.ToolOutcome != nil:
		text = string(record.ToolOutcome.Status) + " " + record.ToolOutcome.CallID
	case record.ModelOutcome != nil:
		text = string(record.ModelOutcome.Status)
	case record.Kind == abi.RecordExtension:
		text = record.Namespace + "/" + record.Type
	}
	if len(text) > 256 {
		text = text[:256]
		for len(text) > 0 && !utf8.ValidString(text) {
			text = text[:len(text)-1]
		}
	}
	return text
}

func (r *Repository) runtimeRecordsLocked(ctx context.Context, owner, id string, after uint64, conversationOnly bool) ([]abi.SessionRecord, error) {
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return nil, err
	}
	query := "SELECT r.artifact_id,a.bytes FROM records r LEFT JOIN artifacts a ON a.id=r.artifact_id AND a.session_id=r.session_id WHERE r.session_id=? AND r.seq>?"
	limit := maxRuntimeRecords
	if conversationOnly {
		query += " AND r.kind='conversation'"
		// Messages is the complete conversation projection, not an Inspector
		// record page. Bound its retained size without making 4097 tiny messages
		// fail before the explicitly configured context system can compact them.
		limit = maxRuntimeBytes / runtimeRecordOverhead
	}
	query += " ORDER BY r.seq LIMIT ?"
	rows, err := r.db.QueryContext(ctx, query, id, after, limit+1)
	if err != nil {
		return nil, err
	}
	var keys []string
	retainedBytes := int64(0)
	for rows.Next() {
		var key string
		var size sql.NullInt64
		if err = rows.Scan(&key, &size); err != nil {
			rows.Close()
			return nil, err
		}
		if !size.Valid || size.Int64 < 0 {
			rows.Close()
			return nil, errors.New("runtime record artifact metadata missing or invalid")
		}
		cost := size.Int64 + runtimeRecordOverhead
		if cost < size.Int64 || cost > maxRuntimeBytes-retainedBytes {
			rows.Close()
			return nil, errors.New("runtime history exceeds 64 MiB restore budget; use bounded retrieval")
		}
		retainedBytes += cost
		keys = append(keys, key)
	}
	if err = rows.Err(); err != nil {
		rows.Close()
		return nil, err
	}
	rows.Close()
	if len(keys) > limit {
		return nil, errors.New("runtime history exceeds 4096-record restore budget; use bounded retrieval")
	}
	var records []abi.SessionRecord
	remaining := maxRuntimeBytes
	for _, key := range keys {
		data, err := r.readFullLocked(ctx, key, remaining)
		if err != nil {
			return nil, err
		}
		remaining -= len(data) + runtimeRecordOverhead
		var record abi.SessionRecord
		if err = json.Unmarshal(data, &record); err != nil {
			return nil, errors.New("stored typed record decoding failed")
		}
		if err = record.Validate(); err != nil {
			return nil, err
		}
		records = append(records, record)
	}
	return records, nil
}

func (s *Session) Records(ctx context.Context, after uint64) ([]abi.SessionRecord, error) {
	r := s.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, storage.ErrClosed
	}
	return r.runtimeRecordsLocked(ctx, s.owner, s.id, after, false)
}
func (s *Session) Messages(ctx context.Context) ([]wire.Message, error) {
	r := s.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, storage.ErrClosed
	}
	records, err := r.runtimeRecordsLocked(ctx, s.owner, s.id, 0, true)
	if err != nil {
		return nil, err
	}
	messages := make([]wire.Message, 0, len(records))
	for _, record := range records {
		messages = append(messages, *record.Message)
	}
	return messages, nil
}

func (r *Repository) pageLocked(ctx context.Context, owner, id string, after uint64, limit int, highWater uint64) (storage.RecordPage, error) {
	if limit < 1 || limit > 100 {
		return storage.RecordPage{}, errors.New("record page limit must be 1..100")
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return storage.RecordPage{}, err
	}
	var current uint64
	if err := r.db.QueryRowContext(ctx, "SELECT next_seq-1 FROM sessions WHERE id=?", id).Scan(&current); err != nil {
		return storage.RecordPage{}, err
	}
	if highWater == 0 {
		highWater = current
	}
	if highWater > current || after > highWater {
		return storage.RecordPage{}, errors.New("record cursor is ahead of committed high-water")
	}
	rows, err := r.db.QueryContext(ctx, `SELECT r.id,r.seq,r.kind,r.namespace,r.type,r.schema_version,r.parent_json,r.summary,a.id,a.media_type,a.processing,a.sha256,a.bytes,r.time_ns FROM records r JOIN artifacts a ON a.id=r.artifact_id WHERE r.session_id=? AND r.seq>? AND r.seq<=? ORDER BY r.seq LIMIT ?`, id, after, highWater, limit+1)
	if err != nil {
		return storage.RecordPage{}, err
	}
	defer rows.Close()
	page := storage.RecordPage{Next: after, HighWater: highWater}
	for rows.Next() {
		var entry storage.RecordEntry
		var parent sql.NullString
		var info storage.ArtifactInfo
		var created int64
		if err = rows.Scan(&entry.ID, &entry.Sequence, &entry.Kind, &entry.Namespace, &entry.Type, &entry.SchemaVersion, &parent, &entry.Summary, &info.ID, &info.MediaType, &info.Processing, &info.SHA256, &info.Bytes, &created); err != nil {
			return storage.RecordPage{}, err
		}
		if len(page.Entries) == limit {
			page.HasMore = true
			break
		}
		if parent.Valid {
			if err = json.Unmarshal([]byte(parent.String), &entry.Parent); err != nil {
				return storage.RecordPage{}, errors.New("stored record parent metadata is invalid")
			}
		}
		entry.Time = time.Unix(0, created).UTC()
		entry.Value = info.Reference()
		page.Entries = append(page.Entries, entry)
		page.Next = entry.Sequence
	}
	return page, rows.Err()
}
func (r *Repository) Page(ctx context.Context, owner, id string, after uint64, limit int, highWater uint64) (storage.RecordPage, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return storage.RecordPage{}, storage.ErrClosed
	}
	return r.pageLocked(ctx, owner, id, after, limit, highWater)
}

// Search is literal over serialized typed record JSON, one first match/record.
// It scans at most 100 records and approximately 1 MiB (one larger record may
// exceed that byte budget), returning an explicit continuation high-water.
func (r *Repository) Search(ctx context.Context, owner, id, query string, after uint64, limit int, highWater uint64) (storage.SearchPage, error) {
	if query == "" || len(query) > 256 || !utf8.ValidString(query) || limit < 1 || limit > 20 {
		return storage.SearchPage{}, errors.New("literal record search requires 1..256 UTF-8 bytes and 1..20 matches")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return storage.SearchPage{}, storage.ErrClosed
	}
	page, err := r.pageLocked(ctx, owner, id, after, 100, highWater)
	if err != nil {
		return storage.SearchPage{}, err
	}
	result := storage.SearchPage{Next: after, HighWater: page.HighWater}
	scanned := int64(0)
	for _, entry := range page.Entries {
		if scanned > 0 && scanned+entry.Value.Bytes > 1<<20 {
			break
		}
		info, err := r.artifactInfoLocked(ctx, owner, id, entry.Value.URI)
		if err != nil {
			return storage.SearchPage{}, err
		}
		matches, err := r.searchArtifactLocked(ctx, info, query, 1)
		if err != nil {
			return storage.SearchPage{}, err
		}
		scanned += entry.Value.Bytes
		result.Next = entry.Sequence
		if len(matches) > 0 {
			result.Matches = append(result.Matches, storage.RecordMatch{Entry: entry, Offset: matches[0].Offset, Snippet: matches[0].Text})
			if len(result.Matches) == limit {
				break
			}
		}
	}
	result.HasMore = result.Next < result.HighWater
	return result, nil
}
