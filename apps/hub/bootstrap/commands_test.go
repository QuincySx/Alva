// INPUT: Same-binary native command fixtures, actual local model HTTP and explicit product approvals
// OUTPUT: Hook effective-argument HITL and on-demand diagnostic evidence/Activity regressions
// POS: [L7 Hub product] Full shared-recipe acceptance without host tools, paid calls or fake empty LSP success
package bootstrap

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestNativeCommandHelper(t *testing.T) {
	mode := os.Getenv("ALVA_PRODUCT_COMMAND_MODE")
	if mode == "" {
		return
	}
	if err := os.WriteFile(os.Getenv("ALVA_PRODUCT_COMMAND_MARKER"), []byte(mode), 0600); err != nil {
		os.Exit(7)
	}
	if mode == "hook" {
		var input map[string]json.RawMessage
		if json.NewDecoder(os.Stdin).Decode(&input) != nil {
			os.Exit(8)
		}
		fmt.Println(`{"permission_decision":"allow","updated_input":{"path":"effective.txt","content":"actual approved hook effect"}}`)
		os.Exit(0)
	}
	fmt.Println("actual diagnostic output — exit0 is not parsed diagnostics")
	fmt.Fprintln(os.Stderr, "actual diagnostic stderr tail")
	os.Exit(0)
}

func TestNativeHookAndDiagnosticSettingsRunOnlyAtApprovedBoundaries(t *testing.T) {
	for _, kind := range []string{"hook", "diagnostic"} {
		t.Run(kind, func(t *testing.T) {
			ctx := context.Background()
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				var delta map[string]any
				finish := "stop"
				if calls.Add(1) == 1 {
					name, args := "write_file", `{"path":"original.txt","content":"original model input"}`
					if kind == "diagnostic" {
						name, args = "project_diagnostics", `{"action":"run","profile_id":"fixture"}`
					}
					delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "command-call", "type": "function", "function": map[string]any{"name": name, "arguments": args}}}}
					finish = "tool_calls"
				} else {
					delta = map[string]any{"content": "completed actual command fixture"}
				}
				data, _ := json.Marshal(map[string]any{"id": "command-model-fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
			}))
			defer server.Close()
			a, err := Open(ctx, Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}})
			if err != nil {
				t.Fatal(err)
			}
			defer closeApp(t, a)
			session, err := a.Service.Create(ctx, kind, "")
			if err != nil {
				t.Fatal(err)
			}
			exe, err := os.Executable()
			if err != nil {
				t.Fatal(err)
			}
			marker := filepath.Join(t.TempDir(), "started")
			command := hub.NativeCommand{Command: exe, Args: []string{"-test.run=^TestNativeCommandHelper$", "--"}, Env: map[string]string{"ALVA_PRODUCT_COMMAND_MODE": kind, "ALVA_PRODUCT_COMMAND_MARKER": marker}, TimeoutMS: 5000, Enabled: true, HostAccessApproved: true}
			settings := hub.CommandSettings{}
			if kind == "hook" {
				settings.Hooks = []hub.HookSpec{{ID: "fixture", Event: "PreToolUse", Matcher: "write_file", NativeCommand: command}}
			} else {
				settings.Diagnostics = []hub.DiagnosticSpec{{ID: "fixture", Description: "local test-only diagnostic", NativeCommand: command}}
			}
			raw, _ := json.Marshal(settings)
			if err = a.Service.SaveCommandsDraft(ctx, session.ID, string(raw)); err != nil {
				t.Fatal(err)
			}
			saved, err := a.Store.Settings(ctx, session.ID)
			if err != nil {
				t.Fatal(err)
			}
			if kind == "hook" && saved.Commands.Hooks[0].Enabled || kind == "diagnostic" && saved.Commands.Diagnostics[0].Enabled {
				t.Fatal("native command draft self-approved")
			}
			if err = a.Store.SetCommandEnabled(ctx, session.ID, kind, "fixture", true, false); err == nil {
				t.Fatal("independent host approval bypassed")
			}
			if err = a.Store.SetCommandEnabled(ctx, session.ID, kind, "fixture", true, true); err != nil {
				t.Fatal(err)
			}
			if _, err = os.Stat(marker); !os.IsNotExist(err) || calls.Load() != 0 {
				t.Fatal("save/enable started native command or model", err)
			}
			if _, err = a.Service.Submit(ctx, session.ID, "actual local command fixture"); err != nil {
				t.Fatal(err)
			}
			approval := nextApproval(t, a, session.ID)
			if kind == "hook" {
				if approval.ToolName != "write_file" || !strings.Contains(approval.InputSummary, "effective.txt") {
					t.Fatal("Hook allow or prior-input approval bypassed effective permissions", approval)
				}
				for _, name := range []string{"original.txt", "effective.txt"} {
					if _, err = os.Stat(filepath.Join(session.Workspace, name)); !os.IsNotExist(err) {
						t.Fatal("Tool wrote before actual approval", name, err)
					}
				}
			} else {
				if approval.ToolName != "project_diagnostics" {
					t.Fatal("wrong actual diagnostic approval", approval.ToolName)
				}
				if _, err = os.Stat(marker); !os.IsNotExist(err) {
					t.Fatal("diagnostic started before tool consent", err)
				}
			}
			if err = a.Store.SetCommandEnabled(ctx, session.ID, kind, "fixture", false, false); err == nil {
				t.Fatal("active command recipe mutated")
			}
			if err = a.Service.RespondApproval(ctx, session.ID, approval.ID, true); err != nil {
				t.Fatal(err)
			}
			detail := awaitTerminal(t, a, session.ID)
			if _, err = os.Stat(marker); err != nil {
				t.Fatal("approved actual command did not start", err)
			}
			if calls.Load() != 2 {
				t.Fatal("unexpected automatic replay", calls.Load())
			}
			if kind == "hook" {
				data, e := os.ReadFile(filepath.Join(session.Workspace, "effective.txt"))
				if e != nil || string(data) != "actual approved hook effect" {
					t.Fatal("effective hook input not executed", e)
				}
				if _, e = os.Stat(filepath.Join(session.Workspace, "original.txt")); !os.IsNotExist(e) {
					t.Fatal("original pre-hook path executed")
				}
			}
			branch, err := product.ActiveBranch(detail)
			if err != nil {
				t.Fatal(err)
			}
			log, err := a.Repository.OpenSession(ctx, session.ID, branch.NativeSessionID)
			if err != nil {
				t.Fatal(err)
			}
			records, err := log.Records(ctx, 0)
			if err != nil {
				t.Fatal(err)
			}
			found := false
			for _, record := range records {
				if kind == "hook" && record.ToolIntent != nil && record.ToolIntent.Call.Name == "write_file" {
					if record.ToolIntent.DeclaredCall == nil || !strings.Contains(string(record.ToolIntent.DeclaredCall.Arguments), "original.txt") || !strings.Contains(string(record.ToolIntent.Call.Arguments), "effective.txt") {
						t.Fatal("pre/effective facts lost")
					}
				}
				if record.Kind == abi.RecordExtension && record.Namespace == "alva.tooling" && record.Type == "diagnostic_outcome" {
					var report struct {
						Status            string
						ExitCode          int `json:"exit_code"`
						Started, Reaped   bool
						CapturedComplete  bool `json:"captured_complete"`
						DiagnosticsParsed bool `json:"diagnostics_parsed"`
					}
					if json.Unmarshal(record.Payload, &report) != nil || report.Status != "completed" || report.ExitCode != 0 || !report.Started || !report.Reaped || !report.CapturedComplete || report.DiagnosticsParsed {
						t.Fatal("fake empty diagnostic evidence", string(record.Payload))
					}
					found = true
				}
				if record.Kind == abi.RecordExtension && record.Namespace == "alva.hooks" && record.Type == "hook_outcome" {
					found = true
				}
			}
			if !found {
				t.Fatal("native command evidence missing")
			}
			rows, err := a.Store.History(ctx, session.ID, 0, 128)
			if err != nil {
				t.Fatal(err)
			}
			activity := false
			for _, row := range rows {
				if row.Event.Kind == engine.ExecutionActivity {
					activity = true
					if row.Event.Activity == nil || row.Event.Activity.Effect != engine.EffectUnknown {
						t.Fatal("command risk omitted")
					}
				}
			}
			if !activity {
				t.Fatal("actual command start lacked product activity")
			}
		})
	}
}
