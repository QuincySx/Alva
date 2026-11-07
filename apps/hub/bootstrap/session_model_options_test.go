// INPUT: Real SQLite-backed product saves and a local HTTP provider with a blocked first request.
// OUTPUT: Reopened session overrides reach actual protocol requests while active Attempts stay frozen.
// POS: [L7 assembly tests] No paid model/account; verifies override, next Turn and clear through the full harness.
package bootstrap

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestSessionModelOptionsReopenActiveFreezeNextTurnAndClearDriveActualHTTP(t *testing.T) {
	var calls atomic.Int32
	bodies := make(chan json.RawMessage, 4)
	release := make(chan struct{})
	var once sync.Once
	unblock := func() { once.Do(func() { close(release) }) }
	defer unblock()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		count := calls.Add(1)
		var body json.RawMessage
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20)).Decode(&body); err != nil {
			http.Error(w, "bad request", 400)
			return
		}
		bodies <- body
		if count == 1 {
			select {
			case <-release:
			case <-r.Context().Done():
				return
			}
		}
		waitReply(w, "", "", "local answer")
	}))
	defer func() { unblock(); server.Close() }()
	config := Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "gpt-5.6"}}
	app, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { unblock(); closeApp(t, app) })
	target := app.Targets[0]
	defaults := app.Service.Connections.ModelPreference(target)
	defaultTemperature, sessionTemperature, tokens := .2, .65, uint32(233)
	defaults.Options = hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &defaultTemperature}, SystemPrompt: "GLOBAL-DEFAULT-ROLE", MaxTurns: 1}
	if _, err = app.Service.Connections.SaveModelPreference(t.Context(), target, defaults); err != nil {
		t.Fatal(err)
	}
	session, err := app.Service.Create(t.Context(), "session HTTP options", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	options := hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &sessionTemperature, MaxTokens: &tokens, ReasoningEffort: "low"}, SystemPrompt: "SESSION-ROLE-ONE", MaxTurns: 1}
	edit := hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: target, Options: &options}
	value, err := app.Service.SaveSessionModelOptions(t.Context(), edit)
	if err != nil || calls.Load() != 0 {
		t.Fatal("saving performed model I/O", value, err)
	}
	closeApp(t, app)
	app, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	start := func() <-chan error {
		t.Helper()
		_, done, e := app.Runtime.Start(t.Context(), session.ID, "explicit local request")
		if e != nil {
			t.Fatal(e)
		}
		return done
	}
	read := func() json.RawMessage {
		t.Helper()
		select {
		case body := <-bodies:
			return body
		case <-time.After(5 * time.Second):
			t.Fatal("actual provider request missing")
			return nil
		}
	}
	finish := func(done <-chan error) {
		t.Helper()
		select {
		case e := <-done:
			if e != nil {
				t.Fatal(e)
			}
		case <-time.After(5 * time.Second):
			t.Fatal("actual execution owner did not finish")
		}
	}
	check := func(raw json.RawMessage, temperature float64, role string, override bool) {
		t.Helper()
		var body struct {
			Temperature *float64 `json:"temperature"`
			Tokens      *uint32  `json:"max_completion_tokens"`
			Effort      string   `json:"reasoning_effort"`
			Messages    []struct {
				Role    string          `json:"role"`
				Content json.RawMessage `json:"content"`
			} `json:"messages"`
		}
		if json.Unmarshal(raw, &body) != nil || body.Temperature == nil || *body.Temperature != temperature {
			t.Fatal("actual HTTP temperature ignored scope", string(raw))
		}
		if override && (body.Tokens == nil || *body.Tokens != 233 || body.Effort != "low") {
			t.Fatal("actual HTTP lost generation override", string(raw))
		}
		found := false
		for _, message := range body.Messages {
			found = found || message.Role == "system" && strings.Contains(string(message.Content), role)
		}
		if !found {
			t.Fatal("actual HTTP lost explicit system role", role, string(raw))
		}
	}
	done := start()
	first := read()
	sessionTemperature, options.SystemPrompt = .85, "SESSION-ROLE-TWO"
	edit.ExpectedRevision = value.Revision
	value, err = app.Service.SaveSessionModelOptions(t.Context(), edit)
	if err != nil {
		t.Fatal(err)
	}
	check(first, .65, "SESSION-ROLE-ONE", true)
	unblock()
	finish(done)
	done = start()
	check(read(), .85, "SESSION-ROLE-TWO", true)
	finish(done)
	edit.ExpectedRevision, edit.Options = value.Revision, nil
	if _, err = app.Service.SaveSessionModelOptions(t.Context(), edit); err != nil {
		t.Fatal(err)
	}
	done = start()
	check(read(), .2, "GLOBAL-DEFAULT-ROLE", false)
	finish(done)
	if calls.Load() != 3 {
		t.Fatal("unexpected execution from saving or clearing overrides", calls.Load())
	}
}
