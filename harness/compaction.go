// INPUT: bytes, context, crypto/rand, encoding/hex/json, errors, fmt, math, strings, sync, time, unicode/utf8, shared storage contracts, agent/abi, agent/context, agent/core, kernel/abi, llm/wire
// OUTPUT: CompactionConfig, ContextReport (usage, parts, compaction), SessionOption/WithCompaction and the replaceable context-compaction Plugin
// POS: [L5 harness] Durable summary compaction over the bounded native tail: summaries are typed native facts, the working view is a pure function of them.
package harness

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// DefaultContextWindow is the compaction budget's model window when no
// trusted declaration exists. It is a trigger budget, never shown as a fact.
const DefaultContextWindow = 128_000

const (
	contextNamespace      = "alva.context"
	summaryKind           = "summary"
	compactionIntentKind  = "compaction_intent"
	compactionFailedKind  = "compaction_failed"
	summaryTextLimit      = 16 << 10
	summaryPayloadLimit   = 32 << 10
	summaryReadLimit      = 64 << 10
	summaryBlockLimit     = 2 << 10
	summaryInputRatio     = 0.6
	summaryMessagePrefix  = "context/summary/"
	compactionErrorLimit  = 1 << 10
	defaultTriggerRatio   = 0.8
	defaultKeepRatio      = 0.3
	defaultSummaryTokens  = 4096
	minimumSummaryInput   = 2048
	calibrationLowerBound = 0.5
	calibrationUpperBound = 2.0
)

// CompactionConfig is the summary compaction policy of one native session.
// Zero values select the defaults; Disabled keeps the plain bounded
// recent-turn source and never reads or writes summaries.
type CompactionConfig struct {
	// WindowTokens is the model's declared context window; 0 uses
	// DefaultContextWindow for the trigger and reports the window as undeclared.
	WindowTokens     int
	TriggerRatio     float64
	KeepRecentRatio  float64
	SummaryMaxTokens int
	// Force compacts before this execution's first model request.
	Force    bool
	Disabled bool
	// Report receives measurements and compaction outcomes, in execution order.
	Report func(context.Context, ContextReport) error
}

func (c CompactionConfig) window() int {
	if c.WindowTokens > 0 {
		return c.WindowTokens
	}
	return DefaultContextWindow
}
func (c CompactionConfig) normalized() (CompactionConfig, error) {
	if c.WindowTokens < 0 || c.SummaryMaxTokens < 0 || c.TriggerRatio < 0 || c.TriggerRatio > 1 || c.KeepRecentRatio < 0 || c.KeepRecentRatio >= 1 || math.IsNaN(c.TriggerRatio) || math.IsNaN(c.KeepRecentRatio) {
		return c, errors.New("invalid context compaction configuration")
	}
	if c.TriggerRatio == 0 {
		c.TriggerRatio = defaultTriggerRatio
	}
	if c.KeepRecentRatio == 0 {
		c.KeepRecentRatio = defaultKeepRatio
	}
	if c.KeepRecentRatio >= c.TriggerRatio {
		return c, errors.New("compaction must keep less than its trigger budget")
	}
	if c.SummaryMaxTokens == 0 {
		c.SummaryMaxTokens = defaultSummaryTokens
	}
	return c, nil
}

// ContextParts splits the measured context by source. Each part is an
// estimated share of the actual measured total, never an independent count.
type ContextParts struct{ System, Tools, Summary, Recent uint64 }

// ContextUsage is the provider-reported size of the latest model request
// (inclusive input, as wire.Usage) plus its output, which joins the context.
type ContextUsage struct {
	InputTokens       uint64
	OutputTokens      uint64
	CachedInputTokens *uint64
	// WindowTokens is 0 when the model window is undeclared.
	WindowTokens uint64
	Parts        *ContextParts
}

type CompactionStatus string

const (
	CompactionSucceeded CompactionStatus = "compacted"
	CompactionFailed    CompactionStatus = "failed"
	// CompactionSkipped: a requested compaction found nothing to summarize.
	CompactionSkipped CompactionStatus = "skipped"
	// SummaryCorrupt: the latest summary record could not be used; the view
	// fell back to bounded recent turns and the record is kept unchanged.
	SummaryCorrupt CompactionStatus = "summary_corrupt"
)

type CompactionTrigger string

const (
	TriggerAutomatic CompactionTrigger = "auto"
	TriggerManual    CompactionTrigger = "manual"
	TriggerOverflow  CompactionTrigger = "overflow"
)

// CompactionReport says what one compaction did. Record names the summary
// record written (or the corrupt one found); Usage is the summary request's own.
type CompactionReport struct {
	Status                        CompactionStatus
	Trigger                       CompactionTrigger
	FromSequence, ThroughSequence uint64
	Messages                      int
	Record                        *abi.RecordReference
	Usage                         *wire.Usage
	Error                         string
}

// ContextReport carries a measurement, a compaction outcome, or both.
type ContextReport struct {
	Usage      *ContextUsage
	Compaction *CompactionReport
}

// SessionOption adjusts SessionPlugins/SessionViews recipes.
type SessionOption func(*sessionOptions)

type sessionOptions struct{ compaction CompactionConfig }

// WithCompaction replaces the default compaction policy of the native session.
func WithCompaction(config CompactionConfig) SessionOption {
	return func(options *sessionOptions) { options.compaction = config }
}

// summarySnapshot is the durable alva.context/summary payload (schema 1).
type summarySnapshot struct {
	Summary          string `json:"summary"`
	FromSequence     uint64 `json:"from_sequence"`
	ThroughSequence  uint64 `json:"through_sequence"`
	PinnedSequence   uint64 `json:"pinned_sequence,omitempty"`
	PreviousSequence uint64 `json:"previous_sequence,omitempty"`
	// EarlierOmitted says conversation records before FromSequence exist and
	// were never summarized (an old session or a broken chain).
	EarlierOmitted bool        `json:"earlier_omitted,omitempty"`
	HighWater      uint64      `json:"high_water"`
	Messages       int         `json:"messages"`
	Trigger        string      `json:"trigger"`
	PromptVersion  string      `json:"prompt_version"`
	ProviderID     string      `json:"provider_id,omitempty"`
	ModelID        string      `json:"model_id,omitempty"`
	Usage          *wire.Usage `json:"usage,omitempty"`
}

func (s summarySnapshot) validate(sequence uint64) error {
	switch {
	case s.Summary == "" || len(s.Summary) > summaryTextLimit || !utf8.ValidString(s.Summary):
		return errors.New("summary text missing, invalid or over 16 KiB")
	case s.FromSequence == 0 || s.FromSequence > s.ThroughSequence || s.ThroughSequence > s.HighWater || s.HighWater >= sequence:
		return errors.New("summary range is not ordered before its own record")
	case s.PinnedSequence > s.ThroughSequence:
		return errors.New("summary pinned input lies after its range")
	case s.PreviousSequence >= sequence || s.Messages < 1:
		return errors.New("summary chain or message count invalid")
	case s.PromptVersion == "" || len(s.PromptVersion) > 128:
		return errors.New("summary prompt version missing")
	}
	switch CompactionTrigger(s.Trigger) {
	case TriggerAutomatic, TriggerManual, TriggerOverflow:
	default:
		return errors.New("summary trigger unknown")
	}
	return nil
}

type loadedSummary struct {
	sequence  uint64
	recordID  string
	reference wire.Reference
	payload   summarySnapshot
}

type corruptSummary struct {
	sequence uint64
	recordID string
	reason   string
}

type viewEntry struct {
	id       string
	sequence uint64
}

// contextView is the composition of the latest Load: the source's prefix
// messages and the conversation records it restored, oldest first.
type contextView struct {
	loaded    bool
	highWater uint64
	prefix    []string
	entries   []viewEntry
	summary   *loadedSummary
	// latest is the newest summary record sequence (valid or corrupt), the
	// compare-and-append expectation for the next summary.
	latest  uint64
	corrupt *corruptSummary
	// gap says conversation records between the summary (or the session
	// start) and the first restored entry are omitted, not summarized.
	gap bool
}

// compactionRun is per-execution state, reset at run start.
type compactionRun struct {
	ratio                           float64
	forced, failed, overflowRetried bool
	corruptReported                 uint64
	estimate                        int
	parts                           ContextParts
}

type compactionPlugin struct {
	repository SessionStorage
	owner, id  string
	config     CompactionConfig
	estimator  agentabi.SimpleTokenEstimator

	mu   sync.Mutex
	view contextView
	run  compactionRun
}

func newCompactionPlugin(repository SessionStorage, owner, id string, config CompactionConfig) (*compactionPlugin, error) {
	normalized, err := config.normalized()
	if err != nil {
		return nil, err
	}
	return &compactionPlugin{repository: repository, owner: owner, id: id, config: normalized, run: compactionRun{ratio: 1}}, nil
}

func (*compactionPlugin) Name() string { return "context-compaction" }
func (p *compactionPlugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.ContextSource(p); err != nil {
		return err
	}
	// Before pending interjections (-500) and request projections, so they
	// apply to the compacted view; after the run claim and recovery evidence.
	return r.Middleware(compactionMiddleware{plugin: p}, -1000)
}

// Load restores the latest valid summary plus every conversation record after
// it that fits the bounded tail. Without a summary it restores the whole
// bounded tail; a corrupt summary visibly falls back to the plain recent turns.
func (p *compactionPlugin) Load(ctx context.Context, log abi.ConversationReader) ([]wire.Message, error) {
	if log == nil || nilImplementation(log) || log.ID() != p.id {
		return nil, errors.New("native context source belongs to a different session")
	}
	messages, view, err := p.load(ctx, false)
	if err != nil {
		return nil, err
	}
	p.mu.Lock()
	p.view = view
	p.mu.Unlock()
	return messages, nil
}

func (p *compactionPlugin) load(ctx context.Context, plain bool) ([]wire.Message, contextView, error) {
	view := contextView{loaded: true}
	tail, err := p.repository.BoundConversationTail(ctx, p.owner, p.id, contextMaxMessages)
	if err != nil {
		return nil, view, err
	}
	metadata, err := tail.Metadata()
	if err != nil {
		return nil, view, err
	}
	view.highWater = tail.HighWater()
	summary, latest, corrupt, err := p.readSummary(ctx)
	if err != nil {
		return nil, view, err
	}
	view.latest, view.corrupt = latest, corrupt
	if plain {
		summary = nil
	}
	if len(metadata) == 0 {
		return nil, view, nil
	}
	if summary == nil {
		target := contextMaxMessages
		if plain || corrupt != nil {
			target = contextTargetMessages
		}
		count := selectRecent(metadata, len(metadata), target)
		if count == 0 {
			return nil, view, errLatestTurnTooLarge
		}
		messages, err := tail.Read(ctx, count, contextMaxBytes)
		if err != nil {
			return nil, view, err
		}
		view.entries = entriesOf(messages, metadata[:count])
		if count < len(metadata) {
			notice := omissionNotice(p.id, metadata[count], view.highWater)
			view.prefix, view.gap = []string{notice.ID}, true
			messages = append([]wire.Message{notice}, messages...)
		}
		return messages, view, nil
	}
	view.summary = summary
	through := summary.payload.ThroughSequence
	after := 0
	for after < len(metadata) && metadata[after].Record.Sequence > through {
		after++
	}
	count := selectRecent(metadata, after, after)
	if count == 0 && after > 0 {
		return nil, view, errLatestTurnTooLarge
	}
	read, pinned := count, -1
	if pin := summary.payload.PinnedSequence; pin != 0 && count == after {
		bytes := int64(0)
		for i := 0; i < len(metadata) && i < contextMaxMessages; i++ {
			bytes += metadata[i].RestoreBytes
			if bytes > contextMaxBytes || metadata[i].Record.Sequence < pin {
				break
			}
			if metadata[i].Record.Sequence == pin && metadata[i].Role == wire.RoleUser {
				read, pinned = i+1, i
				break
			}
		}
	}
	restored, err := tail.Read(ctx, read, contextMaxBytes)
	if err != nil {
		return nil, view, err
	}
	entries := entriesOf(restored, metadata[:read])
	if pinned >= 0 {
		// Keep the pinned input (oldest restored) and the records after the
		// summary; the summary replaces everything between them.
		restored = append(restored[:1:1], restored[len(restored)-count:]...)
		entries = append(entries[:1:1], entries[len(entries)-count:]...)
	}
	message := summaryMessage(p.id, *summary)
	view.prefix = []string{message.ID}
	messages := []wire.Message{message}
	if count < after {
		notice := omissionNotice(p.id, metadata[count], view.highWater)
		view.prefix, view.gap = append(view.prefix, notice.ID), true
		messages = append(messages, notice)
	}
	view.entries = entries
	return append(messages, restored...), view, nil
}

// entriesOf pairs restored messages (oldest first) with their metadata
// (newest first).
func entriesOf(messages []wire.Message, metadata []storage.ConversationMetadata) []viewEntry {
	entries := make([]viewEntry, len(messages))
	for i, message := range messages {
		entries[i] = viewEntry{id: message.ID, sequence: metadata[len(metadata)-1-i].Record.Sequence}
	}
	return entries
}

// readSummary returns the latest valid summary, the latest summary record
// sequence and, when that record is unusable, why. Only storage failures are
// errors; a malformed record is reported, kept and never overwritten.
func (p *compactionPlugin) readSummary(ctx context.Context) (*loadedSummary, uint64, *corruptSummary, error) {
	entry, found, err := p.repository.LatestRecord(ctx, p.owner, p.id, contextNamespace, summaryKind)
	if err != nil || !found {
		return nil, 0, nil, err
	}
	corrupt := func(reason string) (*loadedSummary, uint64, *corruptSummary, error) {
		return nil, entry.Sequence, &corruptSummary{sequence: entry.Sequence, recordID: entry.ID, reason: reason}, nil
	}
	if entry.Kind != abi.RecordExtension || entry.SchemaVersion != 1 {
		return corrupt("unsupported summary record envelope")
	}
	_, data, err := p.repository.ReadArtifactFull(ctx, p.owner, p.id, entry.Value.URI, summaryReadLimit)
	if err != nil {
		if ctx.Err() != nil {
			return nil, 0, nil, ctx.Err()
		}
		return corrupt("summary record body unreadable")
	}
	var record abi.SessionRecord
	if json.Unmarshal(data, &record) != nil || record.Validate() != nil || record.Kind != abi.RecordExtension || record.Namespace != contextNamespace || record.Type != summaryKind || record.SchemaVersion != 1 || record.ID != entry.ID || record.Sequence != entry.Sequence {
		return corrupt("summary record identity or envelope mismatch")
	}
	var payload summarySnapshot
	decoder := json.NewDecoder(bytes.NewReader(record.Payload))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&payload) != nil {
		return corrupt("summary payload malformed")
	}
	if err := payload.validate(entry.Sequence); err != nil {
		return corrupt(err.Error())
	}
	return &loadedSummary{sequence: entry.Sequence, recordID: entry.ID, reference: entry.Value, payload: payload}, entry.Sequence, nil, nil
}

// summaryMessage is a pure function of the durable summary, so a rewritten
// request and every later Load present byte-identical context.
func summaryMessage(sessionID string, summary loadedSummary) wire.Message {
	s := summary.payload
	var text strings.Builder
	fmt.Fprintf(&text, "Summary of earlier conversation records #%d–#%d (%d messages), written by context compaction at high-water %d. It replaces those records in this working view; the original records are unchanged and remain readable with native_history and context_read (the reference below is the summary record).", s.FromSequence, s.ThroughSequence, s.Messages, s.HighWater)
	if s.EarlierOmitted {
		fmt.Fprintf(&text, " Conversation records before #%d were omitted earlier and are not summarized.", s.FromSequence)
	}
	if s.PinnedSequence != 0 {
		fmt.Fprintf(&text, " The current task's input (record #%d) follows verbatim.", s.PinnedSequence)
	}
	text.WriteString("\n\n")
	text.WriteString(s.Summary)
	reference := summary.reference
	return wire.Message{ID: fmt.Sprintf("%s%s/%d", summaryMessagePrefix, sessionID, summary.sequence), Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text.String()}, {Kind: wire.ContentReference, Reference: &reference}}}
}

func (p *compactionPlugin) report(ctx context.Context, report ContextReport) error {
	if p.config.Report == nil {
		return nil
	}
	return p.config.Report(ctx, report)
}

// estimate is the calibrated local size of a request: the CJK-aware heuristic
// scaled by this run's last actual/estimated ratio.
func (p *compactionPlugin) estimate(request abi.ModelRequest) int {
	tokens, err := p.estimator.EstimateMessages(request.Messages)
	if err != nil {
		tokens = 0
		for _, message := range request.Messages {
			encoded, _ := json.Marshal(message.Content)
			tokens += p.estimator.EstimateText(string(encoded))
		}
	}
	if len(request.Tools) > 0 {
		encoded, _ := json.Marshal(request.Tools)
		tokens += p.estimator.EstimateText(string(encoded))
	}
	p.mu.Lock()
	ratio := p.run.ratio
	p.mu.Unlock()
	return int(math.Ceil(float64(tokens) * ratio))
}

var errNothingToCompact = errors.New("no earlier conversation can be summarized")

// compaction is one successful compaction's effect on the current request.
type compaction struct {
	remove map[string]bool
	insert []wire.Message
	report CompactionReport
}

// compact summarizes the view's earlier records, appends the summary as a
// typed native fact and returns how to rewrite the current request. It never
// overwrites a summary: a concurrent newer record fails the compare-and-append.
func (p *compactionPlugin) compact(ctx context.Context, model abi.LanguageModel, request abi.ModelRequest, trigger CompactionTrigger) (compaction, error) {
	result := compaction{report: CompactionReport{Status: CompactionFailed, Trigger: trigger}}
	p.mu.Lock()
	view := p.view
	p.mu.Unlock()
	if !view.loaded || len(view.entries) == 0 {
		return result, errNothingToCompact
	}
	byID := make(map[string]int, len(request.Messages))
	for i, message := range request.Messages {
		if message.ID != "" {
			if _, duplicate := byID[message.ID]; duplicate {
				return result, errors.New("working view has duplicate message identities")
			}
			byID[message.ID] = i
		}
	}
	conversation := make([]wire.Message, len(view.entries))
	for i, entry := range view.entries {
		index, ok := byID[entry.id]
		if !ok {
			return result, errors.New("working view changed before compaction")
		}
		conversation[i] = request.Messages[index]
	}
	window := p.config.window()
	plan, ok := agentcontext.PlanCompaction(conversation, int(float64(window)*p.config.KeepRecentRatio), p.estimator)
	if !ok {
		return result, errNothingToCompact
	}
	var summarized []int
	for i := 0; i < plan.Cut; i++ {
		if i != plan.Pinned {
			summarized = append(summarized, i)
		}
	}
	if len(summarized) == 0 {
		return result, errNothingToCompact
	}
	previous := view.summary
	budget := int(float64(window) * summaryInputRatio)
	if previous != nil {
		budget -= p.estimator.EstimateText(previous.payload.Summary)
	}
	budget = max(budget, minimumSummaryInput)
	render := func(index int) string {
		entry, message := view.entries[index], conversation[index]
		label := fmt.Sprintf("[#%d %s]", entry.sequence, message.Role)
		if index == plan.Pinned {
			label = fmt.Sprintf("[#%d %s — current task, kept verbatim after this summary]", entry.sequence, message.Role)
		}
		return label + "\n" + agentcontext.RenderForSummary(message, summaryBlockLimit)
	}
	first, used := len(summarized), 0
	for first > 0 {
		cost := p.estimator.EstimateText(render(summarized[first-1]))
		if used+cost > budget && first < len(summarized) {
			break
		}
		used += cost
		first--
	}
	included := summarized[first:]
	// A gap in the view or in the input breaks the chain: start a new one
	// instead of claiming the old summary covers what it never saw.
	if view.gap || first > 0 {
		previous = nil
	}
	// Messages counts every conversation message the chain has summarized,
	// matching the from–through range the summary message names.
	from, covered := view.entries[included[0]].sequence, len(included)
	previousText, previousSequence := "", uint64(0)
	earlierOmitted := view.gap || first > 0
	if previous != nil {
		from, previousText, previousSequence = previous.payload.FromSequence, previous.payload.Summary, previous.sequence
		earlierOmitted, covered = previous.payload.EarlierOmitted, covered+previous.payload.Messages
	}
	through := view.entries[plan.Cut-1].sequence
	pinned := uint64(0)
	if plan.Pinned >= 0 {
		pinned = view.entries[plan.Pinned].sequence
	}
	// Chronological input; the pinned task stays visible to the summarizer.
	selected := make(map[int]bool, len(included))
	for _, index := range included {
		selected[index] = true
	}
	lines := make([]string, 0, len(included)+1)
	for i := 0; i < plan.Cut; i++ {
		if selected[i] || i == plan.Pinned {
			lines = append(lines, render(i))
		}
	}
	result.report.FromSequence, result.report.ThroughSequence, result.report.Messages = from, through, covered
	requestID, err := newCompactionID()
	if err != nil {
		return result, err
	}
	log, err := p.repository.OpenSession(ctx, p.owner, p.id)
	if err != nil {
		return result, err
	}
	intent, _ := json.Marshal(map[string]any{"request_id": requestID, "trigger": trigger, "from_sequence": from, "through_sequence": through, "new_messages": len(included), "prompt_version": agentcontext.SummaryPromptVersion})
	if _, err := log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: contextNamespace, Type: compactionIntentKind, SchemaVersion: 1, Payload: intent}}); err != nil {
		return result, fmt.Errorf("commit compaction intent: %w", err)
	}
	fail := func(cause error) (compaction, error) {
		result.report.Error = boundedError(cause.Error())
		failed, _ := json.Marshal(map[string]any{"request_id": requestID, "error": result.report.Error})
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		_, recordErr := log.AppendRecords(cleanup, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: contextNamespace, Type: compactionFailedKind, SchemaVersion: 1, Payload: failed}})
		return result, errors.Join(cause, recordErr)
	}
	summaryRequest := agentcontext.SummaryRequest(previousText, strings.Join(lines, "\n\n"), uint32(min(p.config.SummaryMaxTokens, math.MaxUint32)), request.Config)
	text, usage, err := agentcontext.CollectSummary(ctx, model, summaryRequest, summaryTextLimit)
	if err != nil {
		return fail(err)
	}
	result.report.Usage = usage
	snapshot := summarySnapshot{Summary: text, FromSequence: from, ThroughSequence: through, PinnedSequence: pinned, PreviousSequence: previousSequence, EarlierOmitted: earlierOmitted, HighWater: view.highWater, Messages: covered, Trigger: string(trigger), PromptVersion: agentcontext.SummaryPromptVersion, Usage: usage}
	if identity, ok := model.(abi.ModelIdentity); ok {
		snapshot.ProviderID, snapshot.ModelID = boundedText(identity.ProviderID(), 128), boundedText(identity.ModelID(), 256)
	}
	payload, err := json.Marshal(snapshot)
	if err != nil || len(payload) > summaryPayloadLimit {
		return fail(errors.New("summary payload exceeds 32 KiB"))
	}
	record, err := log.AppendExtensionSnapshot(ctx, contextNamespace, summaryKind, view.latest, payload)
	if err != nil {
		if errors.Is(err, storage.ErrRevisionConflict) {
			return fail(errors.New("a newer summary was written concurrently; nothing was overwritten"))
		}
		return fail(err)
	}
	entry, found, err := p.repository.LatestRecord(ctx, p.owner, p.id, contextNamespace, summaryKind)
	if err != nil || !found || entry.Sequence != record.Sequence {
		return result, errors.Join(errors.New("written summary could not be read back"), err)
	}
	loaded := loadedSummary{sequence: record.Sequence, recordID: record.ID, reference: entry.Value, payload: snapshot}
	result.remove = map[string]bool{}
	for _, id := range view.prefix {
		result.remove[id] = true
	}
	for i := 0; i < plan.Cut; i++ {
		result.remove[view.entries[i].id] = true
	}
	result.insert = []wire.Message{summaryMessage(p.id, loaded)}
	entries := []viewEntry{}
	if plan.Pinned >= 0 {
		result.insert = append(result.insert, conversation[plan.Pinned])
		entries = append(entries, view.entries[plan.Pinned])
	}
	entries = append(entries, view.entries[plan.Cut:]...)
	result.report.Status = CompactionSucceeded
	result.report.Record = &abi.RecordReference{SessionID: p.id, RecordID: record.ID, Sequence: record.Sequence}
	p.mu.Lock()
	p.view = contextView{loaded: true, highWater: view.highWater, prefix: []string{result.insert[0].ID}, entries: entries, summary: &loaded, latest: record.Sequence}
	p.mu.Unlock()
	return result, nil
}

// replaceView removes the given messages and inserts replacement at the
// position of the first removed one, keeping every other message in place.
func replaceView(messages []wire.Message, remove map[string]bool, insert []wire.Message) []wire.Message {
	out := make([]wire.Message, 0, len(messages)+len(insert))
	inserted := false
	for _, message := range messages {
		if message.ID != "" && remove[message.ID] {
			if !inserted {
				out = append(out, wire.CloneMessages(insert)...)
				inserted = true
			}
			continue
		}
		out = append(out, message)
	}
	if !inserted {
		position := 0
		for position < len(out) && out[position].Role == wire.RoleSystem {
			position++
		}
		out = append(out[:position], append(wire.CloneMessages(insert), out[position:]...)...)
	}
	return out
}

// fallback re-projects the request to the plain bounded recent turns after a
// failed compaction, so an oversized summary-less view is not sent as is.
func (p *compactionPlugin) fallback(ctx context.Context, request *abi.ModelRequest) error {
	p.mu.Lock()
	previous := p.view
	p.mu.Unlock()
	messages, view, err := p.load(ctx, true)
	if err != nil {
		return err
	}
	remove := map[string]bool{}
	for _, id := range previous.prefix {
		remove[id] = true
	}
	for _, entry := range previous.entries {
		remove[entry.id] = true
	}
	request.Messages = replaceView(request.Messages, remove, messages)
	p.mu.Lock()
	p.view = view
	p.mu.Unlock()
	return nil
}

func newCompactionID() (string, error) {
	var value [12]byte
	if _, err := rand.Read(value[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(value[:]), nil
}

func boundedText(text string, limit int) string {
	if len(text) <= limit {
		return text
	}
	cut := limit
	for cut > 0 && !utf8.RuneStart(text[cut]) {
		cut--
	}
	return text[:cut]
}
func boundedError(text string) string { return boundedText(text, compactionErrorLimit) }

// compactionMiddleware triggers compaction before a request, retries one
// classified context overflow, and measures each actual request.
type compactionMiddleware struct{ plugin *compactionPlugin }

func (compactionMiddleware) Name() string { return "context-compaction" }

func (m compactionMiddleware) OnRunStart(context.Context, abi.RuntimeContext) error {
	p := m.plugin
	p.mu.Lock()
	p.run = compactionRun{ratio: 1}
	p.mu.Unlock()
	return nil
}

func (m compactionMiddleware) BeforeModel(ctx context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	p := m.plugin
	if runtime.State == nil || runtime.State.Session().ID() != p.id {
		return errors.New("context compaction does not belong to this runtime session")
	}
	p.mu.Lock()
	corrupt, reported := p.view.corrupt, p.run.corruptReported
	force := p.config.Force && !p.run.forced
	p.run.forced = true
	failed := p.run.failed
	p.mu.Unlock()
	if corrupt != nil && reported != corrupt.sequence {
		p.mu.Lock()
		p.run.corruptReported = corrupt.sequence
		p.mu.Unlock()
		reference := &abi.RecordReference{SessionID: p.id, RecordID: corrupt.recordID, Sequence: corrupt.sequence}
		if err := p.report(ctx, ContextReport{Compaction: &CompactionReport{Status: SummaryCorrupt, Trigger: TriggerAutomatic, Record: reference, Error: boundedError(corrupt.reason)}}); err != nil {
			return err
		}
	}
	trigger := TriggerAutomatic
	over := float64(p.estimate(*request)) > float64(p.config.window())*p.config.TriggerRatio
	switch {
	case force:
		trigger = TriggerManual
	case !over || failed:
		return nil
	}
	result, err := p.compact(ctx, runtime.State.Model(), *request, trigger)
	if err == nil {
		request.Messages = replaceView(request.Messages, result.remove, result.insert)
		return p.report(ctx, ContextReport{Compaction: &result.report})
	}
	if ctxErr := ctx.Err(); ctxErr != nil {
		return errors.Join(err, ctxErr)
	}
	if errors.Is(err, errNothingToCompact) {
		if trigger == TriggerManual {
			return p.report(ctx, ContextReport{Compaction: &CompactionReport{Status: CompactionSkipped, Trigger: trigger}})
		}
		return nil
	}
	p.mu.Lock()
	p.run.failed = true
	p.mu.Unlock()
	if result.report.Error == "" {
		result.report.Error = boundedError(err.Error())
	}
	result.report.Status = CompactionFailed
	if over {
		if fallbackErr := p.fallback(ctx, request); fallbackErr != nil {
			return errors.Join(err, fallbackErr)
		}
	}
	return p.report(ctx, ContextReport{Compaction: &result.report})
}

// WrapModel retries once, within the same request, after a reliably
// classified context-length rejection that published nothing: it forces one
// compaction and sends the rewritten request. Anything else fails as before.
func (m compactionMiddleware) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	p := m.plugin
	published := false
	observe := func(chunk wire.StreamChunk) error {
		published = true
		return emit(chunk)
	}
	err := next(ctx, runtime, request, observe)
	if err == nil || published || !contextOverflow(err) || ctx.Err() != nil {
		return err
	}
	p.mu.Lock()
	retried, failed := p.run.overflowRetried, p.run.failed
	p.run.overflowRetried = true
	p.mu.Unlock()
	if retried || failed {
		return err
	}
	model := runtime.InvocationModel
	if model == nil && runtime.State != nil {
		model = runtime.State.Model()
	}
	result, compactErr := p.compact(ctx, model, request, TriggerOverflow)
	if compactErr != nil {
		if errors.Is(compactErr, errNothingToCompact) || ctx.Err() != nil {
			return err
		}
		p.mu.Lock()
		p.run.failed = true
		p.mu.Unlock()
		if result.report.Error == "" {
			result.report.Error = boundedError(compactErr.Error())
		}
		result.report.Status = CompactionFailed
		return errors.Join(err, p.report(ctx, ContextReport{Compaction: &result.report}))
	}
	if reportErr := p.report(ctx, ContextReport{Compaction: &result.report}); reportErr != nil {
		return errors.Join(err, reportErr)
	}
	request.Messages = replaceView(request.Messages, result.remove, result.insert)
	return next(ctx, runtime, request, emit)
}

// contextOverflow is true only for a provider rejection classified from a
// structured provider field; free error text is never interpreted here.
func contextOverflow(err error) bool {
	var provider *agentabi.ProviderError
	return errors.As(err, &provider) && provider.Kind == agentabi.ProviderContextLength
}

// AdmitModel measures the final request after every wrapper. It only
// observes; it never rejects or changes the request.
func (m compactionMiddleware) AdmitModel(_ context.Context, _ abi.RuntimeContext, request abi.ModelRequest) error {
	p := m.plugin
	var parts ContextParts
	for _, message := range request.Messages {
		tokens, err := p.estimator.EstimateMessages([]wire.Message{message})
		if err != nil {
			continue
		}
		switch {
		case message.Role == wire.RoleSystem:
			parts.System += uint64(tokens)
		case strings.HasPrefix(message.ID, summaryMessagePrefix):
			parts.Summary += uint64(tokens)
		default:
			parts.Recent += uint64(tokens)
		}
	}
	if len(request.Tools) > 0 {
		encoded, _ := json.Marshal(request.Tools)
		parts.Tools = uint64(p.estimator.EstimateText(string(encoded)))
	}
	p.mu.Lock()
	p.run.parts, p.run.estimate = parts, int(parts.System+parts.Tools+parts.Summary+parts.Recent)
	p.mu.Unlock()
	return nil
}

// AfterModel reports the provider-reported size of the request just answered
// and recalibrates the local estimate. Without reported usage nothing is shown.
func (m compactionMiddleware) AfterModel(ctx context.Context, _ abi.RuntimeContext, message *wire.Message) error {
	p := m.plugin
	if message == nil || message.Usage == nil || message.Usage.InputTokens == 0 {
		return nil
	}
	usage := *message.Usage
	p.mu.Lock()
	parts, estimate := p.run.parts, p.run.estimate
	if estimate > 0 {
		p.run.ratio = min(max(float64(usage.InputTokens)/float64(estimate), calibrationLowerBound), calibrationUpperBound)
	}
	p.mu.Unlock()
	report := ContextUsage{InputTokens: usage.InputTokens, OutputTokens: usage.OutputTokens}
	if usage.CachedInputTokens != nil {
		cached := *usage.CachedInputTokens
		report.CachedInputTokens = &cached
	}
	if p.config.WindowTokens > 0 {
		report.WindowTokens = uint64(p.config.WindowTokens)
	}
	if total := parts.System + parts.Tools + parts.Summary + parts.Recent; total > 0 {
		scale := func(part uint64) uint64 { return uint64(float64(part) / float64(total) * float64(usage.InputTokens)) }
		scaled := ContextParts{System: scale(parts.System), Tools: scale(parts.Tools), Summary: scale(parts.Summary)}
		scaled.Recent = usage.InputTokens - scaled.System - scaled.Tools - scaled.Summary + usage.OutputTokens
		report.Parts = &scaled
	}
	return p.report(ctx, ContextReport{Usage: &report})
}

var _ abi.ContextSource = (*compactionPlugin)(nil)
