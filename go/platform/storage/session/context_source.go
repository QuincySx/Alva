// INPUT: context, database/sql, encoding/json, errors, fmt, reflect, strings, kernel/abi, llm/wire
// OUTPUT: Repository.BoundContextSource returning an owned bounded native working-view source
// POS: [platform / durable storage] Restores a fixed-high-water protocol-complete tail without reading or rewriting the full conversation.
package session

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const contextTargetMessages = 64
const contextMaxMessages = 512
const contextMaxBytes = 8 << 20

type nativeContextSource struct {
	repository *Repository
	owner, id  string
}

type contextRecord struct {
	id                         string
	sequence                   uint64
	summary                    string
	artifact                   sql.NullString
	mediaType, processing, sha sql.NullString
	bytes                      sql.NullInt64
}

// BoundContextSource binds trusted owner/session identity at assembly time.
// Load accepts only that same runtime session. The original Session remains a
// complete ConversationLog; this capability is explicitly a model working view.
func (r *Repository) BoundContextSource(ctx context.Context, owner, id string) (abi.ContextSource, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return nil, err
	}
	return &nativeContextSource{repository: r, owner: owner, id: id}, nil
}

func (source *nativeContextSource) Load(ctx context.Context, log abi.ConversationReader) ([]wire.Message, error) {
	if missingContextReader(log) || log.ID() != source.id {
		return nil, errors.New("native context source belongs to a different session")
	}
	r := source.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	if err := authorize(ctx, r.db, source.owner, source.id); err != nil {
		return nil, err
	}
	var highWater uint64
	if err := r.db.QueryRowContext(ctx, "SELECT next_seq-1 FROM sessions WHERE id=?", source.id).Scan(&highWater); err != nil {
		return nil, err
	}
	// Metadata only, newest first; all artifact joins retain session ownership.
	// One extra row proves truncation without restoring the omitted prefix.
	rows, err := r.db.QueryContext(ctx, `SELECT r.id,r.seq,r.summary,a.id,a.media_type,a.processing,a.sha256,a.bytes FROM records r LEFT JOIN artifacts a ON a.id=r.artifact_id AND a.session_id=r.session_id WHERE r.session_id=? AND r.kind='conversation' AND r.seq<=? ORDER BY r.seq DESC LIMIT ?`, source.id, highWater, contextMaxMessages+1)
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
	if len(metadata) == 0 {
		return nil, nil
	}
	// Prefer at least the target number of messages, but never split a user
	// turn. A large older turn may be omitted before restoration; the newest
	// complete turn must fit or the source fails explicitly.
	last := -1
	remaining := contextMaxBytes
	for i, record := range metadata {
		if i >= contextMaxMessages {
			break
		}
		if err := record.validate(); err != nil {
			return nil, err
		}
		cost := record.bytes.Int64 + runtimeRecordOverhead
		if cost > int64(remaining) {
			break
		}
		remaining -= int(cost)
		if strings.HasPrefix(record.summary, "user:") {
			last = i
			if i+1 >= contextTargetMessages {
				break
			}
		}
		// If all history fits, a log begun without a user message is still
		// legal. Protocol validation below verifies its actual contents.
		if i == len(metadata)-1 {
			last = i
		}
	}
	if last < 0 {
		return nil, errors.New("native context latest turn exceeds 512-message / 8 MiB working-view budget")
	}
	selected := metadata[:last+1]
	remaining = contextMaxBytes
	messages := make([]wire.Message, 0, len(selected)+1)
	if last+1 < len(metadata) {
		boundary := metadata[last+1]
		if err := boundary.validate(); err != nil {
			return nil, err
		}
		reference := wire.Reference{URI: "alva-artifact://" + boundary.artifact.String, MediaType: boundary.mediaType.String, Processing: boundary.processing.String, SHA256: boundary.sha.String, Bytes: boundary.bytes.Int64}
		if err := reference.Validate(); err != nil {
			return nil, err
		}
		text := fmt.Sprintf("Earlier conversation records through sequence %d are omitted from this bounded working view at high-water %d. This is not a summary. Original facts remain unchanged. Use native_history with high_water=%d and context_read/context_search for explicit bounded retrieval; this reference identifies only the last omitted record.", boundary.sequence, highWater, boundary.sequence)
		messages = append(messages, wire.Message{ID: fmt.Sprintf("context/native/%s/%d/%d", source.id, boundary.sequence, highWater), Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}, {Kind: wire.ContentReference, Reference: &reference}}})
	}
	for i := len(selected) - 1; i >= 0; i-- {
		meta := selected[i]
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

func missingContextReader(reader abi.ConversationReader) bool {
	if reader == nil {
		return true
	}
	value := reflect.ValueOf(reader)
	switch value.Kind() {
	case reflect.Pointer, reflect.Interface, reflect.Map, reflect.Func, reflect.Slice, reflect.Chan:
		return value.IsNil()
	default:
		return false
	}
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
