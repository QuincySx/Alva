// INPUT: context, encoding/json, fmt, os, runtime, strconv, strings, testing, time, real native SQLite session, kernel/abi, llm/wire
// OUTPUT: Long-session context gate: in a GiB-sized native session the model's actual request carries the summary of the earliest content, read from bounded queries only
// POS: [L5 harness testing] CI runs a few MiB; ALVA_LONG_SESSION_GIB seeds the full-size native history and reports timings
package harness

import (
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// seedSummarizedHistory writes bytes of conversation turns the way a session
// that compacted all along stores them: turns, and every chainEvery turns a
// summary record that chains the previous one. The earliest turn carries
// FACT-EARLY, which every later summary keeps.
func seedSummarizedHistory(t *testing.T, f *compactionFixture, bytes int64, chainEvery int) (uint64, int) {
	t.Helper()
	padding := strings.Repeat("长会话正文 history ", 400)
	written, turn, previous, summaries := int64(0), 0, uint64(0), 0
	from := uint64(0)
	for written < bytes {
		batch := make([]abi.SessionRecord, 0, 2*chainEvery)
		for i := 0; i < chainEvery; i++ {
			turn++
			fact := ""
			if turn == 1 {
				fact = " FACT-EARLY"
			}
			user := wire.TextMessage(wire.RoleUser, fmt.Sprintf("turn %d%s %s", turn, fact, padding))
			reply := wire.TextMessage(wire.RoleAssistant, fmt.Sprintf("reply %d %s", turn, padding))
			batch = append(batch, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &user}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &reply}))
			written += int64(len(user.Text()) + len(reply.Text()))
		}
		committed, err := f.log.AppendRecords(t.Context(), batch)
		if err != nil {
			t.Fatal(err)
		}
		if from == 0 {
			from = committed[0].Sequence
		}
		through := committed[len(committed)-1].Sequence
		payload, _ := json.Marshal(summarySnapshot{Summary: fmt.Sprintf("Summary facts: FACT-EARLY and %d later turns", turn-1), FromSequence: from, ThroughSequence: through, PreviousSequence: previous, HighWater: through, Messages: 2 * chainEvery, Trigger: "auto", PromptVersion: "alva.context.summary/v1"})
		record, err := f.log.AppendExtensionSnapshot(t.Context(), contextNamespace, summaryKind, previous, payload)
		if err != nil {
			t.Fatal(err)
		}
		previous = record.Sequence
		summaries++
	}
	return previous, turn
}

func TestLongSessionContextGateCarriesEarlySummary(t *testing.T) {
	bytes := int64(4 << 20)
	if value := os.Getenv("ALVA_LONG_SESSION_GIB"); value != "" {
		gib, err := strconv.ParseFloat(value, 64)
		if err != nil || gib <= 0 {
			t.Fatal("ALVA_LONG_SESSION_GIB must be a positive number of GiB", value)
		}
		bytes = int64(gib * (1 << 30))
	}
	f := newCompactionFixture(t)
	start := time.Now()
	summary, turns := seedSummarizedHistory(t, f, bytes, 64)
	// Recent turns after the latest summary, as the next requests see them.
	f.seed(turns+1, turns+6, 600)
	seeded := time.Since(start)

	plugin := f.plugin()
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	loadStart := time.Now()
	view, err := plugin.Load(t.Context(), sourceOnlyReader(f.log.ID()))
	loaded := time.Since(loadStart)
	runtime.ReadMemStats(&after)
	if err != nil {
		t.Fatal(err)
	}
	viewBytes := 0
	for _, message := range view {
		encoded, _ := json.Marshal(message)
		viewBytes += len(encoded)
	}
	if len(view) != 13 || !strings.HasPrefix(view[0].ID, summaryMessagePrefix) || viewBytes > contextMaxBytes {
		t.Fatal("working view is not the latest summary plus the recent turns", len(view), viewBytes)
	}

	if err := f.run(CompactionConfig{WindowTokens: 200_000}, "newest question"); err != nil {
		t.Fatal(err)
	}
	request := f.mains[len(f.mains)-1]
	got, ok := summaryOf(request)
	if !ok || !strings.Contains(got.Text(), "FACT-EARLY") || !strings.HasSuffix(got.ID, "/"+strconv.FormatUint(summary, 10)) {
		t.Fatal("model request lacks the summary of the earliest content")
	}
	var referenced bool
	for _, block := range got.Content {
		referenced = referenced || block.Kind == wire.ContentReference && block.Reference != nil && block.Reference.URI != ""
	}
	if !referenced || containsText(request, "turn 1 FACT-EARLY") || request.Messages[len(request.Messages)-1].Text() != "newest question" {
		t.Fatal("summary lost its record reference, early turns were resent or the latest input is missing")
	}
	t.Logf("native history %.3f GiB (%d turns) seeded in %s; Load %s allocating %d B, view %d messages / %d B; model request %d messages", float64(bytes)/(1<<30), turns, seeded.Round(time.Millisecond), loaded, after.TotalAlloc-before.TotalAlloc, len(view), viewBytes, len(request.Messages))
}

// An old session that never compacted: the first compaction covers only a
// bounded recent stretch and says the earlier records are not summarized.
func TestLongSessionWithoutSummariesCompactsABoundedStretch(t *testing.T) {
	f := newCompactionFixture(t)
	f.seed(1, 400, 1800)
	if err := f.run(CompactionConfig{WindowTokens: 20_000}, "first question after the upgrade"); err != nil {
		t.Fatal(err)
	}
	latest, _, _, err := f.plugin().readSummary(t.Context())
	if err != nil || latest == nil || !latest.payload.EarlierOmitted || latest.payload.PreviousSequence != 0 {
		t.Fatal("first compaction of an old session misstated its coverage", err)
	}
	if len(f.summaries) != 1 {
		t.Fatal("one compaction expected", len(f.summaries))
	}
	input := f.summaries[0].Messages[1].Text()
	estimate := len(input) / 4
	if estimate > int(float64(20_000)*summaryInputRatio)+summaryBlockLimit || strings.Contains(input, "turn 1 FACT-1 ") {
		t.Fatal("summary input was not bounded to a recent stretch", estimate)
	}
	summary, ok := summaryOf(f.mains[0])
	if !ok || !strings.Contains(summary.Text(), "omitted earlier and are not summarized") {
		t.Fatal("model was not told the earliest records are unsummarized")
	}
}
