// INPUT: Actual registered Alva factory and one local HTTP model response
// OUTPUT: Declared system/turn options reaching real model request without opening accounts or bypassing tool permissions
// POS: [L7 Hub assembly] Option declarations describe actual factory behavior, not a disconnected capability DTO.
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

	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
)

func TestRegisteredAlvaFactoryAppliesExplicitSystemPromptToActualHTTP(t *testing.T) {
	requests := make(chan string, 2)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			Messages []struct {
				Role    string          `json:"role"`
				Content json.RawMessage `json:"content"`
			} `json:"messages"`
		}
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20)).Decode(&body); err != nil {
			http.Error(w, "invalid local request", 400)
			return
		}
		var system string
		for _, message := range body.Messages {
			if message.Role == "system" {
				var text string
				if json.Unmarshal(message.Content, &text) == nil {
					system += text
				} else {
					var blocks []struct {
						Text string `json:"text"`
					}
					if json.Unmarshal(message.Content, &blocks) == nil {
						for _, block := range blocks {
							system += block.Text
						}
					}
				}
			}
		}
		requests <- system
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"id\":\"local\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"actual answer\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"local\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
	}))
	defer server.Close()
	a, err := Open(t.Context(), Config{DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "explicit-model"}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, a) })
	p, err := a.Service.Create(t.Context(), "explicit options fixture", "")
	if err != nil {
		t.Fatal(err)
	}
	d, ok := a.Runtime.Descriptor("alva")
	if !ok || !d.Config.SupportsSystemPrompt || !d.Config.SupportsMaxTurns {
		t.Fatal("factory options not declared")
	}
	target := a.Targets[0]
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var terminal engine.Outcome
	var failureText string
	// This exercises the registered integration factory directly with a real
	// trusted owner, not product Turn admission or client-supplied native IDs.
	err = d.Runtime.Execute(ctx, engine.Request{OwnerID: p.ID, Workspace: p.Workspace, Model: target.ModelID, Provider: target.ProviderID, AccountSource: target.AccountSource, BillingRoute: target.BillingRoute, Prompt: "one answer only", SystemPrompt: "EXPLICIT-SYSTEM-ROLE-FIXTURE", MaxTurns: 1}, func(event engine.Event) error {
		if event.Kind == engine.Error {
			failureText = event.Text
		}
		if event.Kind == engine.Completed {
			terminal = event.Outcome
		}
		return nil
	})
	if err != nil || terminal != engine.Succeeded {
		t.Fatal("actual configured factory failed", terminal, failureText, err)
	}
	select {
	case system := <-requests:
		if !strings.Contains(system, "EXPLICIT-SYSTEM-ROLE-FIXTURE") {
			t.Fatal("system option absent from real provider system role")
		}
	default:
		t.Fatal("registered factory never called actual local model")
	}
	if len(requests) != 0 {
		t.Fatal("one-answer request started additional model calls")
	}
}
