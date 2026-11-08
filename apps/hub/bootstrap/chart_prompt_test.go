// INPUT: Actual registered Alva factory, product ChartInstructions and one recording local HTTP model
// OUTPUT: The chart block format reaches the real request body after an intact session SystemPrompt, with or without one
// POS: [L7 Hub assembly tests] Proves the prompt part is delivered to the model, not merely registered
package bootstrap

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestAlvaRequestBodyCarriesChartFormatAfterSessionSystemPrompt(t *testing.T) {
	bodies := make(chan []byte, 4)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var raw json.RawMessage
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20)).Decode(&raw); err != nil {
			http.Error(w, "invalid local request", 400)
			return
		}
		bodies <- raw
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"id\":\"local\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"ok\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"local\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
	}))
	defer server.Close()
	a, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "explicit-model"}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, a) })
	d, ok := a.Runtime.Descriptor("alva")
	if !ok {
		t.Fatal("alva engine not registered")
	}
	target := a.Targets[0]
	// Near the 8 KiB session budget: the chart part must not consume it.
	session := "SESSION-SYSTEM-FIXTURE " + strings.Repeat("s", 8<<10-len("SESSION-SYSTEM-FIXTURE "))
	for _, system := range []string{session, ""} {
		p, err := a.Service.Create(t.Context(), "chart prompt fixture", "")
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		var terminal engine.Outcome
		err = d.Runtime.Execute(ctx, engine.Request{OwnerID: p.ID, Workspace: p.Workspace, Model: target.ModelID, Provider: target.ProviderID, AccountSource: target.AccountSource, BillingRoute: target.BillingRoute, Prompt: "draw something", SystemPrompt: system, MaxTurns: 1}, func(event engine.Event) error {
			if event.Kind == engine.Completed {
				terminal = event.Outcome
			}
			return nil
		})
		cancel()
		if err != nil || terminal != engine.Succeeded {
			t.Fatal("actual configured factory failed", terminal, err)
		}
		var body struct {
			Messages []struct {
				Role    string          `json:"role"`
				Content json.RawMessage `json:"content"`
			} `json:"messages"`
		}
		select {
		case raw := <-bodies:
			if err := json.Unmarshal(raw, &body); err != nil {
				t.Fatal(err)
			}
		default:
			t.Fatal("factory never called the local model")
		}
		var text string
		if len(body.Messages) == 0 || body.Messages[0].Role != "system" || json.Unmarshal(body.Messages[0].Content, &text) != nil {
			t.Fatal("request body has no leading system message", body.Messages)
		}
		chart := strings.Index(text, product.ChartInstructions)
		if chart < 0 {
			t.Fatal("chart block format absent from the actual request body")
		}
		if system != "" && (!strings.HasPrefix(text, system+"\n\n") || chart <= len(system)) {
			t.Fatal("session SystemPrompt is not intact and first", len(text))
		}
		if strings.Count(text, product.ChartInstructions) != 1 {
			t.Fatal("chart format repeated in one request")
		}
	}
}
