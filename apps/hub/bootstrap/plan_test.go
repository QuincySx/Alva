// INPUT: Real local HTTP model, shared bootstrap, persistent SQLite settings and independently approved lifecycle commands.
// OUTPUT: Read-only plan execution, omitted command startup, explicit Ask restoration and frozen durable-wait regression.
// POS: [L7 native acceptance] Plan is enforced by the actual Guard and per-execution recipe rather than a presentation toggle.
package bootstrap

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestNativePlanHookHelper(t *testing.T) {
	marker := os.Getenv("ALVA_PLAN_HOOK_MARKER")
	if marker == "" {
		return
	}
	if err := os.WriteFile(marker, []byte("actual hook"), 0600); err != nil {
		os.Exit(7)
	}
	fmt.Print("{}")
	os.Exit(0)
}

func TestPlanModeRealGuardReadWriteAndExplicitAskRestoration(t *testing.T) {
	var calls atomic.Int32
	reply := func(w http.ResponseWriter, id, tool, args, text string) {
		response := httptest.NewRecorder()
		waitReply(response, tool, args, text)
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte(strings.ReplaceAll(response.Body.String(), "wait-call", id)))
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body json.RawMessage
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		switch calls.Add(1) {
		case 1:
			if !strings.Contains(string(body), "Plan mode is enabled for this entire execution") {
				t.Error("planning intent absent from actual provider request")
			}
			reply(w, "read-call", "read_file", `{"path":"source.txt"}`, "")
		case 2:
			reply(w, "write-call", "write_file", `{"path":"result.txt","content":"only after explicit Ask approval"}`, "")
		case 3:
			// The denied plan-mode write reached the model as a tool result and
			// the plan execution carries on to a final answer.
			if !strings.Contains(string(body), "Plan mode is on: writes are disabled") {
				t.Error("plan-mode denial did not reach the model")
			}
			waitReply(w, "", "", "Here is the plan.")
		case 4:
			if strings.Contains(string(body), "Plan mode is enabled for this entire execution") {
				t.Error("new Ask execution retained a prior plan system policy")
			}
			waitReply(w, "write_file", `{"path":"result.txt","content":"only after explicit Ask approval"}`, "")
		default:
			waitReply(w, "", "", "Explicitly approved write completed.")
		}
	}))
	defer server.Close()
	config := waitConfig(t, server.URL)
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	p, err := createLive(a.Service, t.Context(), "real plan", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(p.Workspace, "source.txt"), []byte("readable planning evidence"), 0600); err != nil {
		t.Fatal(err)
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "hook-started")
	command := hub.NativeCommand{Command: exe, Args: []string{"-test.run=^TestNativePlanHookHelper$", "--"}, Env: map[string]string{"ALVA_PLAN_HOOK_MARKER": marker}, TimeoutMS: 5000}
	if err = a.Store.ReplaceCommands(t.Context(), p.ID, hub.CommandSettings{Hooks: []hub.HookSpec{{ID: "fixture", Event: "SessionStart", NativeCommand: command}}}); err != nil {
		t.Fatal(err)
	}
	if err = a.Store.SetCommandEnabled(t.Context(), p.ID, "hook", "fixture", true, true); err != nil {
		t.Fatal(err)
	}
	if err = a.Service.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	// Reopen proves bootstrap consumes the committed choice rather than UI memory.
	closeApp(t, a)
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 {
		t.Fatal("save/reopen started model")
	}
	if _, err = a.Service.Submit(t.Context(), p.ID, "Inspect and propose a plan", nil); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, a, p.ID, engine.Succeeded)
	if calls.Load() != 3 {
		t.Fatal("plan execution made unexpected calls", calls.Load())
	}
	if _, err = os.Stat(filepath.Join(p.Workspace, "result.txt")); !os.IsNotExist(err) {
		t.Fatal("plan write reached workspace", err)
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("plan started approved lifecycle command", err)
	}
	detail, err := a.Store.Detail(t.Context(), p.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	log, err := a.Repository.OpenSession(t.Context(), p.ID, branch.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	readSucceeded := false
	for _, record := range records {
		if record.ToolOutcome != nil && !record.ToolOutcome.Result.IsError {
			for _, block := range record.ToolOutcome.Result.Content {
				if block.Reference != nil {
					readSucceeded = true
				}
			}
		}
	}
	if !readSucceeded {
		t.Fatal("Plan did not permit the actual file read")
	}
	settings, err := a.Store.Settings(t.Context(), p.ID)
	if err != nil || !settings.Commands.Hooks[0].Enabled || !settings.Commands.Hooks[0].HostAccessApproved {
		t.Fatal("execution projection changed saved command approvals", err)
	}
	if err = a.Service.SetPlanMode(t.Context(), p.ID, false); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 3 {
		t.Fatal("exiting Plan executed the model")
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("exiting Plan executed a lifecycle command")
	}
	if _, err = a.Service.Submit(t.Context(), p.ID, "Now request a change", nil); err != nil {
		t.Fatal(err)
	}
	approval := nextApproval(t, a, p.ID)
	if approval.ToolName != "write_file" {
		t.Fatal("Ask did not restore real write approval", approval.ToolName)
	}
	if _, err = os.Stat(filepath.Join(p.Workspace, "result.txt")); !os.IsNotExist(err) {
		t.Fatal("exit from Plan authorized a write", err)
	}
	if err = a.Service.SetPlanMode(t.Context(), p.ID, true); err == nil {
		t.Fatal("running Ask recipe changed")
	}
	if err = a.Service.RespondApproval(t.Context(), p.ID, approval.ID, true); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, a, p.ID, engine.Succeeded)
	data, err := os.ReadFile(filepath.Join(p.Workspace, "result.txt"))
	if err != nil || string(data) != "only after explicit Ask approval" {
		t.Fatal("explicitly approved Ask write did not execute", err)
	}
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("saved lifecycle command was not restored on explicit Send", err)
	}
}

func TestPlanProjectionOmitsUnsafeIntegrationsAndDeniesNonReadEffects(t *testing.T) {
	stored := hub.Settings{PlanMode: true, MCP: []hub.MCPServer{{Enabled: true}}, AEP: []hub.AEPBundle{{Enabled: true}}, Commands: hub.CommandSettings{Hooks: []hub.HookSpec{{}}, Diagnostics: []hub.DiagnosticSpec{{}}}, Browser: hub.BrowserSettings{Enabled: true}, MediaEnabled: true, SubagentsEnabled: true, Tools: hub.ToolSettings{Planning: true, Shell: hub.ShellSettings{Enabled: true}, Web: hub.WebSettings{Enabled: true}}}
	before, _ := json.Marshal(stored)
	execution := planExecutionSettings(stored)
	if len(execution.MCP)+len(execution.AEP)+len(execution.Commands.Hooks)+len(execution.Commands.Diagnostics) != 0 || execution.Browser.Enabled || execution.MediaEnabled || execution.SubagentsEnabled || execution.Tools.Shell.Enabled || execution.Tools.Web.Enabled || !execution.Tools.Planning {
		t.Fatal("unsafe optional integration survived Plan recipe", execution)
	}
	after, _ := json.Marshal(stored)
	if !reflect.DeepEqual(before, after) {
		t.Fatal("Plan recipe projection mutated stored snapshot")
	}
	manager := security.NewPermissionManager(security.Plan, security.ApprovalFunc(func(_ context.Context, _ security.ApprovalRequest) (security.ApprovalDecision, error) {
		t.Error("Plan sought approval for a forbidden effect")
		return security.ApprovalDecision{Allowed: true}, nil
	}))
	for _, effect := range []abi.EffectKind{abi.EffectRead, abi.EffectWrite, abi.EffectExecute, abi.EffectNetwork, abi.EffectUnknown} {
		err := manager.Authorize(t.Context(), security.ApprovalRequest{SessionID: "actual", ToolName: "tool", Effect: abi.Effect{Kind: effect}})
		if (effect == abi.EffectRead) != (err == nil) {
			t.Fatal("wrong Plan effect decision", effect, err)
		}
	}
}

func TestPlanModeProducesAPlanThroughTheActualModel(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		waitReply(w, "", "", "1. Inspect the relevant files. 2. Propose the smallest change. 3. Ask the user to leave Plan before executing.")
	}))
	defer server.Close()
	a, err := Open(t.Context(), waitConfig(t, server.URL))
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	p, err := createLive(a.Service, t.Context(), "normal plan", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Service.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(t.Context(), p.ID, "Produce a plan", nil); err != nil {
		t.Fatal(err)
	}
	waitStatus(t, a, p.ID, engine.Succeeded)
	if calls.Load() != 1 {
		t.Fatal("textual planning created unexpected additional requests", calls.Load())
	}
}

func TestPlanModeParticipatesInFrozenWaitRecipe(t *testing.T) {
	workspace := t.TempDir()
	settings := hub.Settings{Version: 1, SessionID: "saved"}
	config := Config{Demo: true}
	target := hub.Target{EngineID: "alva"}
	ask, err := hubWaitRecipe(config, workspace, target, settings, "")
	if err != nil {
		t.Fatal(err)
	}
	settings.PlanMode = true
	plan, err := hubWaitRecipe(config, workspace, target, settings, "")
	if err != nil || plan == ask {
		t.Fatal("wait recipe omitted the actual permission choice", err)
	}
	settings.PlanMode = false
	data, err := json.Marshal(settings)
	if err != nil || strings.Contains(string(data), "plan_mode") {
		t.Fatal("default Ask added a zero-value recipe field", err)
	}
}

func TestPlanModeCannotChangeFrozenDurableWaitRecipe(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) == 1 {
			waitReply(w, "await_external", `{"request":{"job":"plan-freeze"}}`, "")
		} else {
			waitReply(w, "", "", "same Turn resumed")
		}
	}))
	defer server.Close()
	a, err := Open(t.Context(), waitConfig(t, server.URL))
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	p, err := createLive(a.Service, t.Context(), "frozen mode", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Service.EnableDurableWaits(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	turn, err := a.Service.Submit(t.Context(), p.ID, "wait explicitly", nil)
	if err != nil {
		t.Fatal(err)
	}
	view := waitStatus(t, a, p.ID, engine.Waiting)
	if err = a.Service.SetPlanMode(t.Context(), p.ID, true); err == nil {
		t.Fatal("waiting mode changed through service")
	}
	if err = a.Store.SetPlanMode(t.Context(), p.ID, true); err == nil {
		t.Fatal("waiting mode changed through atomic storage")
	}
	settings, err := a.Store.Settings(t.Context(), p.ID)
	if err != nil || settings.PlanMode || calls.Load() != 1 {
		t.Fatal("rejected change mutated mode or resumed model", err)
	}
	resolveFirst(t, a, p.ID, view, `{"value":"continue original recipe"}`)
	view, err = a.Service.Waits(t.Context(), p.ID)
	if err != nil || !view.CanResume {
		t.Fatal("rejected change invalidated original checkpoint", view, err)
	}
	resumed, err := a.Service.ResumeWait(t.Context(), p.ID, waitTarget(view))
	if err != nil || resumed != turn {
		t.Fatal("original Turn did not resume", resumed, err)
	}
	waitStatus(t, a, p.ID, engine.Succeeded)
}
