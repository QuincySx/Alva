// INPUT: Actual registered Alva factory, product manual compaction intent and one local HTTP model
// OUTPUT: Manual compaction end-to-end regression: intent → admitted request → summary request → compacted main request, reports and usage
// POS: [L7 Hub assembly] The one-shot intent reaches the harness through the real factory and is cleared only once admitted.
package bootstrap

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestManualCompactionIntentCompactsTheNextAdmittedRequest(t *testing.T) {
	var mu sync.Mutex
	var summaries, mains []string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Messages []struct {
				Role    string          `json:"role"`
				Content json.RawMessage `json:"content"`
			} `json:"messages"`
		}
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4<<20)).Decode(&body); err != nil {
			http.Error(w, "invalid local request", 400)
			return
		}
		var all strings.Builder
		for _, message := range body.Messages {
			all.WriteString(message.Role + ": " + string(message.Content) + "\n")
		}
		answer := "actual answer"
		mu.Lock()
		if strings.Contains(all.String(), "You compress an agent conversation") {
			summaries = append(summaries, all.String())
			answer = "SUMMARY-OF-FIRST-TURN"
		} else {
			mains = append(mains, all.String())
		}
		mu.Unlock()
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"id\":\"local\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"" + answer + "\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"local\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"id\":\"local\",\"choices\":[],\"usage\":{\"prompt_tokens\":120,\"completion_tokens\":7,\"total_tokens\":127}}\n\ndata: [DONE]\n\n"))
	}))
	defer server.Close()
	a, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "explicit-model"}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, a) })
	p, err := a.Service.Create(t.Context(), "manual compaction fixture", "")
	if err != nil {
		t.Fatal(err)
	}
	d, ok := a.Runtime.Descriptor("alva")
	if !ok || !d.Runtime.Capabilities().ContextCompaction {
		t.Fatal("assembled Alva engine does not declare context compaction")
	}
	target := a.Targets[0]
	execute := func(prompt, resume string) (string, []engine.ContextReport, *engine.Usage) {
		t.Helper()
		ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
		defer cancel()
		var native, failure string
		var reports []engine.ContextReport
		var usage *engine.Usage
		var outcome engine.Outcome
		err := d.Runtime.Execute(ctx, engine.Request{OwnerID: p.ID, Workspace: p.Workspace, Model: target.ModelID, Provider: target.ProviderID, AccountSource: target.AccountSource, BillingRoute: target.BillingRoute, Prompt: prompt, ResumeSession: resume}, func(event engine.Event) error {
			switch event.Kind {
			case engine.SessionStarted:
				native = event.SessionID
			case engine.ContextUpdated:
				reports = append(reports, *event.Context)
			case engine.Error:
				failure = event.Text
			case engine.Completed:
				outcome, usage = event.Outcome, event.Usage
			}
			return nil
		})
		if err != nil || outcome != engine.Succeeded {
			t.Fatal("actual factory run failed", outcome, failure, err)
		}
		return native, reports, usage
	}
	native, reports, _ := execute("first turn with FACT-ALPHA", "")
	if len(reports) != 1 || reports[0].Usage == nil || reports[0].Usage.InputTokens != 120 || reports[0].Usage.WindowTokens != 0 || reports[0].Compaction != nil {
		t.Fatal("first run did not report only its measured, undeclared-window usage", reports)
	}
	if err := a.Service.RequestContextCompaction(t.Context(), p.ID); err != nil {
		t.Fatal(err)
	}
	_, reports, usage := execute("second turn", native)
	if a.Service.ContextCompactionPending(p.ID) {
		t.Fatal("admitted request did not clear the one-shot intent")
	}
	var compacted *engine.ContextCompaction
	for _, report := range reports {
		if report.Compaction != nil {
			compacted = report.Compaction
		}
	}
	if compacted == nil || compacted.Status != engine.ContextCompacted || compacted.Trigger != "manual" || compacted.Reference == nil || compacted.Reference.SessionID != native {
		t.Fatal("manual compaction not performed and reported", reports)
	}
	mu.Lock()
	summaryCount, mainCount, first, second := len(summaries), len(mains), "", ""
	if summaryCount > 0 && mainCount > 1 {
		first, second = summaries[0], mains[1]
	}
	mu.Unlock()
	if summaryCount != 1 || !strings.Contains(first, "FACT-ALPHA") || mainCount != 2 {
		t.Fatal("summary request did not carry the first turn", summaryCount, mainCount)
	}
	if !strings.Contains(second, "SUMMARY-OF-FIRST-TURN") || strings.Contains(second, "first turn with FACT-ALPHA") {
		t.Fatal("compacted main request did not replace the first turn with its summary")
	}
	if usage == nil || usage.InputTokens != 240 || usage.OutputTokens != 14 {
		t.Fatal("summary request usage missing from the attempt usage", usage)
	}
	// Without a new intent the next request is not compacted again.
	_, reports, _ = execute("third turn", native)
	for _, report := range reports {
		if report.Compaction != nil {
			t.Fatal("compaction repeated without an intent", report.Compaction)
		}
	}
}
