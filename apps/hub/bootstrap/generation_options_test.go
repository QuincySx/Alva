// INPUT: Explicit saved model defaults, admitted Hub Turns and loopback HTTP generation fixtures.
// OUTPUT: Actual sampling/reasoning/system fields and enforced one-iteration limits across the full product bridge.
// POS: [L7 assembly testing] Exercises preference snapshot through Runtime, Alva factory, harness and provider without paid accounts.
package bootstrap

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestSavedModelDefaultsDriveActualHTTPAndIterationLimit(t *testing.T) {
	for _, limited := range []bool{false, true} {
		name := "one-answer"
		if limited {
			name = "one-tool-iteration"
		}
		t.Run(name, func(t *testing.T) {
			var calls atomic.Int32
			bodies := make(chan json.RawMessage, 2)
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				var body json.RawMessage
				if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20)).Decode(&body); err != nil {
					http.Error(w, "bad fixture request", 400)
					return
				}
				bodies <- body
				if limited {
					waitReply(w, "read_file", `{"path":"source.txt"}`, "")
				} else {
					waitReply(w, "", "", "actual explicit answer")
				}
			}))
			defer server.Close()
			app, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "gpt-5.6"}})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { closeApp(t, app) })
			descriptor, ok := app.Runtime.Descriptor("alva")
			if !ok || !descriptor.Config.SupportsGeneration || !descriptor.Config.SupportsSystemPrompt || !descriptor.Config.SupportsMaxTurns {
				t.Fatal("registered native factory lacks its implemented option surface")
			}
			target := app.Targets[0]
			preference := app.Service.Connections.ModelPreference(target)
			temperature, tokens := 0.3, uint32(543)
			preference.Options = hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &temperature, MaxTokens: &tokens, ReasoningEffort: "high"}, SystemPrompt: "SAVED-MODEL-SYSTEM-ROLE", MaxTurns: 1}
			if _, err = app.Service.Connections.SaveModelPreference(t.Context(), target, preference); err != nil {
				t.Fatal(err)
			}
			if calls.Load() != 0 {
				t.Fatal("saving generation defaults performed model I/O")
			}
			workspace := t.TempDir()
			if err = os.WriteFile(filepath.Join(workspace, "source.txt"), []byte("read-only fixture"), 0600); err != nil {
				t.Fatal(err)
			}
			session, err := app.Service.Create(t.Context(), "actual generation defaults", workspace)
			if err != nil {
				t.Fatal(err)
			}
			_, done, err := app.Runtime.Start(t.Context(), session.ID, "one admitted request")
			if err != nil {
				t.Fatal(err)
			}
			select {
			case err = <-done:
				if limited {
					if err == nil || !strings.Contains(err.Error(), "maximum agent iterations reached") {
						t.Fatal("actual SDK limit not enforced", err)
					}
				} else if err != nil {
					t.Fatal(err)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("actual generation owner did not finish")
			}
			if calls.Load() != 1 {
				t.Fatal("iteration limit allowed extra actual model requests", calls.Load())
			}
			var raw json.RawMessage
			select {
			case raw = <-bodies:
			default:
				t.Fatal("actual provider request missing")
			}
			var body struct {
				Temperature *float64 `json:"temperature"`
				MaxTokens   *uint32  `json:"max_completion_tokens"`
				Effort      string   `json:"reasoning_effort"`
				Messages    []struct {
					Role    string          `json:"role"`
					Content json.RawMessage `json:"content"`
				} `json:"messages"`
			}
			if json.Unmarshal(raw, &body) != nil || body.Temperature == nil || *body.Temperature != 0.3 || body.MaxTokens == nil || *body.MaxTokens != 543 || body.Effort != "high" {
				t.Fatal("saved generation snapshot absent from actual protocol request", string(raw))
			}
			systemFound := false
			for _, message := range body.Messages {
				if message.Role == "system" && strings.Contains(string(message.Content), "SAVED-MODEL-SYSTEM-ROLE") {
					systemFound = true
				}
			}
			if !systemFound {
				t.Fatal("model system default was demoted or dropped")
			}
			if limited {
				history, err := app.Store.History(t.Context(), session.ID, 0, 128)
				if err != nil {
					t.Fatal(err)
				}
				toolEnd := false
				for _, entry := range history {
					if entry.Event.Kind == engine.ToolEnd && entry.Event.Name == "read_file" && !entry.Event.IsError {
						toolEnd = true
					}
				}
				if !toolEnd {
					t.Fatal("iteration budget did not permit the first real read tool before exhausting")
				}
			}
		})
	}
}
