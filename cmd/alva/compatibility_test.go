// INPUT: Shipped CLI flags, isolated project conventions and real local SSE/SQLite child execution.
// OUTPUT: Cold/default-off selection, progressive Skill loading, real template dispatch and durable change rejection.
// POS: [L6 application] Verifies opt-in through actual native assembly without accounts or ambient configuration.
package main

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
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/extension/compatibility"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

func compatPut(t *testing.T, root, relative, text string) {
	t.Helper()
	name := filepath.Join(root, filepath.FromSlash(relative))
	if err := os.MkdirAll(filepath.Dir(name), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(name, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}

func cliCompatibilityReply(w http.ResponseWriter, id, tool, args string) {
	w.Header().Set("Content-Type", "text/event-stream")
	data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": id, "type": "function", "function": map[string]string{"name": tool, "arguments": args}}}}, "finish_reason": "tool_calls"}}})
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
}

func TestCLICompatibilityIsColdOptInAndSkillBodyIsLazy(t *testing.T) {
	workspace := t.TempDir()
	compatPut(t, workspace, "AGENTS.md", "CLI-PROJECT-RULE")
	compatPut(t, workspace, ".agents/skills/review/SKILL.md", "---\nname: review\ndescription: CLI-SKILL-TRIGGER\nallowed-tools: Bash\ncompatibility: Compiler required\n---\nCLI-LAZY-INSTRUCTIONS")
	compatPut(t, workspace, ".alva/skills/review/SKILL.md", "---\nname: review\ndescription: CLI-SKILL-TRIGGER\nallowed-tools: [Bash]\ncompatibility: Compiler required\n---\nCLI-LAZY-INSTRUCTIONS")
	compatPut(t, workspace, ".alva/compatibility.json", `{"version":1,"rules":["agents"],"skills":[{"path":".agents/skills","format":"agent-skills"},{"path":".alva/skills","format":"alva"}]}`)
	out, _, err := workflowRun(t, "--workspace", workspace, "--inspect-compat", ".alva/compatibility.json")
	var receipt compatibility.Receipt
	if err != nil || json.Unmarshal([]byte(out), &receipt) != nil || len(receipt.SHA256) != 64 || len(receipt.Skills) != 1 || strings.Contains(out, "CLI-LAZY-INSTRUCTIONS") {
		t.Fatal(out, err)
	}
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, err := io.ReadAll(r.Body)
		if err != nil {
			t.Error(err)
		}
		switch calls.Add(1) {
		case 1:
			if bytes.Contains(data, []byte("CLI-PROJECT-RULE")) || bytes.Contains(data, []byte("CLI-SKILL-TRIGGER")) {
				t.Error("default CLI loaded convention files", string(data))
			}
			workflowReply(w, "", "", "off")
		case 2:
			if !bytes.Contains(data, []byte("CLI-PROJECT-RULE")) || !bytes.Contains(data, []byte("CLI-SKILL-TRIGGER")) || bytes.Contains(data, []byte("CLI-LAZY-INSTRUCTIONS")) {
				t.Error("incorrect progressive prompt", string(data))
			}
			if bytes.Count(data, []byte("CLI-SKILL-TRIGGER")) != 1 {
				t.Error("duplicate compatibility Skill metadata")
			}
			workflowReply(w, "skill", `{"name":"review"}`, "")
		case 3:
			if !bytes.Contains(data, []byte("CLI-LAZY-INSTRUCTIONS")) || !bytes.Contains(data, []byte("Compiler required")) || !bytes.Contains(data, []byte("tool_declaration_grants_host_access")) {
				t.Error("actual Skill result missing", string(data))
			}
			workflowReply(w, "", "", "loaded")
		default:
			t.Error("unexpected model call")
			workflowReply(w, "", "", "extra")
		}
	}))
	defer server.Close()
	args := []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--workspace", workspace, "--prompt", "go"}
	if _, _, err := workflowRun(t, args...); err != nil {
		t.Fatal(err)
	}
	out, diag, err := workflowRun(t, append(args, "--compat-config", ".alva/compatibility.json")...)
	if err != nil || !strings.Contains(out, "loaded") || calls.Load() != 3 {
		t.Fatal(out, diag, err, calls.Load())
	}
	for _, args := range [][]string{{"--compat-config", "../escape"}, {"--workspace", workspace, "--compat-config", ".alva/compatibility.json", "--sandbox", "wasm"}, {"--workspace", workspace, "--inspect-compat", ".alva/compatibility.json", "--prompt", "must-not-run"}} {
		if _, _, err := workflowRun(t, args...); err == nil {
			t.Fatal("accepted invalid compatibility mode", args)
		}
	}
}

func TestCLICompatibilityTemplateCreatesActualIndependentChild(t *testing.T) {
	ctx := t.Context()
	workspace := t.TempDir()
	dir := filepath.Join(t.TempDir(), "private")
	compatPut(t, workspace, "AGENTS.md", "ROOT-ONLY-RULE")
	compatPut(t, workspace, "note.txt", "actual-child-file")
	compatPut(t, workspace, ".claude/agents/reviewer.md", "---\nname: reviewer\ndescription: read a file\ntools: Read\nmodel: inherit\n---\nACTUAL-TEMPLATE-PROMPT")
	compatPut(t, workspace, "compat.json", `{"version":1,"rules":["agents"],"agents":[".claude/agents"]}`)
	var calls, approvals atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		switch calls.Add(1) {
		case 1:
			workflowReply(w, "agent", `{"task":"inspect note","agent_type":"reviewer"}`, "")
		case 2:
			if !bytes.Contains(data, []byte("ACTUAL-TEMPLATE-PROMPT")) || bytes.Contains(data, []byte("ROOT-ONLY-RULE")) {
				t.Error("child prompt not separately bound", string(data))
			}
			workflowReply(w, "read_file", `{"path":"note.txt"}`, "")
		case 3:
			var body struct {
				Messages []struct {
					Role    string
					Content json.RawMessage
				}
			}
			if err := json.Unmarshal(data, &body); err != nil {
				t.Error(err)
			}
			var text string
			if len(body.Messages) == 0 || json.Unmarshal(body.Messages[len(body.Messages)-1].Content, &text) != nil {
				t.Error("missing child read reference")
			}
			start, end := strings.Index(text, "{"), strings.Index(text, "}")
			var ref struct {
				URI string `json:"uri"`
			}
			if start < 0 || end < start || json.Unmarshal([]byte(text[start:end+1]), &ref) != nil || ref.URI == "" {
				t.Error("file read did not return a session reference")
			}
			args, _ := json.Marshal(map[string]string{"uri": ref.URI})
			cliCompatibilityReply(w, "child-reference-read", "context_read", string(args))
		case 4:
			if !bytes.Contains(data, []byte("actual-child-file")) {
				t.Error("selected child reference did not contain actual file")
			}
			workflowReply(w, "", "", "child-done")
		case 5:
			workflowReply(w, "", "", "root-done")
		default:
			t.Error("unexpected recursion")
			workflowReply(w, "", "", "extra")
		}
	}))
	defer server.Close()
	repo, err := session.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	native, err := openCLISession(ctx, repo, "")
	if err != nil {
		t.Fatal(err)
	}
	model, err := provider.Build(provider.OpenAIChatKind, provider.Config{Endpoint: server.URL, Model: "fixture"})
	if err != nil {
		t.Fatal(err)
	}
	owner, err := newCLINative(ctx, cliNativeConfig{Model: model, Repository: repo, Native: native, Workspace: workspace, DataDirectory: dir, Mode: security.Ask, CompatibilityFile: "compat.json", Approval: security.ApprovalFunc(func(_ context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals.Add(1)
		if request.ToolName != "agent" {
			t.Error("unexpected authority request", request.ToolName)
		}
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close(context.WithoutCancel(ctx))
	var output bytes.Buffer
	if err := runRunner(ctx, owner, "delegate", &output); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 5 || approvals.Load() != 1 || !strings.Contains(output.String(), "root-done") {
		t.Fatal(calls.Load(), approvals.Load(), output.String())
	}
	list, err := repo.List(ctx, cliOwner, "", 10)
	if err != nil || len(list.Sessions) != 2 {
		t.Fatal("child was not independently persisted", list, err)
	}
	for _, child := range list.Sessions {
		if child.ID != native.ID() {
			link, err := repo.ParentLink(ctx, cliOwner, child.ID)
			if err != nil || link.ParentID != native.ID() {
				t.Fatal("typed child ownership missing", link, err)
			}
		}
	}
}

func TestCLICompatibilityChangedInstructionsCannotResumeDurableWait(t *testing.T) {
	workspace := t.TempDir()
	dir := filepath.Join(t.TempDir(), "private")
	compatPut(t, workspace, "AGENTS.md", "INITIAL-RULE")
	compatPut(t, workspace, "compat.json", `{"version":1,"rules":["agents"]}`)
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) == 1 {
			workflowReply(w, "await_external", `{"request":{"job":"result"}}`, "")
		} else {
			workflowReply(w, "", "", "continued")
		}
	}))
	defer server.Close()
	args := []string{"--live", "--endpoint", server.URL, "--model", "fixture", "--workspace", workspace, "--compat-config", "compat.json", "--data-dir", dir}
	if out, _, err := workflowRun(t, append(args, "--durable", "--prompt", "wait")...); err != nil || !strings.Contains(out, `"status":"waiting"`) {
		t.Fatal(out, err)
	}
	id := workflowSession(t, dir)
	out, _, err := workflowRun(t, "--data-dir", dir, "--session", id, "--waits")
	if err != nil {
		t.Fatal(err)
	}
	var view durable.Snapshot
	if err := json.Unmarshal([]byte(out), &view); err != nil || len(view.State.Requests) != 1 {
		t.Fatal(out, err)
	}
	request := view.State.Requests[0]
	result := filepath.Join(t.TempDir(), "result.json")
	workflowJSONFile(t, result, map[string]string{"value": "done"})
	if _, _, err := workflowRun(t, "--data-dir", dir, "--session", id, "--resolve-wait", request.ID, "--wait-binding", request.Binding, "--wait-result-file", result); err != nil {
		t.Fatal(err)
	}
	compatPut(t, workspace, "AGENTS.md", "CHANGED-RULE")
	resume := append(append([]string{}, args...), "--session", id, "--resume-wait")
	if _, _, err := workflowRun(t, resume...); err == nil || calls.Load() != 1 {
		t.Fatal("changed instructions continued", err, calls.Load())
	}
	compatPut(t, workspace, "AGENTS.md", "INITIAL-RULE")
	if out, diag, err := workflowRun(t, resume...); err != nil || calls.Load() != 2 || !strings.Contains(out, "continued") {
		t.Fatal(out, diag, err, calls.Load())
	}
}
