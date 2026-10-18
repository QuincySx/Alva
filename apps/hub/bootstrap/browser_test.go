// INPUT: Explicit fixture Chrome, local HTTP/CDP pages and actual product/native SDK recipe
// OUTPUT: No-start settings, real HITL/browser tools, durable launch risk and physical cleanup regressions
// POS: [L7 Hub product] Fresh private local browser acceptance; never attaches to user profiles/accounts or a paid service
package bootstrap

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestNativeBrowserRecipeIsExplicitRunOwnedAndReferenceOnly(t *testing.T) {
	const executable = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
	if _, err := os.Stat(executable); err != nil {
		t.Skip("explicit fixture Chrome unavailable; no executable discovery or installation")
	}
	ctx := context.Background()
	var pageCalls, modelCalls atomic.Int32
	page := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		pageCalls.Add(1)
		fmt.Fprint(w, `<!doctype html><title>actual private browser fixture</title><p>bounded local browser text</p>`)
	}))
	defer page.Close()
	model := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
		if err != nil {
			t.Error(err)
		}
		round := modelCalls.Add(1)
		if bytes.Contains(body, []byte(`"type":"image_url"`)) {
			t.Error("browser screenshot automatically entered model input")
		}
		var delta map[string]any
		finish := "stop"
		if round <= 4 {
			name, args := "browser_start", map[string]any{}
			switch round {
			case 2:
				name, args = "browser_navigate", map[string]any{"url": page.URL}
			case 3:
				name, args = "browser_snapshot", map[string]any{"mode": "text"}
			case 4:
				name, args = "browser_screenshot", map[string]any{"format": "png"}
			}
			encoded, _ := json.Marshal(args)
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": fmt.Sprint("browser-call-", round), "type": "function", "function": map[string]any{"name": name, "arguments": string(encoded)}}}}
			finish = "tool_calls"
		} else {
			delta = map[string]any{"content": "actual private browser fixture done"}
		}
		data, _ := json.Marshal(map[string]any{"id": "actual-browser-model", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer model.Close()
	dir := filepath.Join(t.TempDir(), "private")
	a, err := Open(ctx, Config{DataDirectory: dir, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: model.URL, Model: "browser-fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	s, err := a.Service.Create(ctx, "actual browser", "")
	if err != nil {
		t.Fatal(err)
	}
	raw, _ := json.Marshal(hub.BrowserSettings{Executable: executable, Headless: true, Enabled: true, HostAccessApproved: true})
	if err = a.Service.SaveBrowserDraft(ctx, s.ID, string(raw)); err != nil {
		t.Fatal(err)
	}
	if a.Store.SetBrowserEnabled(ctx, s.ID, true, false) == nil {
		t.Fatal("browser host approval skipped")
	}
	if err = a.Store.SetBrowserEnabled(ctx, s.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(dir, "browser-profiles")); !os.IsNotExist(err) || pageCalls.Load() != 0 || modelCalls.Load() != 0 {
		t.Fatal("browser settings launched/scanned", err)
	}
	if _, err = a.Service.Submit(ctx, s.ID, "explicit actual local browser task"); err != nil {
		t.Fatal(err)
	}
	approval := nextApproval(t, a, s.ID)
	if approval.ToolName != "browser_start" {
		t.Fatal("wrong actual browser approval", approval.ToolName)
	}
	profiles, err := os.ReadDir(filepath.Join(dir, "browser-profiles"))
	if err != nil || len(profiles) != 0 || pageCalls.Load() != 0 {
		t.Fatal("browser process/profile created before actual approval", err)
	}
	if a.Store.SetBrowserEnabled(ctx, s.ID, false, false) == nil {
		t.Fatal("active browser recipe mutated")
	}
	if err = a.Service.RespondApproval(ctx, s.ID, approval.ID, true); err != nil {
		t.Fatal(err)
	}
	detail := awaitTerminal(t, a, s.ID)
	if modelCalls.Load() != 5 || pageCalls.Load() == 0 {
		t.Fatal("browser fixture did not execute actual local operations", modelCalls.Load(), pageCalls.Load())
	}
	profiles, err = os.ReadDir(filepath.Join(dir, "browser-profiles"))
	if err != nil || len(profiles) != 0 {
		t.Fatal("run ended before actual browser/profile cleanup", profiles, err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	log, err := a.Repository.OpenSession(ctx, s.ID, branch.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	records, err := log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	cleanupFound := false
	for _, record := range records {
		if record.Kind == abi.RecordExtension && record.Namespace == "alva.browser" && record.Type == "browser_cleanup" {
			var value struct {
				Stage    string
				Complete bool  `json:"cleanup_complete"`
				Pending  []any `json:"pending_instances"`
			}
			if json.Unmarshal(record.Payload, &value) != nil || value.Stage != "run_end" || !value.Complete || len(value.Pending) != 0 {
				t.Fatal("browser cleanup was not physically proven", string(record.Payload))
			}
			cleanupFound = true
		}
	}
	if !cleanupFound {
		t.Fatal("native browser cleanup facts missing")
	}
	for _, kind := range []string{"browser-snapshot", "browser-screenshot"} {
		page, err := a.Repository.PageArtifacts(ctx, s.ID, branch.NativeSessionID, kind, "", 0, 0, 4)
		if err != nil || len(page.Entries) != 1 {
			t.Fatal("actual browser source refs missing", kind, err)
		}
	}
	history, err := a.Store.History(ctx, s.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	activity := false
	for _, row := range history {
		if row.Event.Kind == engine.ExecutionActivity {
			activity = true
			if row.Event.Activity == nil || row.Event.Activity.Effect != engine.EffectUnknown {
				t.Fatal("browser risk omitted")
			}
		}
	}
	if !activity {
		t.Fatal("browser launch had no prior durable activity")
	}
}
