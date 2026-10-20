// INPUT: context, database/sql, encoding/json, errors, strings, kernel/abi, llm/wire
// OUTPUT: BoundConversationTail, ConversationTail and immutable ConversationMetadata
// POS: [platform / durable storage] Bounded fixed-water metadata and verified tail reads without model-context selection policy.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const tailMaxMessages = 512
const tailMaxBytes = 8 << 20

// ConversationMetadata is a detached index entry; it does not grant artifact access.
type ConversationMetadata struct {
	Record       abi.RecordReference
	Role         wire.Role
	Reference    wire.Reference
	RestoreBytes int64
}

// ConversationTail holds an immutable high-water and private metadata. Only its
// bound authority can restore selected bodies; model wording belongs to a harness.
type ConversationTail struct {
	repository *Repository
	owner, id  string
	highWater  uint64
	metadata   []contextRecord
}

func (tail *ConversationTail) HighWater() uint64 { return tail.highWater }
func (tail *ConversationTail) SessionID() string { return tail.id }
func (tail *ConversationTail) Metadata() ([]ConversationMetadata, error) {
	out := make([]ConversationMetadata, len(tail.metadata))
	for i, record := range tail.metadata {
		if err := record.validate(); err != nil {
			return nil, err
		}
		role, _, ok := strings.Cut(record.summary, ":")
		if !ok {
			return nil, errors.New("conversation role metadata missing")
		}
		out[i] = ConversationMetadata{
			Record:       abi.RecordReference{SessionID: tail.id, RecordID: record.id, Sequence: record.sequence},
			Role:         wire.Role(role),
			Reference:    wire.Reference{URI: "alva-artifact://" + record.artifact.String, MediaType: record.mediaType.String, Processing: record.processing.String, SHA256: record.sha.String, Bytes: record.bytes.Int64},
			RestoreBytes: record.bytes.Int64 + runtimeRecordOverhead,
		}
		if err := out[i].Reference.Validate(); err != nil {
			return nil, err
		}
	}
	return out, nil
}

type contextRecord struct {
	id                         string
	sequence                   uint64
	summary                    string
	artifact                   sql.NullString
	mediaType, processing, sha sql.NullString
	bytes                      sql.NullInt64
}

// BoundConversationTail snapshots at most limit+1 newest conversation metadata
// entries. One extra entry proves truncation; no omitted body is restored.
func (r *Repository) BoundConversationTail(ctx context.Context, owner, id string, limit int) (*ConversationTail, error) {
	if limit < 1 || limit > tailMaxMessages {
		return nil, errors.New("conversation metadata limit must be 1..512")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return nil, err
	}
	var highWater uint64
	if err := r.db.QueryRowContext(ctx, "SELECT next_seq-1 FROM sessions WHERE id=?", id).Scan(&highWater); err != nil {
		return nil, err
	}
	// Metadata only, newest first; all artifact joins retain session ownership.
	// One extra row proves truncation without restoring the omitted prefix.
	rows, err := r.db.QueryContext(ctx, `SELECT r.id,r.seq,r.summary,a.id,a.media_type,a.processing,a.sha256,a.bytes FROM records r LEFT JOIN artifacts a ON a.id=r.artifact_id AND a.session_id=r.session_id WHERE r.session_id=? AND r.kind='conversation' AND r.seq<=? ORDER BY r.seq DESC LIMIT ?`, id, highWater, limit+1)
	if err != nil {
		return nil, err
	}
	var metadata []contextRecord
	for rows.Next() {
		var record contextRecord
		if err = rows.Scan(&record.id, &record.sequence, &record.summary, &record.artifact, &record.mediaType, &record.processing, &record.sha, &record.bytes); err != nil {
			rows.Close()
			return nil, err
		}
		metadata = append(metadata, record)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return nil, err
	}
	return &ConversationTail{repository: r, owner: owner, id: id, highWater: highWater, metadata: metadata}, nil
}

// Read restores the requested newest count in chronological order under an
// explicit byte budget. Hash/identity/protocol validation is a storage guarantee.
func (tail *ConversationTail) Read(ctx context.Context, count, maxBytes int) ([]wire.Message, error) {
	if count < 0 || count > len(tail.metadata) || count > tailMaxMessages || maxBytes < 1 || maxBytes > tailMaxBytes {
		return nil, errors.New("conversation tail read exceeds bounded admission")
	}
	r := tail.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	if err := authorize(ctx, r.db, tail.owner, tail.id); err != nil {
		return nil, err
	}
	selected := tail.metadata[:count]
	remaining := maxBytes
	messages := make([]wire.Message, 0, count)
	for i := len(selected) - 1; i >= 0; i-- {
		meta := selected[i]
		if err := meta.validate(); err != nil {
			return nil, err
		}
		if meta.bytes.Int64+runtimeRecordOverhead > int64(remaining) {
			return nil, errors.New("conversation tail exceeds byte budget")
		}
		data, err := r.readFullLocked(ctx, meta.artifact.String, remaining)
		if err != nil {
			return nil, err
		}
		remaining -= len(data) + runtimeRecordOverhead
		var record abi.SessionRecord
		if err := json.Unmarshal(data, &record); err != nil {
			return nil, errors.New("native context record decoding failed")
		}
		if err := record.Validate(); err != nil {
			return nil, err
		}
		if record.Kind != abi.RecordConversation || record.Message == nil || record.ID != meta.id || record.Sequence != meta.sequence || !strings.HasPrefix(meta.summary, string(record.Message.Role)+":") {
			return nil, errors.New("native context record metadata does not match committed body")
		}
		messages = append(messages, *record.Message)
	}
	if err := validateContextProtocol(messages); err != nil {
		return nil, err
	}
	return messages, nil
}

func (record contextRecord) validate() error {
	if record.id == "" || record.sequence == 0 || !record.artifact.Valid || !record.mediaType.Valid || !record.processing.Valid || !record.sha.Valid || !record.bytes.Valid || record.bytes.Int64 < 0 || record.bytes.Int64 > maxRecordBytes {
		return errors.New("native context artifact metadata missing or invalid")
	}
	return nil
}

func validateContextProtocol(messages []wire.Message) error {
	pending := make(map[string]bool)
	for _, message := range messages {
		for _, block := range message.Content {
			if block.ToolCall != nil {
				if pending[block.ToolCall.ID] {
					return errors.New("native context has duplicate pending tool call")
				}
				pending[block.ToolCall.ID] = true
			}
			if block.ToolResult != nil {
				if !pending[block.ToolResult.CallID] {
					return errors.New("native context would orphan a tool result")
				}
				delete(pending, block.ToolResult.CallID)
			}
		}
	}
	if len(pending) != 0 {
		return errors.New("native context has unresolved tool calls")
	}
	return nil
}
