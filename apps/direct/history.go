// INPUT: Direct owner/session identity and explicit fixed-high-water native record/reference queries
// OUTPUT: Cold History, InspectorRead and bounded conversation HistoryPage without runtime restoration
// POS: [L6 application] Native facts remain authoritative; display text is a disposable bounded projection with original references.
package direct

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/platform/storage/session"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type ConversationRow struct {
	ID         string
	Sequence   uint64
	Message    *wire.Message
	Reference  wire.Reference
	Processing string
}
type HistoryPage struct {
	Rows             []ConversationRow
	After, HighWater uint64
	HasEarlier       bool
}

func (s *Service) History(ctx context.Context, id, query string, after, highWater uint64) (session.RecordPage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return session.RecordPage{}, session.ErrClosed
	}
	if query == "" {
		return s.repository.Page(ctx, owner, id, after, 20, highWater)
	}
	page, err := s.repository.Search(ctx, owner, id, query, after, 20, highWater)
	if err != nil {
		return session.RecordPage{}, err
	}
	result := session.RecordPage{Next: page.Next, HighWater: page.HighWater, HasMore: page.HasMore}
	for _, match := range page.Matches {
		result.Entries = append(result.Entries, match.Entry)
	}
	return result, nil
}
func (s *Service) InspectorRead(ctx context.Context, id, uri string, offset int64) (agentcontext.TextPage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return agentcontext.TextPage{}, session.ErrClosed
	}
	references, err := s.repository.BoundReferences(ctx, owner, id)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	return references.ReadText(ctx, uri, offset, 2048)
}

// Conversation reads a window of <=100 record sequences. before=0 selects the
// latest committed high-water; the returned After allows an earlier window.
// Large message bodies remain explicit references instead of fake truncation.
func (s *Service) Conversation(ctx context.Context, id string, before uint64) (HistoryPage, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return HistoryPage{}, session.ErrClosed
	}
	probe, err := s.repository.Page(ctx, owner, id, 0, 1, 0)
	if err != nil {
		return HistoryPage{}, err
	}
	high := probe.HighWater
	if before > 0 {
		if before > high+1 {
			return HistoryPage{}, errors.New("conversation cursor exceeds committed facts")
		}
		high = before - 1
	}
	after := uint64(0)
	if high > 100 {
		after = high - 100
	}
	result := HistoryPage{After: after, HighWater: high, HasEarlier: after > 0, Rows: []ConversationRow{}}
	if high == 0 {
		return result, nil
	}
	page, err := s.repository.Page(ctx, owner, id, after, 100, high)
	if err != nil {
		return HistoryPage{}, err
	}
	budget := 128 << 10
	for _, entry := range page.Entries {
		if entry.Kind != abi.RecordConversation {
			continue
		}
		row := ConversationRow{ID: entry.ID, Sequence: entry.Sequence, Reference: entry.Value}
		if entry.Value.Bytes > int64(min(32<<10, budget)) {
			row.Processing = "Message body omitted whole from bounded display; inspect the saved original reference explicitly."
			result.Rows = append(result.Rows, row)
			continue
		}
		_, raw, err := s.repository.ReadArtifactFull(ctx, owner, id, entry.Value.URI, min(32<<10, budget))
		if err != nil {
			return HistoryPage{}, err
		}
		budget -= len(raw)
		var record abi.SessionRecord
		if err := json.Unmarshal(raw, &record); err != nil {
			return HistoryPage{}, err
		}
		if record.ID != entry.ID || record.Sequence != entry.Sequence || record.Kind != abi.RecordConversation || record.Message == nil {
			return HistoryPage{}, errors.New("conversation record identity/payload mismatch")
		}
		cloned := wire.CloneMessages([]wire.Message{*record.Message})
		row.Message = &cloned[0]
		result.Rows = append(result.Rows, row)
	}
	return result, nil
}
