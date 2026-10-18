// INPUT: context, errors, fmt, sync, time, kernel/abi, llm/wire
// OUTPUT: MemorySession, NewMemorySession
// POS: [L2.5 SDK assembly] Atomic in-memory SDK log with typed facts and isolated conversation projection.
package core

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type MemorySession struct {
	mu            sync.RWMutex
	id            string
	records       []abi.SessionRecord
	messages      []wire.Message
	modelIntents  map[string]bool
	modelOutcomes map[string]bool
	toolIntents   map[string]bool
	toolOutcomes  map[string]bool
}

func NewMemorySession(id string) *MemorySession {
	return &MemorySession{id: id, modelIntents: make(map[string]bool), modelOutcomes: make(map[string]bool), toolIntents: make(map[string]bool), toolOutcomes: make(map[string]bool)}
}
func (session *MemorySession) ID() string { return session.id }
func (session *MemorySession) Messages(ctx context.Context) ([]wire.Message, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	session.mu.RLock()
	defer session.mu.RUnlock()
	return wire.CloneMessages(session.messages), nil
}
func (session *MemorySession) Records(ctx context.Context, after uint64) ([]abi.SessionRecord, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	session.mu.RLock()
	defer session.mu.RUnlock()
	if after >= uint64(len(session.records)) {
		return nil, nil
	}
	return abi.CloneRecords(session.records[after:]), nil
}
func (session *MemorySession) Flush(ctx context.Context) error { return ctx.Err() }

func (session *MemorySession) AppendRecords(ctx context.Context, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if session.id == "" {
		return nil, errors.New("session ID cannot be empty")
	}
	owned := abi.CloneRecords(records)
	for _, record := range owned {
		if err := record.Validate(); err != nil {
			return nil, err
		}
		if record.ID != "" || record.Sequence != 0 || !record.Time.IsZero() {
			return nil, errors.New("record identity is store-owned")
		}
	}
	session.mu.Lock()
	defer session.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	// Validate the complete batch against staged indexes, without mutating any
	// committed state. Failed validation cannot partially append messages/facts.
	models := make(map[string]bool)
	modelOutcomes := make(map[string]bool)
	tools := make(map[string]bool)
	toolOutcomes := make(map[string]bool)
	for i := range owned {
		record := &owned[i]
		if record.Parent != nil {
			parent := record.Parent
			if parent.SessionID != session.id || parent.Sequence == 0 || parent.Sequence > uint64(len(session.records)+i) {
				return nil, errors.New("causal parent is outside committed/staged session")
			}
			var actual abi.SessionRecord
			if parent.Sequence <= uint64(len(session.records)) {
				actual = session.records[parent.Sequence-1]
			} else {
				actual = owned[int(parent.Sequence)-len(session.records)-1]
			}
			if parent.RecordID != actual.ID {
				return nil, errors.New("causal parent identity mismatch")
			}
		}
		switch record.Kind {
		case abi.RecordModelIntent:
			id := record.ModelIntent.RequestID
			if session.modelIntents[id] || models[id] {
				return nil, errors.New("duplicate model intent")
			}
			models[id] = true
		case abi.RecordModelOutcome:
			id := record.ModelOutcome.RequestID
			if !(session.modelIntents[id] || models[id]) || session.modelOutcomes[id] || modelOutcomes[id] {
				return nil, errors.New("unmatched or duplicate model outcome")
			}
			modelOutcomes[id] = true
		case abi.RecordToolIntent:
			id := toolKey(record.ToolIntent.RunID, record.ToolIntent.Call.ID)
			if session.toolIntents[id] || tools[id] {
				return nil, errors.New("duplicate tool intent")
			}
			tools[id] = true
		case abi.RecordToolOutcome:
			id := toolKey(record.ToolOutcome.RunID, record.ToolOutcome.CallID)
			if !(session.toolIntents[id] || tools[id]) || session.toolOutcomes[id] || toolOutcomes[id] {
				return nil, errors.New("unmatched or duplicate tool outcome")
			}
			toolOutcomes[id] = true
		}
		record.Sequence = uint64(len(session.records) + i + 1)
		record.ID = fmt.Sprintf("%s:%d", session.id, record.Sequence)
		if record.Message != nil && record.Message.ID == "" {
			record.Message.ID = record.ID
		}
		record.Time = time.Now().UTC()
		if record.Parent == nil && record.Sequence > 1 {
			var previous abi.SessionRecord
			if i > 0 {
				previous = owned[i-1]
			} else {
				previous = session.records[len(session.records)-1]
			}
			record.Parent = &abi.RecordReference{SessionID: session.id, RecordID: previous.ID, Sequence: previous.Sequence}
		}
	}
	for key := range models {
		session.modelIntents[key] = true
	}
	for key := range modelOutcomes {
		session.modelOutcomes[key] = true
	}
	for key := range tools {
		session.toolIntents[key] = true
	}
	for key := range toolOutcomes {
		session.toolOutcomes[key] = true
	}
	for _, record := range owned {
		if record.Kind == abi.RecordConversation {
			session.messages = append(session.messages, wire.CloneMessages([]wire.Message{*record.Message})[0])
		}
	}
	session.records = append(session.records, owned...)
	return abi.CloneRecords(owned), nil
}

func toolKey(runID, callID string) string { return fmt.Sprintf("%d:%s%s", len(runID), runID, callID) }
