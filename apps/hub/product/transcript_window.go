// INPUT: TranscriptChunk pages, the shared history projection and tool views
// OUTPUT: TranscriptWindow block accumulation, boundary merge (tool views included), superseded todo marking and byte-bounded trimming
// POS: [L7 Hub product] UI-thread Message Stream window without I/O; counts blocks and bytes, never events
package product

import (
	"math"

	"github.com/QuincySx/Alva/apps/hub"
)

const transcriptDefaultBytes = 64 << 20

// blockOverhead approximates a block's fixed memory beyond its strings.
const blockOverhead = 256

// TranscriptWindow accumulates content blocks for one session view. It is used
// only on the UI thread and performs no I/O; TranscriptService reads pages.
type TranscriptWindow struct {
	limit      int
	view       *projection
	moreBefore bool
	moreAfter  bool
}

// NewTranscriptWindow bounds the window's estimated block bytes; a
// non-positive limit selects the 64 MiB default.
func NewTranscriptWindow(limitBytes int) *TranscriptWindow {
	if limitBytes <= 0 {
		limitBytes = transcriptDefaultBytes
	}
	return &TranscriptWindow{limit: limitBytes, view: newProjection()}
}

// Reset replaces the window with a Tail chunk.
func (w *TranscriptWindow) Reset(chunk TranscriptChunk) {
	w.view = newProjection()
	w.moreBefore, w.moreAfter = chunk.More, false
	for _, entry := range chunk.entries {
		w.fold(entry)
	}
	w.view.markTodos()
}

// ApplyNewer appends history after Last. Deltas keep extending their block and
// a final message replaces it with the complete text.
func (w *TranscriptWindow) ApplyNewer(chunk TranscriptChunk) {
	for _, entry := range chunk.entries {
		if w.view.last == 0 || entry.Sequence > w.view.last {
			w.fold(entry)
		}
	}
	w.moreAfter = chunk.More
	w.view.markTodos()
}

func (w *TranscriptWindow) fold(entry hub.HistoryEntry) {
	if attempt := w.view.apply(entry); attempt != "" {
		w.view.attributeAttempt(attempt)
	}
}

// ApplyOlder prepends history before First. A block split by the page edge
// keeps its current position: final current text wins, otherwise the older
// text is joined in front.
func (w *TranscriptWindow) ApplyOlder(chunk TranscriptChunk) {
	w.moreBefore = chunk.More
	older := newProjection()
	for _, entry := range chunk.entries {
		if w.view.first == 0 || entry.Sequence < w.view.first {
			older.apply(entry)
		}
	}
	if len(older.blocks) == 0 && older.first == 0 {
		return
	}
	for attempt, origin := range older.origins {
		current, exists := w.view.origins[attempt]
		if !exists {
			w.view.origins[attempt] = origin
		} else if !current.ambiguous && (origin.ambiguous || origin.sessionID != current.sessionID || origin.nativeID != current.nativeID) {
			current.ambiguous = true
			w.view.origins[attempt] = current
		}
	}
	for attempt, outcome := range older.outcomes {
		if _, exists := w.view.outcomes[attempt]; !exists {
			w.view.outcomes[attempt] = outcome
		}
	}
	kept := make([]blockState, 0, len(older.blocks)+len(w.view.blocks))
	for _, b := range older.blocks {
		if i, exists := w.view.index[b.ID]; exists {
			w.view.blocks[i] = mergeOlder(b, w.view.blocks[i])
			continue
		}
		// The attempt's Completed may already be in the window.
		if outcome, settled := w.view.outcomes[b.AttemptID]; settled && b.Kind != BlockKindUser && b.Status == BlockStatusRunning {
			b.Status = outcome
		}
		kept = append(kept, b)
	}
	w.view.blocks = append(kept, w.view.blocks...)
	w.view.reindex()
	for i := range w.view.blocks {
		w.view.attribute(&w.view.blocks[i])
	}
	if w.view.first == 0 || older.first < w.view.first {
		w.view.first = older.first
	}
	w.view.last = max(w.view.last, older.last)
	w.view.markTodos()
}

// mergeOlder folds an older part of the same block into its current part, as
// if the older events had been projected first.
func mergeOlder(older, current blockState) blockState {
	merged := current
	merged.Kind = older.Kind
	if !current.final {
		merged.Text = older.Text + current.Text
		if older.final {
			merged.final, merged.Truncated = true, older.Truncated
			if merged.Ref == nil {
				merged.Ref = older.Ref
			}
			if merged.Status == BlockStatusRunning {
				merged.Status = older.Status
			}
		}
	}
	if merged.Kind == BlockKindTool && merged.NativeSessionID == "" && merged.AttemptID != "" && older.SessionID == merged.SessionID {
		merged.NativeSessionID = older.NativeSessionID
	}
	if merged.Name == "" {
		merged.Name = older.Name
	}
	if merged.Kind == BlockKindTool {
		merged.Tool = mergeToolView(older.Tool, current.Tool)
	}
	if merged.Turn == 0 {
		merged.Turn = older.Turn
	}
	merged.first = min(older.first, current.first)
	return merged
}

func (p *projection) reindex() {
	p.index = make(map[string]int, len(p.blocks))
	for i := range p.blocks {
		p.index[p.blocks[i].ID] = i
	}
}

// Blocks returns a detached copy of the window's blocks in display order.
func (w *TranscriptWindow) Blocks() []Block {
	blocks := make([]Block, len(w.view.blocks))
	for i := range w.view.blocks {
		blocks[i] = w.view.blocks[i].Block
		blocks[i].Ref = cloneReference(blocks[i].Ref)
		blocks[i].History = w.view.blocks[i].last
	}
	return blocks
}

// First is the lowest committed sequence the window covers; 0 when empty.
func (w *TranscriptWindow) First() uint64 { return w.view.first }

// Last is the highest committed sequence the window covers; 0 when empty.
func (w *TranscriptWindow) Last() uint64 { return w.view.last }

// MoreBefore reports that earlier history exists outside the window.
func (w *TranscriptWindow) MoreBefore() bool { return w.moreBefore }

// MoreAfter reports that newer history exists outside the window; readers stop
// following new text until they reload the latest page.
func (w *TranscriptWindow) MoreAfter() bool { return w.moreAfter }

func blockBytes(b *Block) int {
	size := blockOverhead + len(b.ID) + len(b.SessionID) + len(b.TurnID) + len(b.AttemptID) + len(b.Name) + len(b.Text) + len(b.NativeSessionID) + len(b.HandoffID) + toolViewBytes(b.Tool)
	if b.Ref != nil {
		size += len(b.Ref.SessionID) + len(b.Ref.RecordID) + len(b.Ref.URI) + 8
	}
	return size
}

func (w *TranscriptWindow) bytes() int {
	total := 0
	for i := range w.view.blocks {
		total += blockBytes(&w.view.blocks[i].Block)
	}
	return total
}

// Trim drops blocks at the end away from the viewport while the window is over
// its byte limit: the oldest when atEnd, otherwise the newest. Running blocks
// and the live Turn they belong to are never dropped. A cut is taken only where
// every dropped block's events precede every kept block's events, so paging
// back across it rebuilds the dropped blocks whole.
func (w *TranscriptWindow) Trim(atEnd bool) {
	blocks := w.view.blocks
	n := len(blocks)
	if n < 2 {
		return
	}
	sizes := make([]int, n)
	total := 0
	for i := range blocks {
		sizes[i] = blockBytes(&blocks[i].Block)
		total += sizes[i]
	}
	if total <= w.limit {
		return
	}
	live := ""
	for i := n - 1; i >= 0; i-- {
		if blocks[i].Status == BlockStatusRunning {
			live = blocks[i].TurnID
			break
		}
	}
	protected := func(b *blockState) bool {
		return b.Status == BlockStatusRunning || live != "" && b.TurnID == live
	}
	if atEnd {
		suffixMin := make([]uint64, n+1)
		suffixMin[n] = math.MaxUint64
		for i := n - 1; i >= 0; i-- {
			suffixMin[i] = min(suffixMin[i+1], blocks[i].first)
		}
		removed, prefixMax, cut := 0, uint64(0), 0
		for k := 0; k < n-1 && !protected(&blocks[k]); k++ {
			removed += sizes[k]
			prefixMax = max(prefixMax, blocks[k].last)
			if prefixMax < suffixMin[k+1] {
				cut = k + 1
				if total-removed <= w.limit {
					break
				}
			}
		}
		if cut == 0 {
			return
		}
		w.view.blocks = append([]blockState(nil), blocks[cut:]...)
		w.view.first = suffixMin[cut]
		w.moreBefore = true
	} else {
		prefixMax := make([]uint64, n+1)
		for i := 0; i < n; i++ {
			prefixMax[i+1] = max(prefixMax[i], blocks[i].last)
		}
		removed, suffixMin, cut := 0, uint64(math.MaxUint64), n
		for k := n - 1; k >= 1 && !protected(&blocks[k]); k-- {
			removed += sizes[k]
			suffixMin = min(suffixMin, blocks[k].first)
			if prefixMax[k] < suffixMin {
				cut = k
				if total-removed <= w.limit {
					break
				}
			}
		}
		if cut == n {
			return
		}
		w.view.blocks = append([]blockState(nil), blocks[:cut]...)
		w.view.last = prefixMax[cut]
		w.moreAfter = true
	}
	w.view.reindex()
}
