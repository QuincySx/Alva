// INPUT: Actual Hub assembly, isolated convention files, local SSE and durable session ports.
// OUTPUT: Default-off/merged Skill catalogs, real template children and immutable wait recipe regressions.
// POS: [L7 integration] Verifies the shared desktop/server composition without paid providers or global files.
package bootstrap

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
)

func compatibilityFile(t *testing.T, root, relative, text string) {
	t.Helper()
	filename := filepath.Join(root, filepath.FromSlash(relative))
	if err := os.MkdirAll(filepath.Dir(filename), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filename, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}

func compatibilityReply(w http.ResponseWriter, id, tool, args string) {
	w.Header().Set("Content-Type", "text/event-stream")
	data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": id, "type": "function", "function": map[string]string{"name": tool, "arguments": args}}}}, "finish_reason": "tool_calls"}}})
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
}

func TestHubCompatibilityIsOptionalMergesNativeSkillsAndDispatchesTemplate(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		switch calls.Add(1) {
		case 1:
			if bytes.Contains(data, []byte("HUB-ROOT-RULE")) || bytes.Contains(data, []byte("STANDARD-SKILL")) {
				t.Error("disabled compatibility loaded conventions")
			}
			waitReply(w, "", "", "off")
		case 2:
			if !bytes.Contains(data, []byte("HUB-ROOT-RULE")) || !bytes.Contains(data, []byte("STANDARD-SKILL")) || !bytes.Contains(data, []byte("NATIVE-SKILL")) || bytes.Contains(data, []byte("LAZY-BODY")) {
				t.Error("incorrect combined context", string(data))
			}
			if bytes.Count(data, []byte("STANDARD-SKILL")) != 1 {
				t.Error("matching native/standard skill metadata was injected twice")
			}
			var body struct {
				Tools []struct {
					Function struct {
						Name string `json:"name"`
					} `json:"function"`
				} `json:"tools"`
			}
			if err := json.Unmarshal(data, &body); err != nil {
				t.Error(err)
			}
			count := 0
			for _, tool := range body.Tools {
				if tool.Function.Name == "skill" {
					count++
				}
			}
			if count != 1 {
				t.Error("multiple Skill tools", count)
			}
			compatibilityReply(w, "compat-skill", "skill", `{"name":"standard"}`)
		case 3:
			if !bytes.Contains(data, []byte("LAZY-BODY")) {
				t.Error("selected Skill body absent")
			}
			compatibilityReply(w, "compat-agent", "agent", `{"task":"review","agent_type":"reviewer"}`)
		case 4:
			if !bytes.Contains(data, []byte("HUB-TEMPLATE-PROMPT")) || bytes.Contains(data, []byte("HUB-ROOT-RULE")) {
				t.Error("child not independently scoped", string(data))
			}
			waitReply(w, "", "", "child-complete")
		case 5:
			waitReply(w, "", "", "root-complete")
		default:
			t.Error("unexpected calls")
			waitReply(w, "", "", "extra")
		}
	}))
	defer server.Close()
	a, err := Open(t.Context(), waitConfig(t, server.URL))
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	s, err := a.Service.Create(t.Context(), "compatibility", "")
	if err != nil {
		t.Fatal(err)
	}
	compatibilityFile(t, s.Workspace, "AGENTS.md", "HUB-ROOT-RULE")
	compatibilityFile(t, s.Workspace, ".alva/skills/native/SKILL.md", "---\nname: native\ndescription: NATIVE-SKILL\n---\nNATIVE-LAZY-BODY")
	compatibilityFile(t, s.Workspace, ".agents/skills/standard/SKILL.md", "---\nname: standard\ndescription: STANDARD-SKILL\nallowed-tools: Bash\n---\nSTANDARD-LAZY-BODY")
	compatibilityFile(t, s.Workspace, ".alva/skills/standard/SKILL.md", "---\nname: standard\ndescription: STANDARD-SKILL\nallowed-tools: [Bash]\n---\nSTANDARD-LAZY-BODY")
	compatibilityFile(t, s.Workspace, ".claude/agents/reviewer.md", "---\nname: reviewer\ndescription: template reviewer\nmodel: inherit\n---\nHUB-TEMPLATE-PROMPT")
	compatibilityFile(t, s.Workspace, "compat.json", `{"version":1,"rules":["agents"],"skills":[{"path":".agents/skills","format":"agent-skills"}],"agents":[".claude/agents"]}`)
	settings, err := a.Store.Settings(t.Context(), s.ID)
	if err != nil || settings.CompatibilityFile != "" {
		t.Fatal(settings, err)
	}
	if _, err := a.Service.Submit(t.Context(), s.ID, "default off"); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, a, s.ID)
	if err := a.Store.SetSkillsEnabled(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	if err := a.Store.SetSubagentsEnabled(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	receipt, err := a.Service.InspectCompatibility(t.Context(), s.ID, "compat.json")
	if err != nil || len(receipt.Agents) != 1 || calls.Load() != 1 {
		t.Fatal(receipt, err, calls.Load())
	}
	if err := a.Service.SetCompatibilityFile(t.Context(), s.ID, "compat.json"); err != nil {
		t.Fatal(err)
	}
	if _, err := a.Service.Submit(t.Context(), s.ID, "opt in"); err != nil {
		t.Fatal(err)
	}
	approval := nextApproval(t, a, s.ID)
	if approval.ToolName != "agent" {
		t.Fatal("missing independent spawn consent", approval.ToolName)
	}
	if err := a.Service.SetCompatibilityFile(t.Context(), s.ID, ""); err == nil {
		t.Fatal("active compatibility recipe changed")
	}
	if err := a.Service.RespondApproval(t.Context(), s.ID, approval.ID, true); err != nil {
		t.Fatal(err)
	}
	detail := awaitTerminal(t, a, s.ID)
	if calls.Load() != 5 || detail.Session.Status != engine.Succeeded {
		t.Fatal(detail.Session.Status, calls.Load())
	}
	if err := a.Service.SetCompatibilityFile(t.Context(), s.ID, ""); err != nil {
		t.Fatal(err)
	}
	settings, err = a.Store.Settings(t.Context(), s.ID)
	if err != nil || settings.CompatibilityFile != "" || !settings.SkillsEnabled || !settings.SubagentsEnabled {
		t.Fatal("disable affected unrelated settings", settings, err)
	}
}

func TestHubCompatibilityWaitRejectsChangedRulesBeforeExecution(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) == 1 {
			waitReply(w, "await_external", `{"request":{"job":"result"}}`, "")
		} else {
			waitReply(w, "", "", "continued")
		}
	}))
	defer server.Close()
	a, err := Open(t.Context(), waitConfig(t, server.URL))
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	s, err := a.Service.Create(t.Context(), "frozen compatibility", "")
	if err != nil {
		t.Fatal(err)
	}
	compatibilityFile(t, s.Workspace, "AGENTS.md", "ORIGINAL")
	compatibilityFile(t, s.Workspace, "compat.json", `{"version":1,"rules":["agents"]}`)
	if err := a.Service.SetCompatibilityFile(t.Context(), s.ID, "compat.json"); err != nil {
		t.Fatal(err)
	}
	if err := a.Service.EnableDurableWaits(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	turn, err := a.Service.Submit(t.Context(), s.ID, "wait")
	if err != nil {
		t.Fatal(err)
	}
	view := waitStatus(t, a, s.ID, engine.Waiting)
	if err := a.Store.SetCompatibilityFile(t.Context(), s.ID, ""); err == nil {
		t.Fatal("waiting recipe edited")
	}
	resolveFirst(t, a, s.ID, view, `{"value":"done"}`)
	view, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	compatibilityFile(t, s.Workspace, "AGENTS.md", "CHANGED")
	if _, err := a.Service.ResumeWait(t.Context(), s.ID, waitTarget(view)); err == nil || calls.Load() != 1 {
		t.Fatal("changed instructions executed", err, calls.Load())
	}
	compatibilityFile(t, s.Workspace, "AGENTS.md", "ORIGINAL")
	resumed, err := a.Service.ResumeWait(t.Context(), s.ID, product.WaitTarget{BranchID: view.BranchID, TurnID: view.TurnID, Revision: view.Snapshot.Revision})
	if err != nil || resumed != turn {
		t.Fatal(resumed, err)
	}
	waitStatus(t, a, s.ID, engine.Succeeded)
	if calls.Load() != 2 {
		t.Fatal("replayed model calls", calls.Load())
	}
	if data, err := os.ReadFile(filepath.Join(s.Workspace, "AGENTS.md")); err != nil || strings.TrimSpace(string(data)) != "ORIGINAL" {
		t.Fatal(err)
	}
}
