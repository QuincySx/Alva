// INPUT: Product branch routing, the registered EngineInspector and committed product history pages
// OUTPUT: TraceService paging (Tail/Older/Newer/Around), TraceTarget/TraceHint/TraceChunk, bounded TraceDetail bodies, raw record pages and literal TraceSearchPage
// POS: [L7 Hub product] Narrow execution-trace reader over Inspector metadata; never a full session read, never restores or executes an Agent
package product

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const (
	traceInspectorPage = 20  // RepositoryInspector's per-call metadata bound
	traceTailRecords   = 200 // first screen and each older page
	traceAroundRecords = 120 // records on each side of a jump target
	traceForwardPages  = 20
	traceProductPages  = 16 // product history pages per anchor scan
	traceBodyBytes     = 64 << 10
)

// TraceTarget names one trace: a product session branch and an optional typed
// child path (at most 8 hops, revalidated on every read). Turn is the parent
// step's product Turn for a child path; child records inherit it.
type TraceTarget struct {
	SessionID, BranchID string
	Path                []ChildHop
	Turn                uint64
}

// Child returns the target of a child step's linked session.
func (t TraceTarget) Child(step TraceStep) (TraceTarget, error) {
	if step.Kind != TraceStepChild || step.ChildLink == 0 || len(t.Path) >= 8 {
		return TraceTarget{}, errors.New("step has no enterable child session or the child depth limit is reached")
	}
	child := t
	child.Path = append(append([]ChildHop(nil), t.Path...), ChildHop{LinkSequence: step.ChildLink})
	child.Turn = step.Turn
	return child, nil
}

// TraceHint binds a jump to product context the caller already holds, such as
// a Message Stream tool block: its Turn and a product history sequence near the
// record. The zero hint reads anchors from the end of product history.
type TraceHint struct {
	Turn    uint64
	History uint64
}

// TraceChunk carries contiguous native metadata and the product anchors read
// with it. Its contents stay private: callers hand it to a TraceWindow.
type TraceChunk struct {
	NativeID string
	// Unsupported reports an engine without an Inspector: there is no trace,
	// and this is not an error.
	Unsupported           bool
	MoreBefore, MoreAfter bool
	HighWater             uint64
	entries               []traceEntry
	anchors               []traceAnchor
	starts                []traceStart
	product               traceProductRange
	inherited             uint64
}

// traceTargets resolves a branch/typed child path to its owned Inspector
// scope. Only HubService implements it, with the existing routing rules.
type traceTargets interface {
	traceTarget(ctx context.Context, id, branchID string, path []ChildHop) (engine.Inspector, engine.InspectionScope, error)
}

// TraceService reads execution-trace pages. It owns no window state.
type TraceService struct {
	history TranscriptStore
	targets traceTargets
}

// NewTraceService binds product history (for Turn anchors) and the service's
// Inspector routing.
func NewTraceService(history TranscriptStore, service *HubService) *TraceService {
	return &TraceService{history: history, targets: service}
}

var errTraceEmpty = errors.New("branch has no native execution records yet")

// traceTarget is inspectionTarget plus an explicit empty state for a branch
// that has not started native execution.
func (s *HubService) traceTarget(ctx context.Context, id, branchID string, path []ChildHop) (engine.Inspector, engine.InspectionScope, error) {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, engine.InspectionScope{}, err
	}
	for _, branch := range detail.Branches {
		if branch.ID != branchID {
			continue
		}
		descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
		// The trace opens from the tail, so it needs an Inspector that declares
		// backward pages; one that might ignore Before is treated as unsupported.
		if !ok || descriptor.Inspector == nil {
			return nil, engine.InspectionScope{}, engine.ErrUnsupported
		}
		if capabilities := descriptor.Inspector.InspectionCapabilities(); !capabilities.Historical || !capabilities.BackwardPages {
			return nil, engine.InspectionScope{}, engine.ErrUnsupported
		}
		if branch.NativeSessionID == "" {
			return nil, engine.InspectionScope{}, errTraceEmpty
		}
		return s.inspectionTarget(ctx, id, branchID, path)
	}
	return nil, engine.InspectionScope{}, hub.ErrNotFound
}

func (s *TraceService) target(ctx context.Context, target TraceTarget) (engine.Inspector, engine.InspectionScope, error) {
	if s == nil || s.targets == nil || s.history == nil {
		return nil, engine.InspectionScope{}, errors.New("trace reader is unavailable")
	}
	return s.targets.traceTarget(ctx, target.SessionID, target.BranchID, target.Path)
}

func traceEntryOf(entry engine.InspectionEntry) traceEntry {
	result := traceEntry{id: entry.ID, sequence: entry.Sequence, time: entry.Time, kind: entry.Kind, namespace: entry.Namespace, kindExt: entry.Type, summary: entry.Summary, uri: entry.Value.URI, bytes: entry.Value.Bytes, requestID: entry.RequestID, runID: entry.RunID, callID: entry.CallID, status: entry.Status, startedAt: entry.StartedAt, endedAt: entry.EndedAt}
	if entry.Parent != nil {
		result.parent = entry.Parent.Sequence
	}
	return result
}

// readBackward reads up to want records strictly before before (InspectionEnd
// for the end), returning them ascending.
func readBackward(ctx context.Context, inspector engine.Inspector, scope engine.InspectionScope, before, highWater uint64, want int) ([]traceEntry, bool, uint64, error) {
	var pages [][]traceEntry
	earlier, total := false, 0
	for total < want {
		page, err := inspector.Page(ctx, scope, engine.InspectionQuery{Before: before, HighWater: highWater, Limit: traceInspectorPage})
		if err != nil {
			return nil, false, 0, err
		}
		highWater = page.HighWater
		entries := make([]traceEntry, len(page.Entries))
		for i, entry := range page.Entries {
			entries[i] = traceEntryOf(entry)
		}
		pages = append(pages, entries)
		total += len(entries)
		earlier = page.HasEarlier
		if !earlier || len(entries) == 0 {
			break
		}
		before = page.Before
	}
	result := make([]traceEntry, 0, total)
	for i := len(pages) - 1; i >= 0; i-- {
		result = append(result, pages[i]...)
	}
	return result, earlier, highWater, nil
}

// readForward reads up to pages metadata pages after after.
func readForward(ctx context.Context, inspector engine.Inspector, scope engine.InspectionScope, after, highWater uint64, pages int) ([]traceEntry, bool, uint64, error) {
	var result []traceEntry
	more := false
	for range pages {
		page, err := inspector.Page(ctx, scope, engine.InspectionQuery{After: after, HighWater: highWater, Limit: traceInspectorPage})
		if err != nil {
			return nil, false, 0, err
		}
		highWater = page.HighWater
		for _, entry := range page.Entries {
			result = append(result, traceEntryOf(entry))
		}
		more = page.HasMore
		if !more || len(page.Entries) == 0 {
			break
		}
		after = page.Next
	}
	return result, more, highWater, nil
}

// productScan is the Turn evidence found in one bounded product history scan.
type productScan struct {
	anchors []traceAnchor
	starts  []traceStart
	product traceProductRange
}

func (p *productScan) observe(entry hub.HistoryEntry, nativeID string) {
	if p.product.low == 0 || entry.Sequence < p.product.low {
		p.product.low = entry.Sequence
	}
	p.product.high = max(p.product.high, entry.Sequence)
	if entry.TurnSequence == 0 {
		return
	}
	if ref := entry.Event.Reference; ref != nil && ref.SessionID == nativeID && ref.Sequence != 0 {
		p.anchors = append(p.anchors, traceAnchor{native: ref.Sequence, turn: entry.TurnSequence, product: entry.Sequence})
	}
	if entry.Event.Kind == engine.SessionStarted && entry.Event.SessionID == nativeID {
		p.starts = append(p.starts, traceStart{turn: entry.TurnSequence, product: entry.Sequence})
	}
}

// scanBackward reads product history before before (0 = the end) until it
// has passed the whole Turn of the first anchor below stopBelow, so every
// anchor at or above stopBelow has been seen even when a Turn's events are not
// in native order. It is bounded by traceProductPages; beyond that, unproven
// Turns stay unknown rather than scanning the whole history.
func (s *TraceService) scanBackward(ctx context.Context, sessionID, nativeID string, before, stopBelow uint64) (productScan, error) {
	var scan productScan
	boundary := uint64(0)
	for range traceProductPages {
		entries, err := s.history.HistoryBefore(ctx, sessionID, before, transcriptPage)
		if err != nil {
			return productScan{}, err
		}
		for i := len(entries) - 1; i >= 0; i-- {
			entry := entries[i]
			if boundary != 0 && entry.TurnSequence != 0 && entry.TurnSequence < boundary {
				return scan, nil
			}
			scan.observe(entry, nativeID)
			if ref := entry.Event.Reference; boundary == 0 && ref != nil && ref.SessionID == nativeID && ref.Sequence != 0 && ref.Sequence < stopBelow && entry.TurnSequence != 0 {
				boundary = entry.TurnSequence
			}
		}
		if len(entries) < transcriptPage {
			scan.product.start = true
			return scan, nil
		}
		before = entries[0].Sequence
	}
	return scan, nil
}

// scanForward reads product history after after until it has passed the whole
// Turn of the first anchor above stopAbove (0 = read to the end), bounded.
func (s *TraceService) scanForward(ctx context.Context, sessionID, nativeID string, after, stopAbove uint64) (productScan, error) {
	var scan productScan
	boundary := uint64(0)
	for range traceForwardPages {
		entries, err := s.history.History(ctx, sessionID, after, transcriptPage)
		if err != nil {
			return productScan{}, err
		}
		for _, entry := range entries {
			if boundary != 0 && entry.TurnSequence > boundary {
				return scan, nil
			}
			scan.observe(entry, nativeID)
			if ref := entry.Event.Reference; stopAbove != 0 && boundary == 0 && ref != nil && ref.SessionID == nativeID && ref.Sequence > stopAbove && entry.TurnSequence != 0 {
				boundary = entry.TurnSequence
			}
		}
		if len(entries) < transcriptPage {
			return scan, nil
		}
		after = entries[len(entries)-1].Sequence
	}
	return scan, nil
}

func (c *TraceChunk) addScan(scan productScan) {
	c.anchors = append(c.anchors, scan.anchors...)
	c.starts = append(c.starts, scan.starts...)
	if scan.product.low != 0 && (c.product.low == 0 || scan.product.low < c.product.low) {
		c.product.low = scan.product.low
	}
	c.product.high = max(c.product.high, scan.product.high)
	c.product.start = c.product.start || scan.product.start
}

// open resolves the target into a chunk shell; ok is false when the chunk is
// already final (unsupported engine or no native records yet).
func (s *TraceService) open(ctx context.Context, target TraceTarget) (engine.Inspector, engine.InspectionScope, TraceChunk, bool, error) {
	inspector, scope, err := s.target(ctx, target)
	switch {
	case errors.Is(err, engine.ErrUnsupported):
		return nil, scope, TraceChunk{Unsupported: true}, false, nil
	case errors.Is(err, errTraceEmpty):
		return nil, scope, TraceChunk{}, false, nil
	case err != nil:
		return nil, scope, TraceChunk{}, false, err
	}
	chunk := TraceChunk{NativeID: scope.NativeID}
	if len(target.Path) > 0 {
		chunk.inherited = target.Turn
	}
	return inspector, scope, chunk, true, nil
}

// Tail reads the newest records and the product anchors that label them.
func (s *TraceService) Tail(ctx context.Context, target TraceTarget) (TraceChunk, error) {
	inspector, scope, chunk, ok, err := s.open(ctx, target)
	if !ok || err != nil {
		return chunk, err
	}
	// Native first: product history read afterwards is never older than it.
	chunk.entries, chunk.MoreBefore, chunk.HighWater, err = readBackward(ctx, inspector, scope, engine.InspectionEnd, 0, traceTailRecords)
	if err != nil || len(chunk.entries) == 0 || len(target.Path) > 0 {
		return chunk, err
	}
	scan, err := s.scanBackward(ctx, target.SessionID, scope.NativeID, 0, chunk.entries[0].sequence)
	chunk.addScan(scan)
	return chunk, err
}

// Older reads records before the cursor's first record.
func (s *TraceService) Older(ctx context.Context, target TraceTarget, cursor TraceCursor) (TraceChunk, error) {
	inspector, scope, chunk, ok, err := s.open(ctx, target)
	if !ok || err != nil || cursor.First <= 1 {
		return chunk, err
	}
	chunk.entries, chunk.MoreBefore, chunk.HighWater, err = readBackward(ctx, inspector, scope, cursor.First, cursor.HighWater, traceTailRecords)
	if err != nil || len(chunk.entries) == 0 || len(target.Path) > 0 || cursor.product.start {
		return chunk, err
	}
	scan, err := s.scanBackward(ctx, target.SessionID, scope.NativeID, cursor.product.low, chunk.entries[0].sequence)
	chunk.addScan(scan)
	return chunk, err
}

// Newer reads records after the cursor's last record against the current
// high-water; an intent paired in a later page is completed by this read.
func (s *TraceService) Newer(ctx context.Context, target TraceTarget, cursor TraceCursor) (TraceChunk, error) {
	inspector, scope, chunk, ok, err := s.open(ctx, target)
	if !ok || err != nil {
		return chunk, err
	}
	chunk.entries, chunk.MoreAfter, chunk.HighWater, err = readForward(ctx, inspector, scope, cursor.Last, 0, traceForwardPages)
	if err != nil || len(target.Path) > 0 {
		return chunk, err
	}
	// Product events can trail their native records; reading forward from the
	// scanned high end also labels records already in the window.
	scan, err := s.scanForward(ctx, target.SessionID, scope.NativeID, cursor.product.high, 0)
	chunk.addScan(scan)
	return chunk, err
}

// Around reads a page on both sides of a native record, for a search hit or a
// Message Stream tool block. The hint binds the record's known Turn and the
// product history next to it; without one, anchors come from the end of
// product history within the same bounded scan.
func (s *TraceService) Around(ctx context.Context, target TraceTarget, sequence uint64, hint TraceHint) (TraceChunk, error) {
	inspector, scope, chunk, ok, err := s.open(ctx, target)
	if !ok || err != nil {
		return chunk, err
	}
	if sequence == 0 {
		return chunk, errors.New("trace jump requires a committed record sequence")
	}
	older, earlier, high, err := readBackward(ctx, inspector, scope, sequence+1, 0, traceAroundRecords)
	if err != nil {
		return chunk, err
	}
	newer, more, _, err := readForward(ctx, inspector, scope, sequence, high, (traceAroundRecords+traceInspectorPage-1)/traceInspectorPage)
	if err != nil {
		return chunk, err
	}
	chunk.entries = append(older, newer...)
	chunk.MoreBefore, chunk.MoreAfter, chunk.HighWater = earlier, more, high
	if len(chunk.entries) == 0 || len(target.Path) > 0 {
		return chunk, nil
	}
	first, last := chunk.entries[0].sequence, chunk.entries[len(chunk.entries)-1].sequence
	if hint.Turn != 0 {
		chunk.anchors = append(chunk.anchors, traceAnchor{native: sequence, turn: hint.Turn, product: hint.History})
	}
	if hint.History == 0 {
		scan, err := s.scanBackward(ctx, target.SessionID, scope.NativeID, 0, first)
		chunk.addScan(scan)
		return chunk, err
	}
	scan, err := s.scanBackward(ctx, target.SessionID, scope.NativeID, hint.History+1, first)
	if err != nil {
		return chunk, err
	}
	chunk.addScan(scan)
	scan, err = s.scanForward(ctx, target.SessionID, scope.NativeID, hint.History, last)
	chunk.addScan(scan)
	return chunk, err
}

// TraceDetail is the readable content of one step, read from record bodies on
// demand. Text fields are bounded to 64 KiB per record; Truncated marks a
// shortened body whose full text stays in the raw record pages.
type TraceDetail struct {
	StepID string
	// NativeID is the native session the step belongs to; ChildID is a child
	// step's linked session.
	NativeID  string
	ChildID   string
	Usage     *wire.Usage
	Error     string
	Arguments string
	Result    string
	Message   string
	Truncated bool
}

// readBody reads a record's JSON through bounded 2 KiB Inspector reads.
func readBody(ctx context.Context, inspector engine.Inspector, scope engine.InspectionScope, uri string) (string, bool, error) {
	var text strings.Builder
	offset := int64(0)
	for text.Len() < traceBodyBytes {
		page, err := inspector.Read(ctx, scope, uri, offset)
		if err != nil {
			return "", false, err
		}
		text.WriteString(page.Text)
		if !page.Truncated || page.NextOffset <= offset {
			return text.String(), false, nil
		}
		offset = page.NextOffset
	}
	return text.String(), true, nil
}

func resultText(result wire.ToolResult) string {
	var parts []string
	for _, block := range result.Content {
		if block.Text != "" {
			parts = append(parts, block.Text)
		}
	}
	return strings.Join(parts, "\n")
}

// Detail reads the bodies of a step's records for its overview and preview.
func (s *TraceService) Detail(ctx context.Context, target TraceTarget, step TraceStep) (TraceDetail, error) {
	inspector, scope, err := s.target(ctx, target)
	if err != nil {
		return TraceDetail{}, err
	}
	detail := TraceDetail{StepID: step.ID, NativeID: scope.NativeID}
	for _, record := range step.Records {
		if record.URI == "" || record.Role == "related" || record.Role == "link" || record.Role == "intent" && step.Kind == TraceStepModel {
			continue
		}
		raw, truncated, err := readBody(ctx, inspector, scope, record.URI)
		if err != nil {
			return detail, err
		}
		var value abi.SessionRecord
		if truncated || json.Unmarshal([]byte(raw), &value) != nil {
			// Too long to decode within the preview budget: show its prefix.
			detail.Truncated = true
			switch record.Role {
			case "intent":
				detail.Arguments = raw
			case "outcome":
				detail.Result = raw
			default:
				detail.Message = raw
			}
			continue
		}
		switch {
		case value.Message != nil:
			detail.Message = value.Message.Text()
		case value.ModelOutcome != nil:
			detail.Usage, detail.Error = value.ModelOutcome.Usage, value.ModelOutcome.Error
		case value.ToolIntent != nil:
			detail.Arguments = string(value.ToolIntent.Call.Arguments)
		case value.ToolOutcome != nil:
			detail.Result, detail.Error = resultText(value.ToolOutcome.Result), value.ToolOutcome.Error
		}
	}
	if step.Kind == TraceStepChild && step.ChildLink != 0 {
		link, err := inspector.Child(ctx, scope, step.ChildLink)
		if err != nil {
			return detail, err
		}
		detail.ChildID = link.ChildID
	}
	return detail, nil
}

// Raw reads one 2 KiB page of a record's committed JSON.
func (s *TraceService) Raw(ctx context.Context, target TraceTarget, uri string, offset int64) (agentcontext.TextPage, error) {
	inspector, scope, err := s.target(ctx, target)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	page, err := inspector.Read(ctx, scope, uri, offset)
	return agentcontext.TextPage{URI: page.URI, Text: page.Text, Offset: page.Offset, NextOffset: page.NextOffset, TotalBytes: page.TotalBytes, Truncated: page.Truncated}, err
}

// TraceHit is one literal search match: the record to jump to.
type TraceHit struct {
	Sequence uint64
	Kind     string
	Type     string
	Summary  string
	Time     time.Time
}

// TraceSearchPage is one bounded forward literal-search page. Continue with
// Next and the same HighWater while HasMore; a page may be empty yet HasMore.
type TraceSearchPage struct {
	Hits            []TraceHit
	Next, HighWater uint64
	HasMore         bool
}

// Search pages a literal search through the trace's records, oldest first.
func (s *TraceService) Search(ctx context.Context, target TraceTarget, query string, after, highWater uint64) (TraceSearchPage, error) {
	inspector, scope, err := s.target(ctx, target)
	if err != nil {
		return TraceSearchPage{}, err
	}
	page, err := inspector.Page(ctx, scope, engine.InspectionQuery{After: after, HighWater: highWater, Limit: traceInspectorPage, Literal: query})
	if err != nil {
		return TraceSearchPage{}, err
	}
	result := TraceSearchPage{Next: page.Next, HighWater: page.HighWater, HasMore: page.HasMore}
	for _, entry := range page.Entries {
		result.Hits = append(result.Hits, TraceHit{Sequence: entry.Sequence, Kind: entry.Kind, Type: entry.Type, Summary: entry.Summary, Time: entry.Time})
	}
	return result, nil
}
