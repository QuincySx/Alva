// INPUT: context, encoding/json, errors, fmt, path/filepath, reflect, regexp, sort, strconv, strings, testing, storage/session, agent/abi, kernel/abi, llm/wire
// OUTPUT: Summary compaction regressions over a real native SQLite session with fake models
// POS: [L5 harness testing] Proves chained summaries keep early facts, replay is deterministic, failures and corruption degrade visibly without overwriting, and overflow retries once.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// fullReadGuard is the run's log; any whole-history read fails the test.
type fullReadGuard struct {
	storage.Session
	t *testing.T
}

func (g fullReadGuard) Messages(context.Context) ([]wire.Message, error) {
	g.t.Error("compaction run restored the whole conversation")
	return nil, errors.New("whole conversation read")
}

type compactionFixture struct {
	t         *testing.T
	dir       string
	repo      storage.Repository
	log       storage.Session
	reports   []ContextReport
	mains     []abi.ModelRequest
	summaries []abi.ModelRequest
	summarize func(abi.ModelRequest) (string, error)
	main      func(abi.ModelRequest) error
	reply     func(abi.ModelRequest) []wire.StreamChunk
	tools     []abi.Tool
}

func newCompactionFixture(t *testing.T) *compactionFixture {
	t.Helper()
	dir := filepath.Join(t.TempDir(), "facts")
	repo, err := session.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { repo.Close() })
	log, err := repo.Create(t.Context(), "owner", "compacting", "")
	if err != nil {
		t.Fatal(err)
	}
	return &compactionFixture{t: t, dir: dir, repo: repo, log: log, summarize: factSummary}
}

var factPattern = regexp.MustCompile(`FACT-\d+`)

// factSummary is a deterministic summarizer: it keeps every FACT-n marker
// from the previous summary and the transcript, in numeric order.
func factSummary(request abi.ModelRequest) (string, error) {
	seen := map[string]bool{}
	var facts []string
	for _, fact := range factPattern.FindAllString(request.Messages[len(request.Messages)-1].Text(), -1) {
		if !seen[fact] {
			seen[fact] = true
			facts = append(facts, fact)
		}
	}
	sort.Slice(facts, func(i, j int) bool {
		a, _ := strconv.Atoi(strings.TrimPrefix(facts[i], "FACT-"))
		b, _ := strconv.Atoi(strings.TrimPrefix(facts[j], "FACT-"))
		return a < b
	})
	return "Summary facts: " + strings.Join(facts, " "), nil
}

func isSummaryRequest(request abi.ModelRequest) bool {
	return len(request.Messages) > 0 && request.Messages[0].Role == wire.RoleSystem && strings.HasPrefix(request.Messages[0].Text(), "You compress an agent conversation")
}

func textReply(text string, input uint64) []wire.StreamChunk {
	return []wire.StreamChunk{{Kind: wire.ChunkText, Text: text}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: input, OutputTokens: 5}}, {Kind: wire.ChunkFinish, FinishReason: "stop"}}
}

func (f *compactionFixture) model() abi.LanguageModel {
	return sourceModelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		var chunks []wire.StreamChunk
		if isSummaryRequest(request) {
			f.summaries = append(f.summaries, request)
			if len(request.Tools) != 0 || !request.Config.DisableTools {
				f.t.Error("summary request offered tools")
			}
			text, err := f.summarize(request)
			if err != nil {
				return err
			}
			chunks = []wire.StreamChunk{{Kind: wire.ChunkText, Text: text}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: 100, OutputTokens: 20}}, {Kind: wire.ChunkFinish, FinishReason: "stop"}}
		} else {
			f.mains = append(f.mains, abi.ModelRequest{Messages: wire.CloneMessages(request.Messages), Tools: request.Tools})
			validProtocol(f.t, request.Messages)
			if f.main != nil {
				if err := f.main(request); err != nil {
					return err
				}
			}
			estimate, _ := agentabi.SimpleTokenEstimator{}.EstimateMessages(request.Messages)
			if f.reply != nil {
				chunks = f.reply(request)
			} else {
				chunks = textReply("answer", uint64(estimate))
			}
		}
		for _, chunk := range chunks {
			if err := emit(chunk); err != nil {
				return err
			}
		}
		return nil
	})
}

// validProtocol fails on a tool result whose call is not earlier in the request.
func validProtocol(t *testing.T, messages []wire.Message) {
	t.Helper()
	calls := map[string]bool{}
	for _, message := range messages {
		for _, block := range message.Content {
			if block.ToolCall != nil {
				calls[block.ToolCall.ID] = true
			}
			if block.ToolResult != nil && !calls[block.ToolResult.CallID] {
				t.Errorf("request orphaned tool result %s", block.ToolResult.CallID)
			}
		}
	}
}

func (f *compactionFixture) run(config CompactionConfig, input string, iterations ...int) error {
	f.t.Helper()
	config.Report = func(_ context.Context, report ContextReport) error {
		f.reports = append(f.reports, report)
		return nil
	}
	plugins, err := SessionViews(f.t.Context(), f.repo, "owner", f.log.ID(), WithCompaction(config))
	if err != nil {
		f.t.Fatal(err)
	}
	builder := NewBuilder(f.model(), fullReadGuard{Session: f.log, t: f.t}).Workspace(f.t.TempDir())
	if len(iterations) > 0 {
		builder.MaxIterations(iterations[0])
	}
	for _, tool := range f.tools {
		builder.Tool(tool)
	}
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	agent, err := builder.Build(f.t.Context())
	if err != nil {
		f.t.Fatal(err)
	}
	return agent.Run(f.t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, input)}, func(abi.Event) error { return nil })
}

// seed appends user/assistant turns from..to, each carrying its FACT marker.
func (f *compactionFixture) seed(from, to, padding int) {
	f.t.Helper()
	var batch []abi.SessionRecord
	for i := from; i <= to; i++ {
		user := wire.TextMessage(wire.RoleUser, fmt.Sprintf("turn %d FACT-%d %s", i, i, strings.Repeat("u", padding)))
		reply := wire.TextMessage(wire.RoleAssistant, fmt.Sprintf("reply %d %s", i, strings.Repeat("r", padding)))
		batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &user}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &reply}))
	}
	if _, err := f.log.AppendRecords(f.t.Context(), batch); err != nil {
		f.t.Fatal(err)
	}
}

func (f *compactionFixture) plugin() *compactionPlugin {
	f.t.Helper()
	plugin, err := newCompactionPlugin(f.repo, "owner", f.log.ID(), CompactionConfig{})
	if err != nil {
		f.t.Fatal(err)
	}
	return plugin
}

func (f *compactionFixture) compactions(status CompactionStatus) []CompactionReport {
	var out []CompactionReport
	for _, report := range f.reports {
		if report.Compaction != nil && report.Compaction.Status == status {
			out = append(out, *report.Compaction)
		}
	}
	return out
}

func summaryOf(request abi.ModelRequest) (wire.Message, bool) {
	for _, message := range request.Messages {
		if strings.HasPrefix(message.ID, summaryMessagePrefix) {
			return message, true
		}
	}
	return wire.Message{}, false
}

func containsText(request abi.ModelRequest, fragment string) bool {
	for _, message := range request.Messages {
		if strings.Contains(message.Text(), fragment) {
			return true
		}
	}
	return false
}

func TestCompactionCarriesEarlyFactsAcrossChainedSummaries(t *testing.T) {
	f := newCompactionFixture(t)
	f.reply = func(request abi.ModelRequest) []wire.StreamChunk {
		estimate, _ := agentabi.SimpleTokenEstimator{}.EstimateMessages(request.Messages)
		return textReply("reply "+strings.Repeat("r", 1800), uint64(estimate))
	}
	config := CompactionConfig{WindowTokens: 20_000}
	for i := 1; i <= 50; i++ {
		if err := f.run(config, fmt.Sprintf("turn %d FACT-%d %s", i, i, strings.Repeat("u", 1800))); err != nil {
			t.Fatal(i, err)
		}
	}
	compacted := f.compactions(CompactionSucceeded)
	if len(compacted) < 3 {
		t.Fatal("long organic session did not compact repeatedly", len(compacted))
	}
	final := f.mains[len(f.mains)-1]
	summary, ok := summaryOf(final)
	if !ok || !strings.Contains(summary.Text(), "FACT-1 ") || !strings.Contains(summary.Text(), "FACT-2 ") {
		t.Fatal("final request lost the earliest facts", summary.Text())
	}
	if containsText(final, "turn 1 FACT-1 u") || !strings.HasPrefix(final.Messages[len(final.Messages)-1].Text(), "turn 50 FACT-50") {
		t.Fatal("final request resent early turns verbatim or lost the latest input")
	}
	latest, _, corrupt, err := f.plugin().readSummary(t.Context())
	if err != nil || corrupt != nil || latest == nil {
		t.Fatal("latest summary unreadable", err, corrupt)
	}
	first := compacted[0]
	if latest.payload.FromSequence != first.FromSequence || latest.payload.PreviousSequence == 0 || latest.payload.PromptVersion == "" {
		t.Fatal("summary chain does not reach back to the first summarized record", latest.payload, first)
	}
	// Each summary request saw only the delta since the previous summary.
	for _, request := range f.summaries[1:] {
		if !strings.Contains(request.Messages[1].Text(), "Previous summary:") {
			t.Fatal("chained summary ignored the previous summary")
		}
	}
	for _, report := range f.reports {
		if report.Usage != nil && report.Usage.Parts != nil {
			p := report.Usage.Parts
			if p.System+p.Tools+p.Summary+p.Recent != report.Usage.InputTokens+report.Usage.OutputTokens || report.Usage.WindowTokens != 20_000 {
				t.Fatal("context parts do not sum to the measured total", report.Usage)
			}
		}
	}
}

func TestCompactionViewIsDeterministicAcrossReloadAndRestart(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 60, 1800)
	if err := f.run(CompactionConfig{WindowTokens: 20_000}, "latest question"); err != nil {
		t.Fatal(err)
	}
	if len(f.compactions(CompactionSucceeded)) != 1 {
		t.Fatal("expected exactly one compaction", f.reports)
	}
	rewritten, ok := summaryOf(f.mains[0])
	if !ok {
		t.Fatal("compacted request carries no summary")
	}
	view, err := f.plugin().Load(t.Context(), sourceOnlyReader(f.log.ID()))
	if err != nil || !reflect.DeepEqual(view[0], rewritten) {
		t.Fatal("reload presented a different summary than the rewritten request", err)
	}
	if err := f.repo.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := session.Open(t.Context(), f.dir)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	plugin, err := newCompactionPlugin(reopened, "owner", f.log.ID(), CompactionConfig{})
	if err != nil {
		t.Fatal(err)
	}
	restarted, err := plugin.Load(t.Context(), sourceOnlyReader(f.log.ID()))
	if err != nil || !reflect.DeepEqual(view, restarted) {
		t.Fatal("restart changed the compacted working view", err)
	}
	if containsText(abi.ModelRequest{Messages: view}, "turn 1 FACT-1 u") {
		t.Fatal("summarized records reappeared in the view")
	}
}

func TestCompactionFailureKeepsNoSummaryAndFallsBackVisibly(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 120, 1200)
	f.summarize = func(abi.ModelRequest) (string, error) { return "", errors.New("summary provider unavailable") }
	if err := f.run(CompactionConfig{WindowTokens: 20_000}, "latest question"); err != nil {
		t.Fatal("a failed compaction must not fail the run", err)
	}
	if _, found, err := f.repo.LatestRecord(t.Context(), "owner", f.log.ID(), contextNamespace, summaryKind); err != nil || found {
		t.Fatal("failed compaction wrote a summary", found, err)
	}
	if _, found, err := f.repo.LatestRecord(t.Context(), "owner", f.log.ID(), contextNamespace, compactionFailedKind); err != nil || !found {
		t.Fatal("failed compaction left no failure evidence", err)
	}
	failed := f.compactions(CompactionFailed)
	if len(failed) != 1 || !strings.Contains(failed[0].Error, "summary provider unavailable") {
		t.Fatal("failure not reported", f.reports)
	}
	request := f.mains[0]
	if _, ok := summaryOf(request); ok || !containsText(request, "not a summary") || len(request.Messages) > contextTargetMessages+8 {
		t.Fatal("failed compaction did not fall back to the bounded recent turns", len(request.Messages))
	}
	if !strings.Contains(request.Messages[len(request.Messages)-1].Text(), "latest question") {
		t.Fatal("fallback lost the latest input")
	}
}

type probeTool struct{ result string }

func (probeTool) Name() string        { return "probe" }
func (probeTool) Description() string { return "Local compaction fixture" }
func (probeTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object"}`)
}
func (p probeTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: p.result}}}, nil
}
func (probeTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (probeTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead}
}
func (probeTool) IsReadOnly(wire.ToolCall) bool { return true }

func TestCompactionInsideOneLongTurnPinsItsInput(t *testing.T) {
	f := newCompactionFixture(t)
	f.tools = []abi.Tool{probeTool{result: "FACT-" + strings.Repeat("p", 3000)}}
	calls := 0
	f.reply = func(request abi.ModelRequest) []wire.StreamChunk {
		estimate, _ := agentabi.SimpleTokenEstimator{}.EstimateMessages(request.Messages)
		if calls < 24 {
			calls++
			call := wire.ToolCall{ID: fmt.Sprintf("probe-%d", calls), Name: "probe", Arguments: json.RawMessage(`{}`)}
			return []wire.StreamChunk{{Kind: wire.ChunkToolCall, ToolCall: &call}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: uint64(estimate), OutputTokens: 5}}, {Kind: wire.ChunkFinish, FinishReason: "tool_calls"}}
		}
		return textReply("done", uint64(estimate))
	}
	input := "investigate everything FACT-0"
	if err := f.run(CompactionConfig{WindowTokens: 12_000}, input, 64); err != nil {
		t.Fatal(err)
	}
	compacted := f.compactions(CompactionSucceeded)
	if len(compacted) < 2 {
		t.Fatal("one long turn did not compact repeatedly", len(compacted))
	}
	for _, report := range compacted {
		if report.FromSequence != compacted[0].FromSequence {
			t.Fatal("repeated mid-turn compactions broke the summary chain", compacted)
		}
	}
	latest, _, _, err := f.plugin().readSummary(t.Context())
	if err != nil || latest == nil || latest.payload.PinnedSequence == 0 {
		t.Fatal("mid-turn compaction did not pin the turn input", err)
	}
	final := f.mains[len(f.mains)-1]
	summary, ok := summaryOf(final)
	if !ok || strings.Contains(summary.Text(), "omitted earlier") {
		t.Fatal("final request carries no summary or claims omitted history in a fresh session")
	}
	for i, message := range final.Messages {
		if message.ID == summary.ID {
			if i+1 >= len(final.Messages) || final.Messages[i+1].Text() != input || final.Messages[i+1].Role != wire.RoleUser {
				t.Fatal("pinned input does not follow the summary verbatim")
			}
		}
	}
}

func TestCompactionConflictNeverOverwritesANewerSummary(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 60, 1800)
	var competing abi.SessionRecord
	f.summarize = func(abi.ModelRequest) (string, error) {
		payload, _ := json.Marshal(summarySnapshot{Summary: "competing summary", FromSequence: 1, ThroughSequence: 2, HighWater: 2, Messages: 2, Trigger: "auto", PromptVersion: "test"})
		record, err := f.log.AppendExtensionSnapshot(context.Background(), contextNamespace, summaryKind, 0, payload)
		competing = record
		if err != nil {
			return "", err
		}
		return "our summary", nil
	}
	if err := f.run(CompactionConfig{WindowTokens: 20_000}, "latest question"); err != nil {
		t.Fatal(err)
	}
	entry, found, err := f.repo.LatestRecord(t.Context(), "owner", f.log.ID(), contextNamespace, summaryKind)
	if err != nil || !found || entry.Sequence != competing.Sequence {
		t.Fatal("conflicting compaction overwrote the newer summary", entry.Sequence, competing.Sequence, err)
	}
	failed := f.compactions(CompactionFailed)
	if len(failed) != 1 || !strings.Contains(failed[0].Error, "nothing was overwritten") {
		t.Fatal("conflict not reported as a failed compaction", f.reports)
	}
}

func TestCorruptSummaryFallsBackStartsFreshChainAndKeepsRecord(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 50, 400)
	corrupt, err := f.log.AppendExtensionSnapshot(t.Context(), contextNamespace, summaryKind, 0, json.RawMessage(`{"summary":"x","bogus":true}`))
	if err != nil {
		t.Fatal(err)
	}
	before, err := f.repo.Page(t.Context(), "owner", f.log.ID(), corrupt.Sequence-1, 1, 0)
	if err != nil || len(before.Entries) != 1 {
		t.Fatal(err)
	}
	_, original, err := f.repo.ReadArtifactFull(t.Context(), "owner", f.log.ID(), before.Entries[0].Value.URI, 64<<10)
	if err != nil {
		t.Fatal(err)
	}
	if err := f.run(CompactionConfig{WindowTokens: 200_000}, "first question"); err != nil {
		t.Fatal("a corrupt summary blocked the run", err)
	}
	if reports := f.compactions(SummaryCorrupt); len(reports) != 1 || reports[0].Record == nil || reports[0].Record.Sequence != corrupt.Sequence {
		t.Fatal("corrupt summary not reported once with its record", f.reports)
	}
	if _, ok := summaryOf(f.mains[0]); ok || !containsText(f.mains[0], "not a summary") || len(f.mains[0].Messages) > contextTargetMessages+8 {
		t.Fatal("corrupt summary did not fall back to bounded recent turns")
	}
	// The next compaction starts a fresh chain after the kept corrupt record.
	if err := f.run(CompactionConfig{WindowTokens: 200_000, Force: true}, "second question"); err != nil {
		t.Fatal(err)
	}
	latest, _, stillCorrupt, err := f.plugin().readSummary(t.Context())
	if err != nil || stillCorrupt != nil || latest == nil || latest.payload.PreviousSequence != 0 || latest.sequence <= corrupt.Sequence {
		t.Fatal("fresh chain not started after the corrupt record", err, stillCorrupt)
	}
	_, kept, err := f.repo.ReadArtifactFull(t.Context(), "owner", f.log.ID(), before.Entries[0].Value.URI, 64<<10)
	if err != nil || string(kept) != string(original) {
		t.Fatal("corrupt record was changed", err)
	}
	reports := len(f.compactions(SummaryCorrupt))
	if err := f.run(CompactionConfig{WindowTokens: 200_000}, "third question"); err != nil || len(f.compactions(SummaryCorrupt)) != reports {
		t.Fatal("healed session still reports corruption or fails", err)
	}
	if _, ok := summaryOf(f.mains[len(f.mains)-1]); !ok {
		t.Fatal("healed session does not use its fresh summary")
	}
}

func TestCompactionRetriesOneClassifiedOverflow(t *testing.T) {
	overflow := &agentabi.ProviderError{Kind: agentabi.ProviderContextLength, Message: "fixture overflow"}
	t.Run("retry succeeds", func(t *testing.T) {
		f := newCompactionFixture(t)
		f.seed(1, 30, 400)
		rejected := false
		f.main = func(abi.ModelRequest) error {
			if !rejected {
				rejected = true
				return overflow
			}
			return nil
		}
		if err := f.run(CompactionConfig{WindowTokens: 200_000}, "latest question"); err != nil {
			t.Fatal(err)
		}
		compacted := f.compactions(CompactionSucceeded)
		if len(f.mains) != 2 || len(compacted) != 1 || compacted[0].Trigger != TriggerOverflow {
			t.Fatal("overflow was not compacted and retried exactly once", len(f.mains), f.reports)
		}
		if _, ok := summaryOf(f.mains[1]); !ok {
			t.Fatal("retry did not send the compacted request")
		}
	})
	t.Run("second overflow fails", func(t *testing.T) {
		f := newCompactionFixture(t)
		f.seed(1, 30, 400)
		f.main = func(abi.ModelRequest) error { return overflow }
		err := f.run(CompactionConfig{WindowTokens: 200_000}, "latest question")
		if !errors.Is(err, overflow) || len(f.mains) != 2 {
			t.Fatal("repeated overflow was retried more than once or hidden", len(f.mains), err)
		}
	})
	t.Run("one retry per run", func(t *testing.T) {
		f := newCompactionFixture(t)
		f.seed(1, 30, 400)
		f.tools = []abi.Tool{probeTool{result: "probe result"}}
		calls := 0
		f.main = func(abi.ModelRequest) error {
			calls++
			if calls == 1 || calls == 3 {
				return overflow
			}
			return nil
		}
		f.reply = func(abi.ModelRequest) []wire.StreamChunk {
			call := wire.ToolCall{ID: "probe-once", Name: "probe", Arguments: json.RawMessage(`{}`)}
			return []wire.StreamChunk{{Kind: wire.ChunkToolCall, ToolCall: &call}, {Kind: wire.ChunkFinish, FinishReason: "tool_calls"}}
		}
		err := f.run(CompactionConfig{WindowTokens: 200_000}, "latest question")
		if !errors.Is(err, overflow) || len(f.mains) != 3 || len(f.compactions(CompactionSucceeded)) != 1 {
			t.Fatal("a second overflow in the same run was compacted and retried again", len(f.mains), err)
		}
	})
	t.Run("unclassified error is not retried", func(t *testing.T) {
		f := newCompactionFixture(t)
		f.seed(1, 30, 400)
		invalid := &agentabi.ProviderError{Kind: agentabi.ProviderInvalidArgument, Message: "prompt is too long"}
		f.main = func(abi.ModelRequest) error { return invalid }
		err := f.run(CompactionConfig{WindowTokens: 200_000}, "latest question")
		if !errors.Is(err, invalid) || len(f.mains) != 1 || len(f.summaries) != 0 {
			t.Fatal("an unclassified rejection triggered compaction", len(f.mains), err)
		}
	})
}

func TestManualCompactionRunsBelowTriggerOrReportsNothingToDo(t *testing.T) {
	f := newCompactionFixture(t)
	if err := f.run(CompactionConfig{WindowTokens: 200_000, Force: true}, "only question"); err != nil {
		t.Fatal(err)
	}
	if len(f.compactions(CompactionSkipped)) != 1 || len(f.summaries) != 0 {
		t.Fatal("manual compaction of a single turn did not report nothing to do", f.reports)
	}
	f.seed(1, 10, 200)
	if err := f.run(CompactionConfig{WindowTokens: 200_000, Force: true}, "next question"); err != nil {
		t.Fatal(err)
	}
	compacted := f.compactions(CompactionSucceeded)
	if len(compacted) != 1 || compacted[0].Trigger != TriggerManual {
		t.Fatal("manual compaction below the trigger did not run", f.reports)
	}
	// Force applies to the execution's first request only.
	if err := f.run(CompactionConfig{WindowTokens: 200_000}, "after"); err != nil || len(f.compactions(CompactionSucceeded)) != 1 {
		t.Fatal("compaction repeated without a request", err)
	}
}

func TestContextUsageReportsMeasuredTotalsOnly(t *testing.T) {
	f := newCompactionFixture(t)
	if err := f.run(CompactionConfig{}, "question"); err != nil {
		t.Fatal(err)
	}
	var usage *ContextUsage
	for _, report := range f.reports {
		if report.Usage != nil {
			usage = report.Usage
		}
	}
	if usage == nil || usage.WindowTokens != 0 || usage.InputTokens == 0 || usage.Parts == nil || usage.Parts.System == 0 || usage.Parts.Recent < usage.OutputTokens {
		t.Fatal("undeclared window or measured parts misreported", usage)
	}
	f.reply = func(abi.ModelRequest) []wire.StreamChunk {
		return []wire.StreamChunk{{Kind: wire.ChunkText, Text: "no usage"}, {Kind: wire.ChunkFinish, FinishReason: "stop"}}
	}
	reports := len(f.reports)
	if err := f.run(CompactionConfig{}, "again"); err != nil || len(f.reports) != reports {
		t.Fatal("a response without usage produced an estimated report", err)
	}
}

func TestDisabledCompactionKeepsPlainSourceAndIgnoresSummaries(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 50, 100)
	if _, err := f.log.AppendExtensionSnapshot(t.Context(), contextNamespace, summaryKind, 0, json.RawMessage(`{"summary":"x"}`)); err != nil {
		t.Fatal(err)
	}
	if err := f.run(CompactionConfig{Disabled: true}, "question"); err != nil {
		t.Fatal(err)
	}
	if len(f.reports) != 0 || !containsText(f.mains[0], "not a summary") {
		t.Fatal("disabled compaction read summaries or reported", f.reports)
	}
}

func TestCompactionStartsAFreshChainWhenItCannotSeeEverythingSinceTheLastSummary(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 6, 200)
	if err := f.run(CompactionConfig{WindowTokens: 200_000, Force: true}, "first question"); err != nil {
		t.Fatal(err)
	}
	first, _, _, err := f.plugin().readSummary(t.Context())
	if err != nil || first == nil {
		t.Fatal("seed summary missing", err)
	}
	// Far more than the summary input budget accumulated without compaction.
	f.seed(7, 110, 1800)
	if err := f.run(CompactionConfig{WindowTokens: 20_000}, "second question"); err != nil {
		t.Fatal(err)
	}
	latest, _, _, err := f.plugin().readSummary(t.Context())
	if err != nil || latest == nil || latest.sequence == first.sequence {
		t.Fatal("no new summary", err)
	}
	if latest.payload.PreviousSequence != 0 || !latest.payload.EarlierOmitted || latest.payload.FromSequence <= first.payload.ThroughSequence {
		t.Fatal("summary claims a chain over records it never saw", latest.payload)
	}
	summary, ok := summaryOf(f.mains[len(f.mains)-1])
	if !ok || !strings.Contains(summary.Text(), "omitted earlier and are not summarized") {
		t.Fatal("broken chain not stated to the model")
	}
}
