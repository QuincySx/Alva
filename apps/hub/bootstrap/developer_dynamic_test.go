// INPUT: Shared Hub assembly, temporary SQLite, saved Connections and a local inference/catalog fixture.
// OUTPUT: Durable selectable developer routes, exact frozen requests, stale-CAS rejection and real tool-argument grading.
// POS: [L7 assembly acceptance] Exercises Connections to explicit developer inference without real accounts or chat execution.
package bootstrap

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/developer"
	"github.com/QuincySx/Alva/apps/hub"
)

func dynamicDeveloperReply(w http.ResponseWriter, data []byte, invalidMulti bool) {
	name := "ask_human"
	args := map[string]any{"question": "Please clarify"}
	text := ""
	switch {
	case bytes.Contains(data, []byte("What is 2+2?")):
		name, text = "", "4"
	case bytes.Contains(data, []byte("current weather for the city Paris")):
		name, args = "get_weather", map[string]any{"city": "Paris"}
	case bytes.Contains(data, []byte("which database")):
		args["options"] = []any{map[string]string{"label": "SQLite"}, map[string]string{"label": "PostgreSQL"}, map[string]string{"label": "MySQL"}}
	case bytes.Contains(data, []byte("which features")):
		args["options"] = []any{map[string]string{"label": "Search"}, map[string]string{"label": "Export"}, map[string]string{"label": "Alerts"}}
		args["multi_select"] = !invalidMulti
	}
	delta := map[string]any{"content": text}
	finish := "stop"
	if name != "" {
		raw, _ := json.Marshal(args)
		delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "fixture-call", "type": "function", "function": map[string]any{"name": name, "arguments": string(raw)}}}}
		finish = "tool_calls"
	}
	event, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
	w.Header().Set("Content-Type", "text/event-stream")
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", event)
}

func awaitDynamicDeveloper(t *testing.T, service *developer.Service) developer.Snapshot {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		snapshot := service.Snapshot()
		if snapshot.State != "running" {
			return snapshot
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("developer request owner did not finish")
	return developer.Snapshot{}
}

func TestDynamicConnectionsDeveloperInferenceReopensAndFreezesExactRevision(t *testing.T) {
	var calls, catalogs atomic.Int32
	entered, release := make(chan struct{}), make(chan struct{})
	var releaseOnce sync.Once
	releaseFirst := func() { releaseOnce.Do(func() { close(release) }) }
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/models" {
			catalogs.Add(1)
			if r.Method != http.MethodGet {
				t.Error("catalog used inference method")
			}
			fmt.Fprint(w, `{"data":[{"id":"exact-old-model"}]}`)
			return
		}
		n := calls.Add(1)
		data, _ := io.ReadAll(r.Body)
		var body struct {
			Model     string `json:"model"`
			Stream    bool   `json:"stream"`
			MaxTokens int    `json:"max_tokens"`
			Tools     []struct {
				Function struct {
					Name       string          `json:"name"`
					Parameters json.RawMessage `json:"parameters"`
				} `json:"function"`
			} `json:"tools"`
		}
		model, key, header := "exact-old-model", "synthetic-old-key", "synthetic-old-header"
		if r.URL.Path == "/new" {
			model, key, header = "exact-new-model", "synthetic-new-key", "synthetic-new-header"
		}
		if json.Unmarshal(data, &body) != nil || r.Method != http.MethodPost || (r.URL.Path != "/old" && r.URL.Path != "/new") || body.Model != model || !body.Stream || body.MaxTokens != 321 || r.Header.Get("Authorization") != "Bearer "+key || r.Header.Get("X-Fixture") != header || len(body.Tools) != 2 || body.Tools[0].Function.Name != "ask_human" || body.Tools[1].Function.Name != "get_weather" || !bytes.Contains(body.Tools[0].Function.Parameters, []byte(`"question"`)) {
			t.Error("developer request lost its exact frozen endpoint/model/key/header/budget/tool schema")
			http.Error(w, "fixture request mismatch", http.StatusBadRequest)
			return
		}
		if n == 1 {
			close(entered)
			select {
			case <-release:
			case <-r.Context().Done():
				return
			}
		}
		dynamicDeveloperReply(w, data, r.URL.Path == "/new")
	}))
	defer func() { releaseFirst(); server.Close() }()
	config := Config{OpenStores: LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true}
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	if len(a.Service.Developer.Targets()) != 0 {
		t.Fatal("demo unexpectedly supplied a live developer route")
	}
	u := hub.ConnectionUpdate{Name: "Explicit developer fixture", Protocol: "openai_chat", Endpoint: server.URL + "/old", CatalogURL: server.URL + "/models", Auth: "api_key", APIKey: "synthetic-old-key", Headers: map[string]string{"X-Fixture": "synthetic-old-header"}, ReplaceHeaders: true, Enabled: true, Models: []hub.ConnectionModel{{ID: "exact-old-model", Enabled: true}}}
	saved, err := a.Service.Connections.Save(t.Context(), u)
	if err != nil {
		t.Fatal(err)
	}
	u.ID, u.ExpectedRevision = saved.ID, saved.Revision
	probe, err := a.Service.Connections.TestConnection(t.Context(), u)
	if err != nil || probe.Scope != "model_catalog_access" || calls.Load() != 0 || catalogs.Load() != 1 || a.Service.Developer.Snapshot().ID != 0 {
		t.Fatal("connection catalog probe became inference", probe, err)
	}
	targets := a.Service.Developer.Targets()
	if len(targets) != 1 {
		t.Fatal("saved connection was not selectable in developer options")
	}
	request := developer.Request{Mode: developer.Test, Targets: []string{targets[0].ID}, ToolNames: []string{"ask_human"}, MaxOutputTokens: 321}
	if plan, err := a.Service.Developer.Plan(request); err != nil || plan.TotalPlannedCalls != 5 || calls.Load() != 0 {
		t.Fatal("cold preview invoked inference or lost tool matrix", plan, err)
	}
	closeApp(t, a)
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	if current := a.Service.Developer.Targets(); len(current) != 1 || current[0] != targets[0] || calls.Load() != 0 {
		t.Fatal("SQLite reopen changed revision identity or replayed inference")
	}
	if err = a.Service.Developer.Start(request); err != nil {
		t.Fatal(err)
	}
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("explicit Run did not reach actual HTTP fixture")
	}
	u.Endpoint, u.APIKey = server.URL+"/new", "synthetic-new-key"
	u.Headers = map[string]string{"X-Fixture": "synthetic-new-header"}
	u.Models = []hub.ConnectionModel{{ID: "exact-new-model", Enabled: true}}
	edited, err := a.Service.Connections.Save(t.Context(), u)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Connections.Save(t.Context(), u); !errors.Is(err, hub.ErrConnectionConflict) {
		t.Fatal("stale connection CAS unexpectedly succeeded", err)
	}
	if _, err = a.Service.Developer.Plan(request); err == nil {
		t.Fatal("edited connection still admitted old developer revision")
	}
	current := a.Service.Developer.Targets()
	if len(current) != 1 || current[0].ID == targets[0].ID {
		t.Fatal("new connection revision reused old developer identity")
	}
	next := request
	next.Targets = []string{current[0].ID}
	if err = a.Service.Developer.Start(next); err == nil {
		t.Fatal("second job was admitted while frozen job was active")
	}
	disabled, err := a.Service.Connections.SetEnabled(t.Context(), saved.ID, edited.Revision, false)
	if err != nil || len(a.Service.Developer.Targets()) != 0 {
		t.Fatal("disabled connection remained a future developer target", err)
	}
	if _, err = a.Service.Developer.Plan(next); err == nil {
		t.Fatal("disabled developer revision planned future execution")
	}
	releaseFirst()
	first := awaitDynamicDeveloper(t, a.Service.Developer)
	if first.State != "completed" || first.StartedCalls != 5 || first.Report.Reports[0].Passed != 5 || calls.Load() != 5 {
		t.Fatal("in-flight job drifted after connection edit/disable", first)
	}
	if _, err = a.Service.Connections.SetEnabled(t.Context(), saved.ID, disabled.Revision, true); err != nil {
		t.Fatal(err)
	}
	next.Targets = []string{a.Service.Developer.Targets()[0].ID}
	if err = a.Service.Developer.Start(next); err != nil {
		t.Fatal(err)
	}
	second := awaitDynamicDeveloper(t, a.Service.Developer)
	if second.State != "completed_with_failures" || second.StartedCalls != 5 || second.Report.Reports[0].Passed != 4 || second.Report.Reports[0].Failed != 1 || calls.Load() != 10 {
		t.Fatal("new revision did not reach real tool-argument grader", second)
	}
	encoded, _ := json.Marshal(struct {
		Connections []hub.ModelConnection
		Report      developer.Snapshot
	}{a.Service.Connections.Snapshot(), second})
	metadata, _ := json.Marshal([]any{a.Service.Developer.Targets(), first, second})
	if strings.Contains(string(encoded), "synthetic-") || strings.Contains(string(metadata), server.URL) {
		t.Fatal("private configuration escaped developer metadata/report")
	}
	sessions, err := a.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || a.Runtime.HasActive() || catalogs.Load() != 1 {
		t.Fatal("developer inference created a chat task or performed implicit catalog IO", err)
	}
}
