// INPUT: context, strconv, hub committed history pages, engine events (with tool displays) and the tool view folding
// OUTPUT: Block content-block values (tool blocks carry a ToolView), TranscriptStore port, TranscriptService paging and the shared history projection
// POS: [L7 Hub product] UI-independent Message Stream projection from committed product history; never model context or fact authority
package product

import (
	"context"
	"errors"
	"strconv"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// BlockKind is the presentation type of one content block. Interfaces render
// by kind, never by tool or engine name.
type BlockKind string

const (
	BlockKindUser      BlockKind = "user"
	BlockKindReply     BlockKind = "reply"
	BlockKindReasoning BlockKind = "reasoning"
	BlockKindContent   BlockKind = "content"
	BlockKindTool      BlockKind = "tool"
	BlockKindActivity  BlockKind = "activity"
	BlockKindStatus    BlockKind = "status"
	BlockKindTimeline  BlockKind = "timeline"
)

// BlockStatus uses the engine outcome spellings plus running.
type BlockStatus string

const (
	BlockStatusRunning     BlockStatus = "running"
	BlockStatusCompleted   BlockStatus = "completed"
	BlockStatusFailed      BlockStatus = "failed"
	BlockStatusCancelled   BlockStatus = "cancelled"
	BlockStatusInterrupted BlockStatus = "interrupted"
	BlockStatusWaiting     BlockStatus = "waiting"
)

// Block is one item of the Message Stream projected from committed history.
//
// ID is stable across pages: attemptID+"/"+event ID, turnID+"/input",
// attemptID+"/activity/"+activity ID, or a sequence-qualified status/timeline ID.
// Name is the tool, content-block kind, activity or timeline kind; status
// blocks name their source: "error", "retry" or "settlement". Truncated marks
// Text as a bounded prefix that Ref, when set, reaches in full. Ref is the final
// event's native reference (for a tool, its latest event's). NativeSessionID
// attributes authorship: a tool keeps its call's native owner, other blocks
// carry their attempt's unambiguous committed origin. HandoffID names the saved
// handoff snapshot a timeline block refers to.
type Block struct {
	ID              string
	Kind            BlockKind
	SessionID       string
	TurnID          string
	AttemptID       string
	Turn            uint64 // HistoryEntry.TurnSequence; 0 when unknown
	Status          BlockStatus
	Name            string
	Text            string
	Truncated       bool
	Ref             *engine.Reference
	NativeSessionID string
	HandoffID       string
	// History is the latest committed product history sequence folded into
	// the block, the position a trace jump binds its Turn evidence to.
	History uint64
	// Tool is a tool block's presentation, derived once from its events
	// (nil for other kinds). The value is shared and never mutated.
	Tool *ToolView
}

// TranscriptStore is the only history capability the Message Stream needs.
type TranscriptStore interface {
	History(ctx context.Context, id string, after uint64, limit int) ([]hub.HistoryEntry, error)
	HistoryBefore(ctx context.Context, id string, before uint64, limit int) ([]hub.HistoryEntry, error)
}

// TranscriptService pages committed history for a TranscriptWindow. It owns no
// window state and widens no other HubService responsibility.
type TranscriptService struct{ Store TranscriptStore }

// TranscriptChunk carries one contiguous run of committed history. Its entries
// stay private: callers can only hand it to a TranscriptWindow.
type TranscriptChunk struct {
	entries     []hub.HistoryEntry
	First, Last uint64
	// More reports that history may continue past this chunk in the direction
	// it was read: earlier for Tail/Older, later for Newer.
	More bool
}

const (
	transcriptPage          = 128
	transcriptBackwardPages = 16
	transcriptForwardPages  = 8
	transcriptVisibleBlocks = 32
)

// Tail reads the newest history until it holds enough user/reply/tool blocks.
func (s *TranscriptService) Tail(ctx context.Context, sessionID string) (TranscriptChunk, error) {
	return s.backward(ctx, sessionID, 0)
}

// Older reads history strictly before the given committed sequence.
func (s *TranscriptService) Older(ctx context.Context, sessionID string, before uint64) (TranscriptChunk, error) {
	if before == 0 {
		// Zero means "no bound" to the store; an empty window has nothing older.
		return TranscriptChunk{}, nil
	}
	return s.backward(ctx, sessionID, before)
}

// Newer reads history strictly after the given committed sequence.
func (s *TranscriptService) Newer(ctx context.Context, sessionID string, after uint64) (TranscriptChunk, error) {
	if s == nil || s.Store == nil {
		return TranscriptChunk{}, errors.New("transcript history is unavailable")
	}
	var chunk TranscriptChunk
	for page := 0; page < transcriptForwardPages; page++ {
		entries, err := s.Store.History(ctx, sessionID, after, transcriptPage)
		if err != nil {
			return TranscriptChunk{}, err
		}
		chunk.entries = append(chunk.entries, entries...)
		chunk.More = len(entries) == transcriptPage
		if !chunk.More {
			break
		}
		after = entries[len(entries)-1].Sequence
	}
	chunk.bounds()
	return chunk, nil
}

func (s *TranscriptService) backward(ctx context.Context, sessionID string, before uint64) (TranscriptChunk, error) {
	if s == nil || s.Store == nil {
		return TranscriptChunk{}, errors.New("transcript history is unavailable")
	}
	var pages [][]hub.HistoryEntry
	visible := map[string]bool{}
	more := false
	for page := 0; page < transcriptBackwardPages; page++ {
		entries, err := s.Store.HistoryBefore(ctx, sessionID, before, transcriptPage)
		if err != nil {
			return TranscriptChunk{}, err
		}
		pages = append(pages, entries)
		for _, entry := range entries {
			if id := visibleBlockID(entry); id != "" {
				visible[id] = true
			}
		}
		more = len(entries) == transcriptPage
		if !more || len(visible) >= transcriptVisibleBlocks {
			break
		}
		before = entries[0].Sequence
	}
	var chunk TranscriptChunk
	for i := len(pages) - 1; i >= 0; i-- {
		chunk.entries = append(chunk.entries, pages[i]...)
	}
	chunk.More = more
	chunk.bounds()
	return chunk, nil
}

func (c *TranscriptChunk) bounds() {
	if len(c.entries) != 0 {
		c.First, c.Last = c.entries[0].Sequence, c.entries[len(c.entries)-1].Sequence
	}
}

// visibleBlockID identifies user, reply and tool blocks, the ones a reader
// counts as messages when deciding whether a page shows enough conversation.
func visibleBlockID(entry hub.HistoryEntry) string {
	if entry.Input != "" {
		return entry.TurnID + "/input"
	}
	if entry.Timeline != nil {
		return ""
	}
	switch entry.Event.Kind {
	case engine.MessageDelta, engine.Message, engine.ToolStart, engine.ToolEnd:
		return entry.AttemptID + "/" + entry.Event.ID
	}
	return ""
}

// blockState is a Block plus the projection facts the window needs: whether
// its text is final and the committed sequence span folded into it.
type blockState struct {
	Block
	final       bool
	first, last uint64
}

// blockOrigin is an attempt's native identity from committed events only.
type blockOrigin struct {
	sessionID, nativeID string
	ambiguous           bool
}

// projection folds history entries, in order, into blocks. It is the moved
// desktop Transcript.project logic, made incremental and keyed by block ID.
type projection struct {
	blocks      []blockState
	index       map[string]int
	origins     map[string]blockOrigin
	outcomes    map[string]BlockStatus
	first, last uint64
}

func newProjection() *projection {
	return &projection{index: map[string]int{}, origins: map[string]blockOrigin{}, outcomes: map[string]BlockStatus{}}
}

// observeOrigin records the attempt's native identity and reports a change.
func (p *projection) observeOrigin(attempt, session, native string) bool {
	origin, exists := p.origins[attempt]
	switch {
	case !exists:
		p.origins[attempt] = blockOrigin{sessionID: session, nativeID: native}
		return true
	case !origin.ambiguous && (origin.sessionID != session || origin.nativeID != native):
		origin.ambiguous = true
		p.origins[attempt] = origin
		return true
	}
	return false
}

// attribute gives a non-tool block its attempt's unambiguous native origin.
func (p *projection) attribute(b *blockState) {
	if b.Kind == BlockKindTool {
		return
	}
	b.NativeSessionID = ""
	if origin, exists := p.origins[b.AttemptID]; exists && b.AttemptID != "" && !origin.ambiguous && origin.sessionID == b.SessionID {
		b.NativeSessionID = origin.nativeID
	}
}

func (p *projection) attributeAttempt(attempt string) {
	for i := range p.blocks {
		if p.blocks[i].AttemptID == attempt {
			p.attribute(&p.blocks[i])
		}
	}
}

// upsert returns the block with id, creating it at the end when missing.
func (p *projection) upsert(id string, entry hub.HistoryEntry, kind BlockKind, status BlockStatus) *blockState {
	if i, exists := p.index[id]; exists {
		b := &p.blocks[i]
		b.last = max(b.last, entry.Sequence)
		if b.Turn == 0 {
			b.Turn = entry.TurnSequence
		}
		return b
	}
	p.index[id] = len(p.blocks)
	p.blocks = append(p.blocks, blockState{Block: Block{ID: id, Kind: kind, SessionID: entry.SessionID, TurnID: entry.TurnID, AttemptID: entry.AttemptID, Turn: entry.TurnSequence, Status: status}, first: entry.Sequence, last: entry.Sequence})
	b := &p.blocks[len(p.blocks)-1]
	p.attribute(b)
	return b
}

func cloneReference(reference *engine.Reference) *engine.Reference {
	if reference == nil {
		return nil
	}
	value := *reference
	return &value
}

// rememberTool keeps a call's native owner independent of its result
// reference. Synthetic ends may omit both identity fields; they inherit the
// same call's owner within one product session, never a start reference.
func rememberTool(b *blockState, entry hub.HistoryEntry) {
	previousSession, previousNative, previousName := b.SessionID, b.NativeSessionID, b.Name
	b.SessionID = entry.SessionID
	native, name := entry.Event.SessionID, entry.Event.Name
	if name == "" {
		name = previousName
	}
	b.Ref = cloneReference(entry.Event.Reference)
	if b.Ref != nil && native == "" {
		native = b.Ref.SessionID
	}
	if native == "" && entry.AttemptID != "" && previousSession == b.SessionID {
		native = previousNative
	}
	b.NativeSessionID, b.Name = native, name
}

// apply folds one entry. It returns the attempt whose origin changed, if any.
func (p *projection) apply(entry hub.HistoryEntry) string {
	if p.first == 0 || entry.Sequence < p.first {
		p.first = entry.Sequence
	}
	p.last = max(p.last, entry.Sequence)
	changed := ""
	if entry.AttemptID != "" && entry.Event.SessionID != "" && p.observeOrigin(entry.AttemptID, entry.SessionID, entry.Event.SessionID) {
		changed = entry.AttemptID
	}
	if entry.Input != "" {
		b := p.upsert(entry.TurnID+"/input", entry, BlockKindUser, BlockStatusCompleted)
		if !b.final {
			b.Text, b.final = entry.Input, true
		}
		return changed
	}
	sequence := strconv.FormatUint(entry.Sequence, 10)
	if entry.Timeline != nil {
		event := entry.Timeline
		b := p.upsert("product/"+sequence, entry, BlockKindTimeline, BlockStatusCompleted)
		b.Name, b.Text, b.HandoffID, b.final = event.Kind, event.Text+"\n"+event.FromBranchID+" → "+event.ToBranchID, event.HandoffID, true
		return changed
	}
	event := entry.Event
	id := entry.AttemptID + "/" + event.ID
	switch event.Kind {
	case engine.ExecutionActivity:
		if a := event.Activity; a != nil {
			known := ""
			if event.SessionID != "" {
				known = " Known native execution identity retained for inspection; this does not say the Agent started."
			}
			b := p.upsert(entry.AttemptID+"/activity/"+a.ID, entry, BlockKindActivity, BlockStatusCompleted)
			b.Name, b.Text, b.final = a.Name, "Committed before activity start; effect="+string(a.Effect)+". This is not a Tool.Execute start/result or proof of completion."+known, true
		}
	case engine.ReasoningDelta, engine.ContentBlock:
		kind := BlockKindReasoning
		if event.Kind == engine.ContentBlock && event.Name != "reasoning" {
			kind = BlockKindContent
		}
		b := p.upsert(id, entry, kind, BlockStatusRunning)
		if b.Name == "" {
			b.Name = event.Name
		}
		if event.Kind == engine.ReasoningDelta {
			b.Text += event.Text
		} else {
			b.Text, b.Status, b.final, b.Truncated, b.Ref = event.Text, BlockStatusCompleted, true, event.Truncated, cloneReference(event.Reference)
		}
	case engine.MessageDelta, engine.Message:
		b := p.upsert(id, entry, BlockKindReply, BlockStatusRunning)
		if event.Kind == engine.Message {
			b.Text, b.Status, b.final, b.Truncated, b.Ref = event.Text, BlockStatusCompleted, true, event.Truncated, cloneReference(event.Reference)
		} else {
			b.Text += event.Text
		}
	case engine.ToolStart:
		b := p.upsert(id, entry, BlockKindTool, BlockStatusRunning)
		rememberTool(b, entry)
		b.Text, b.Status, b.final, b.Truncated = event.Text, BlockStatusRunning, false, false
		b.Tool = foldToolView(b.Tool, b.Name, event)
	case engine.ToolEnd:
		b := p.upsert(id, entry, BlockKindTool, BlockStatusRunning)
		rememberTool(b, entry)
		b.Text, b.Status, b.final, b.Truncated = event.Text, BlockStatusCompleted, true, event.Truncated
		if event.IsError {
			b.Status = BlockStatusFailed
		}
		b.Tool = foldToolView(b.Tool, b.Name, event)
	case engine.Error:
		b := p.upsert(entry.AttemptID+"/error/"+sequence, entry, BlockKindStatus, BlockStatusFailed)
		b.Name, b.Text, b.final = "error", event.Text, true
		if event.Recoverable {
			b.Name = "retry"
		}
	case engine.Completed:
		outcome := BlockStatus(event.Outcome)
		if event.Text != "" {
			b := p.upsert(entry.AttemptID+"/settlement/"+sequence, entry, BlockKindStatus, outcome)
			b.Name, b.Text, b.Status, b.final = "settlement", event.Text, outcome, true
		}
		if entry.AttemptID != "" {
			p.outcomes[entry.AttemptID] = outcome
		}
		for i := range p.blocks {
			if b := &p.blocks[i]; b.AttemptID == entry.AttemptID && b.Kind != BlockKindUser && b.Status == BlockStatusRunning {
				b.Status = outcome
			}
		}
	}
	return changed
}

// markTodos keeps only the latest todo block of each Turn complete: earlier
// ones are marked superseded. A changed flag replaces the shared view value.
func (p *projection) markTodos() {
	latest := map[string]bool{}
	for i := len(p.blocks) - 1; i >= 0; i-- {
		b := &p.blocks[i]
		if b.Tool == nil || b.Tool.Kind != engine.ToolTodo {
			continue
		}
		turn := b.TurnID
		if turn == "" {
			turn = "attempt/" + b.AttemptID
		}
		superseded := latest[turn]
		latest[turn] = true
		if b.Tool.Superseded != superseded {
			view := cloneToolView(*b.Tool)
			view.Superseded = superseded
			b.Tool = &view
		}
	}
}
