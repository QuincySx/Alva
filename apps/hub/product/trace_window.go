// INPUT: TraceChunk native metadata pages and product turn anchors
// OUTPUT: TraceStep/TraceStepKind/TraceRecord projection and TraceWindow accumulation, pairing, turn labels and byte-bounded trimming that releases only the trimmed side's product evidence
// POS: [L7 Hub product] UI-thread execution-trace window without I/O; steps are rebuilt from metadata only, never record bodies
package product

import (
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
)

// TraceStepKind is the role of one trace step.
type TraceStepKind string

const (
	TraceStepInput TraceStepKind = "input"
	TraceStepModel TraceStepKind = "model"
	TraceStepTool  TraceStepKind = "tool"
	TraceStepChild TraceStepKind = "child"
)

// TraceRecord is one committed native record a step folds in. Role names its
// part: "input", "intent", "outcome", "message", "link" or "related".
type TraceRecord struct {
	Sequence uint64
	Role     string
	Kind     string
	Type     string
	URI      string
	Bytes    int64
	Time     time.Time
}

// TraceStep is one row of a session's execution trace, built from record
// metadata only.
//
// Turn is the product Turn ordinal shown by the Message Stream (0 when the
// window cannot prove it). Ordinal is the step's 1-based position within its
// Turn; OrdinalExact is false when the Turn starts before the window. Start/End
// are tool execution instants when recorded, otherwise record commit times and
// Approximate is set. Open marks a model/tool call whose outcome is not in the
// window yet. First/Last bound the native sequences folded into the step.
type TraceStep struct {
	ID           string
	Kind         TraceStepKind
	Turn         uint64
	Ordinal      int
	OrdinalExact bool
	Name         string
	Summary      string
	Status       string
	Start, End   time.Time
	Approximate  bool
	Open         bool
	First, Last  uint64
	RequestID    string
	RunID        string
	CallID       string
	// ChildLink is a child step's link record sequence, the ChildHop to enter.
	ChildLink uint64
	Records   []TraceRecord
}

// Duration is End-Start, zero for points and open calls.
func (s TraceStep) Duration() time.Duration {
	if s.Open || s.Start.IsZero() || s.End.IsZero() || s.End.Before(s.Start) {
		return 0
	}
	return s.End.Sub(s.Start)
}

// Record returns the step's record with the given role.
func (s TraceStep) Record(role string) (TraceRecord, bool) {
	for _, record := range s.Records {
		if record.Role == role {
			return record, true
		}
	}
	return TraceRecord{}, false
}

// traceEntry is the compact native metadata a window retains per record.
type traceEntry struct {
	id                       string
	sequence, parent         uint64
	time                     time.Time
	kind, namespace, kindExt string
	summary                  string
	uri                      string
	bytes                    int64
	requestID, runID, callID string
	status                   string
	startedAt, endedAt       time.Time
}

func (e *traceEntry) size() int {
	return 160 + len(e.id) + len(e.kind) + len(e.namespace) + len(e.kindExt) + len(e.summary) + len(e.uri) + len(e.requestID) + len(e.runID) + len(e.callID) + len(e.status)
}

// role is a conversation record's message role from its stored summary.
func (e *traceEntry) role() string {
	if e.kind != "conversation" {
		return ""
	}
	role, _, _ := strings.Cut(e.summary, ": ")
	return role
}

func (e *traceEntry) text() string {
	if _, text, found := strings.Cut(e.summary, ": "); found && e.kind == "conversation" {
		return text
	}
	return e.summary
}

func (e *traceEntry) userInput() bool { return e.role() == "user" }

func (e *traceEntry) childLink() bool {
	return e.kind == "extension" && e.namespace == "alva.agent" && e.kindExt == "child_session_linked"
}

func (e *traceEntry) record(role string) TraceRecord {
	return TraceRecord{Sequence: e.sequence, Role: role, Kind: e.kind, Type: e.kindExt, URI: e.uri, Bytes: e.bytes, Time: e.time}
}

// traceAnchor proves that the native record at native belongs to product Turn
// turn: a committed product event at product sequence product referenced it.
type traceAnchor struct{ native, turn, product uint64 }

// traceStart is a product Attempt starting on this native session.
type traceStart struct{ turn, product uint64 }

const traceDefaultBytes = 4 << 20

// TraceWindow accumulates native record metadata for one trace view and
// rebuilds its steps after every change. It is used only on the UI thread and
// performs no I/O; TraceService reads pages.
type TraceWindow struct {
	limit       int
	nativeID    string
	unsupported bool
	entries     []traceEntry
	moreBefore  bool
	moreAfter   bool
	highWater   uint64
	anchors     []traceAnchor
	starts      []traceStart
	product     traceProductRange
	inherited   uint64
	steps       []TraceStep
	index       map[string]int
}

// traceProductRange is the inclusive product history span already scanned
// for anchors, and whether its older end is the start of history.
type traceProductRange struct {
	low, high uint64
	start     bool
}

// NewTraceWindow bounds the window's estimated metadata bytes; a non-positive
// limit selects the 4 MiB default.
func NewTraceWindow(limitBytes int) *TraceWindow {
	if limitBytes <= 0 {
		limitBytes = traceDefaultBytes
	}
	return &TraceWindow{limit: limitBytes, index: map[string]int{}}
}

// TraceCursor is the window position the service continues from.
type TraceCursor struct {
	First, Last, HighWater uint64
	product                traceProductRange
}

// Cursor returns the window's paging position.
func (w *TraceWindow) Cursor() TraceCursor {
	return TraceCursor{First: w.First(), Last: w.Last(), HighWater: w.highWater, product: w.product}
}

// Reset replaces the window with a Tail or Around chunk.
func (w *TraceWindow) Reset(chunk TraceChunk) {
	w.nativeID, w.unsupported, w.inherited = chunk.NativeID, chunk.Unsupported, chunk.inherited
	w.entries = slices.Clone(chunk.entries)
	w.moreBefore, w.moreAfter = chunk.MoreBefore, chunk.MoreAfter
	w.highWater = chunk.HighWater
	w.anchors, w.starts = nil, nil
	w.product = chunk.product
	w.addAnchors(chunk.anchors, chunk.starts)
	w.rebuild()
}

// ApplyOlder prepends records before First and the anchors read with them.
func (w *TraceWindow) ApplyOlder(chunk TraceChunk) {
	if chunk.NativeID != w.nativeID {
		return
	}
	first := w.First()
	var older []traceEntry
	for _, entry := range chunk.entries {
		if first == 0 || entry.sequence < first {
			older = append(older, entry)
		}
	}
	w.entries = append(older, w.entries...)
	w.moreBefore = chunk.MoreBefore
	if chunk.product.low != 0 && (w.product.low == 0 || chunk.product.low < w.product.low) {
		w.product.low = chunk.product.low
	}
	w.product.high = max(w.product.high, chunk.product.high)
	w.product.start = w.product.start || chunk.product.start
	w.addAnchors(chunk.anchors, chunk.starts)
	w.rebuild()
}

// ApplyNewer appends records after Last and the anchors read with them; a
// pending intent pairs with its outcome here.
func (w *TraceWindow) ApplyNewer(chunk TraceChunk) {
	if w.nativeID == "" && len(w.entries) == 0 && !w.unsupported {
		// A branch opened before its first native run adopts the session it
		// started, together with that run's inherited Turn.
		w.nativeID, w.inherited = chunk.NativeID, chunk.inherited
	}
	if chunk.NativeID != w.nativeID {
		return
	}
	last := w.Last()
	for _, entry := range chunk.entries {
		if entry.sequence > last {
			w.entries = append(w.entries, entry)
			last = entry.sequence
		}
	}
	w.moreAfter = chunk.MoreAfter
	w.highWater = max(w.highWater, chunk.HighWater)
	if chunk.product.low != 0 && (w.product.low == 0 || chunk.product.low < w.product.low) {
		w.product.low = chunk.product.low
	}
	w.product.high = max(w.product.high, chunk.product.high)
	w.addAnchors(chunk.anchors, chunk.starts)
	w.rebuild()
}

func (w *TraceWindow) addAnchors(anchors []traceAnchor, starts []traceStart) {
	seen := make(map[uint64]bool, len(w.anchors))
	for _, anchor := range w.anchors {
		seen[anchor.native] = true
	}
	for _, anchor := range anchors {
		if !seen[anchor.native] && anchor.turn != 0 {
			w.anchors = append(w.anchors, anchor)
			seen[anchor.native] = true
		}
	}
	sort.Slice(w.anchors, func(i, j int) bool { return w.anchors[i].native < w.anchors[j].native })
	known := make(map[uint64]bool, len(w.starts))
	for _, start := range w.starts {
		known[start.product] = true
	}
	for _, start := range starts {
		if !known[start.product] && start.turn != 0 {
			w.starts = append(w.starts, start)
			known[start.product] = true
		}
	}
	sort.Slice(w.starts, func(i, j int) bool { return w.starts[i].product < w.starts[j].product })
}

// NativeID is the native session this window shows.
func (w *TraceWindow) NativeID() string { return w.nativeID }

// Unsupported reports that the branch's engine provides no Inspector.
func (w *TraceWindow) Unsupported() bool { return w.unsupported }

// First is the lowest native sequence in the window; 0 when empty.
func (w *TraceWindow) First() uint64 {
	if len(w.entries) == 0 {
		return 0
	}
	return w.entries[0].sequence
}

// Last is the highest native sequence in the window; 0 when empty.
func (w *TraceWindow) Last() uint64 {
	if len(w.entries) == 0 {
		return 0
	}
	return w.entries[len(w.entries)-1].sequence
}

// MoreBefore reports earlier records outside the window.
func (w *TraceWindow) MoreBefore() bool { return w.moreBefore }

// MoreAfter reports newer records outside the window; readers reload the tail.
func (w *TraceWindow) MoreAfter() bool { return w.moreAfter }

// Records is the number of native records the window holds.
func (w *TraceWindow) Records() int { return len(w.entries) }

// Bytes is the window's estimated retained metadata size.
func (w *TraceWindow) Bytes() int {
	total := 0
	for i := range w.entries {
		total += w.entries[i].size()
	}
	return total + 24*len(w.anchors) + 16*len(w.starts)
}

// Steps returns a detached copy of the steps in record order.
func (w *TraceWindow) Steps() []TraceStep {
	steps := make([]TraceStep, len(w.steps))
	for i, step := range w.steps {
		steps[i] = step
		steps[i].Records = slices.Clone(step.Records)
	}
	return steps
}

// Step returns the step with id.
func (w *TraceWindow) Step(id string) (TraceStep, bool) {
	i, exists := w.index[id]
	if !exists {
		return TraceStep{}, false
	}
	step := w.steps[i]
	step.Records = slices.Clone(step.Records)
	return step, true
}

// StepAt returns the step folding in the native record at sequence.
func (w *TraceWindow) StepAt(sequence uint64) (TraceStep, bool) {
	for _, step := range w.steps {
		for _, record := range step.Records {
			if record.Sequence == sequence {
				return w.Step(step.ID)
			}
		}
	}
	// A record that is not itself a step part (telemetry, a reply message)
	// selects the nearest step that started before it.
	best := -1
	for i, step := range w.steps {
		if step.First <= sequence && (best < 0 || step.First >= w.steps[best].First) {
			best = i
		}
	}
	if best < 0 || sequence < w.First() || sequence > w.Last() {
		return TraceStep{}, false
	}
	return w.Step(w.steps[best].ID)
}

// Trim drops records at the end away from the viewport while the window is
// over its byte limit: the oldest when atEnd, otherwise the newest. Only the
// trimmed side's product evidence is released: that side's scanned product
// bound moves to the nearest kept anchor and anchors/starts beyond it are
// dropped, so the next page on that side rescans exactly what it needs. The
// other side keeps its anchors and bound.
func (w *TraceWindow) Trim(atEnd bool) {
	total := w.Bytes()
	if total <= w.limit || len(w.entries) < 2 {
		return
	}
	cut := 0
	if atEnd {
		for cut < len(w.entries)-1 && total > w.limit {
			total -= w.entries[cut].size()
			cut++
		}
		w.entries = slices.Clone(w.entries[cut:])
		w.moreBefore = true
	} else {
		cut = len(w.entries)
		for cut > 1 && total > w.limit {
			cut--
			total -= w.entries[cut].size()
		}
		w.entries = slices.Clone(w.entries[:cut])
		w.moreAfter = true
	}
	w.releaseTrimmedEvidence(atEnd)
	w.rebuild()
}

// releaseTrimmedEvidence narrows the scanned product range on the trimmed
// side. Without a kept anchor there is no proven bound, so nothing is
// released and the range stays as scanned.
func (w *TraceWindow) releaseTrimmedEvidence(atEnd bool) {
	first, last := w.First(), w.Last()
	bound := uint64(0)
	for _, anchor := range w.anchors {
		if anchor.product == 0 || anchor.native < first || anchor.native > last {
			continue
		}
		if atEnd && (bound == 0 || anchor.product < bound) || !atEnd && anchor.product > bound {
			bound = anchor.product
		}
	}
	if bound == 0 || atEnd && bound <= w.product.low || !atEnd && w.product.high != 0 && bound >= w.product.high {
		return
	}
	released := func(product, native uint64) bool {
		if product == 0 {
			// A caller-supplied hint has no product position: keep it only
			// while its record is in the window or on the untrimmed side.
			return atEnd && native < first || !atEnd && native > last
		}
		return atEnd && product < bound || !atEnd && product > bound
	}
	anchors := w.anchors[:0]
	for _, anchor := range w.anchors {
		if !released(anchor.product, anchor.native) {
			anchors = append(anchors, anchor)
		}
	}
	w.anchors = anchors
	starts := w.starts[:0]
	for _, start := range w.starts {
		if !(atEnd && start.product < bound || !atEnd && start.product > bound) {
			starts = append(starts, start)
		}
	}
	w.starts = starts
	if atEnd {
		w.product.low, w.product.start = bound, false
	} else {
		w.product.high = bound
	}
}

// rebuild pairs records into steps, labels Turns and numbers steps.
func (w *TraceWindow) rebuild() {
	entries := w.entries
	position := make(map[uint64]int, len(entries))
	for i := range entries {
		position[entries[i].sequence] = i
	}
	var steps []TraceStep
	models := map[string]int{}
	tools := map[string]int{}
	for i := range entries {
		entry := &entries[i]
		switch {
		case entry.userInput():
			steps = append(steps, TraceStep{ID: "input/" + strconv.FormatUint(entry.sequence, 10), Kind: TraceStepInput, Name: "user", Summary: entry.text(), Start: entry.time, End: entry.time, First: entry.sequence, Last: entry.sequence, Records: []TraceRecord{entry.record("input")}})
		case entry.kind == "model_intent":
			models[entry.requestID] = len(steps)
			steps = append(steps, TraceStep{ID: "model/" + entry.requestID, Kind: TraceStepModel, Name: "model", Summary: "模型调用", Status: "running", Start: entry.time, Approximate: true, Open: true, First: entry.sequence, Last: entry.sequence, RequestID: entry.requestID, Records: []TraceRecord{entry.record("intent")}})
		case entry.kind == "model_outcome":
			index, paired := models[entry.requestID]
			if !paired {
				continue // its intent is outside the window; it returns with it
			}
			step := &steps[index]
			step.Status, step.End, step.Open, step.Last = entry.status, entry.time, false, entry.sequence
			step.Records = append(step.Records, entry.record("outcome"))
			for _, near := range []uint64{entry.sequence - 1, entry.sequence + 1} {
				if j, exists := position[near]; exists && entries[j].role() == "assistant" {
					step.Records = append(step.Records, entries[j].record("message"))
					if text := entries[j].text(); text != "" {
						step.Summary = text
					}
					break
				}
			}
			for j := position[step.First] + 1; j < i; j++ {
				if related := &entries[j]; related.kind == "extension" && related.namespace == "alva.kernel.audit" && strings.HasPrefix(related.kindExt, "llm_") {
					step.Records = append(step.Records, related.record("related"))
				}
			}
		case entry.kind == "tool_intent":
			key := entry.runID + "/" + entry.callID
			tools[key] = len(steps)
			name, _, _ := strings.Cut(entry.summary, " ")
			steps = append(steps, TraceStep{ID: "tool/" + key, Kind: TraceStepTool, Name: name, Summary: name, Status: "running", Start: entry.time, Approximate: true, Open: true, First: entry.sequence, Last: entry.sequence, RunID: entry.runID, CallID: entry.callID, Records: []TraceRecord{entry.record("intent")}})
		case entry.kind == "tool_outcome":
			index, paired := tools[entry.runID+"/"+entry.callID]
			if !paired {
				continue
			}
			step := &steps[index]
			step.Status, step.End, step.Open, step.Last = entry.status, entry.time, false, entry.sequence
			if !entry.startedAt.IsZero() && !entry.endedAt.IsZero() {
				step.Start, step.End, step.Approximate = entry.startedAt, entry.endedAt, false
			}
			step.Records = append(step.Records, entry.record("outcome"))
		case entry.childLink():
			name := "子 Agent"
			if j, exists := position[entry.parent]; exists && entries[j].kind == "tool_intent" {
				tool, _, _ := strings.Cut(entries[j].summary, " ")
				name = tool
			}
			steps = append(steps, TraceStep{ID: "child/" + strconv.FormatUint(entry.sequence, 10), Kind: TraceStepChild, Name: name, Summary: "进入子 Agent 会话", Start: entry.time, End: entry.time, First: entry.sequence, Last: entry.sequence, ChildLink: entry.sequence, Records: []TraceRecord{entry.record("link")}})
		}
	}
	turns := w.labelTurns()
	counts := map[uint64]int{}
	exact := map[uint64]bool{}
	for i := range steps {
		step := &steps[i]
		step.Turn = turns[position[step.First]]
		if step.Turn == 0 {
			continue
		}
		if _, started := counts[step.Turn]; !started {
			// Numbering is exact only when the window holds the Turn's
			// beginning: the session start, or a directly preceding step of a
			// known other Turn.
			exact[step.Turn] = i == 0 && !w.moreBefore || i > 0 && steps[i-1].Turn != 0 && steps[i-1].Turn != step.Turn
		}
		counts[step.Turn]++
		step.Ordinal, step.OrdinalExact = counts[step.Turn], exact[step.Turn]
	}
	w.steps = steps
	w.index = make(map[string]int, len(steps))
	for i := range steps {
		w.index[steps[i].ID] = i
	}
}

// labelTurns maps each window record to its product Turn. A native run starts
// at a user input record; every record of the run shares the Turn of any
// product anchor inside it. Runs without an anchor (a call that failed before
// any reply or tool) take a Turn only when it is unambiguous: equal Turns on
// both sides, or exactly as many Attempts started on this native session
// between the neighbours as there are unlabelled runs. Otherwise they stay 0.
func (w *TraceWindow) labelTurns() []uint64 {
	n := len(w.entries)
	labels := make([]uint64, n)
	if n == 0 {
		return labels
	}
	if w.inherited != 0 {
		for i := range labels {
			labels[i] = w.inherited
		}
		return labels
	}
	type run struct {
		from, to int // entry index range [from, to)
		turn     uint64
	}
	var runs []run
	for i := 0; i < n; i++ {
		if i == 0 || w.entries[i].userInput() {
			runs = append(runs, run{from: i})
		}
	}
	for r := range runs {
		runs[r].to = n
		if r+1 < len(runs) {
			runs[r].to = runs[r+1].from
		}
		lo, hi := w.entries[runs[r].from].sequence, w.entries[runs[r].to-1].sequence
		at := sort.Search(len(w.anchors), func(i int) bool { return w.anchors[i].native >= lo })
		if at < len(w.anchors) && w.anchors[at].native <= hi {
			runs[r].turn = w.anchors[at].turn
		}
	}
	// Anchors just outside the window bound the edge runs' neighbours.
	before, after := uint64(0), uint64(0)
	if first := w.entries[0].sequence; len(w.anchors) > 0 {
		if at := sort.Search(len(w.anchors), func(i int) bool { return w.anchors[i].native >= first }); at > 0 {
			before = w.anchors[at-1].turn
		}
	}
	if last := w.entries[n-1].sequence; len(w.anchors) > 0 {
		if at := sort.Search(len(w.anchors), func(i int) bool { return w.anchors[i].native > last }); at < len(w.anchors) {
			after = w.anchors[at].turn
		}
	}
	for r := 0; r < len(runs); {
		if runs[r].turn != 0 {
			r++
			continue
		}
		end := r
		for end < len(runs) && runs[end].turn == 0 {
			end++
		}
		// Neighbours inside the window are adjacent runs. A neighbour taken
		// from an anchor outside the window leaves unseen runs in between, so
		// it only confirms equal Turns on both sides.
		left, right := before, after
		leftAdjacent, rightAdjacent := !w.moreBefore, !w.moreAfter
		if r > 0 {
			left, leftAdjacent = runs[r-1].turn, true
		}
		if end < len(runs) {
			right, rightAdjacent = runs[end].turn, true
		}
		atTail := end == len(runs) && !w.moreAfter
		var candidates []uint64
		seen := map[uint64]bool{}
		for _, start := range w.starts {
			if start.turn > left && (right == 0 && atTail || start.turn < right) && !seen[start.turn] {
				seen[start.turn] = true
				candidates = append(candidates, start.turn)
			}
		}
		switch {
		case left != 0 && left == right:
			for k := r; k < end; k++ {
				runs[k].turn = left
			}
		case leftAdjacent && rightAdjacent && (right != 0 || atTail) && len(candidates) == end-r:
			for k := r; k < end; k++ {
				runs[k].turn = candidates[k-r]
			}
		case leftAdjacent && atTail && left != 0 && len(candidates) == 0:
			// A retry of the last Turn adds a run but no new Turn.
			for k := r; k < end; k++ {
				runs[k].turn = left
			}
		}
		r = end
	}
	for _, run := range runs {
		for i := run.from; i < run.to; i++ {
			labels[i] = run.turn
		}
	}
	return labels
}
