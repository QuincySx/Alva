// INPUT: Isolated project convention files, actual SDK loop and fixed-session Skill tools.
// OUTPUT: Explicit scope/precedence, frozen instructions, lazy resources and boundary/authority regressions.
// POS: [L4 extension] Exercises the opt-in plugin without accounts, imported execution policy or core changes.
package compatibility

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func put(t *testing.T, root, relative, text string) {
	t.Helper()
	name := filepath.Join(root, filepath.FromSlash(relative))
	if err := os.MkdirAll(filepath.Dir(name), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(name, []byte(text), 0600); err != nil {
		t.Fatal(err)
	}
}

type captureModel struct{ request abi.ModelRequest }

func (m *captureModel) Stream(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	m.request = request
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func TestCompatibilityScopeFrozenInstructionsAndActualPlugin(t *testing.T) {
	ctx := t.Context()
	root := t.TempDir()
	put(t, root, "AGENTS.md", "OUTER @/unselected/home/private.md")
	put(t, root, "CLAUDE.md", "SAME-DIR-LATER")
	put(t, root, "src/AGENTS.md", "INNER")
	put(t, root, "src/GEMINI.md", "SELECTED-GEMINI")
	put(t, root, "unrelated/AGENTS.md", "MUST-NOT-LOAD")
	put(t, root, ".agents/skills/review/SKILL.md", "---\nname: review\ndescription: Skill trigger\nallowed-tools: Bash\ncompatibility: Need a compiler\n---\nSECRET-LAZY-BODY")
	put(t, root, ".agents/skills/review/references/note.md", "SELECTED-RESOURCE")
	put(t, root, ".claude/agents/reviewer.md", "---\nname: reviewer\ndescription: review only\ntools: Read, Grep\nmodel: inherit\n---\nREVIEWER-PROMPT")
	config := Config{Version: 1, Scope: "src", Rules: []string{"agents", "claude", "gemini"}, Skills: []SkillSource{{".agents/skills", "agent-skills"}}, Agents: []string{".claude/agents"}}
	s, err := Open(ctx, root, config)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close(ctx)
	if !strings.Contains(s.prompt, "OUTER") || !strings.Contains(s.prompt, "SELECTED-GEMINI") || strings.Index(s.prompt, "OUTER") > strings.Index(s.prompt, "SAME-DIR-LATER") || strings.Index(s.prompt, "SAME-DIR-LATER") > strings.Index(s.prompt, "INNER") || strings.Contains(s.prompt, "MUST-NOT-LOAD") {
		t.Fatal(s.prompt)
	}
	first := s.Fingerprint()
	put(t, root, ".agents/skills/review/SKILL.md", "---\nname: review\ndescription: changed\n---\nCHANGED-BODY")
	doc, err := s.Load(ctx, "review")
	if err != nil || doc.Body != "SECRET-LAZY-BODY" {
		t.Fatal(doc, err)
	}
	doc.Meta.AllowedTools[0] = "write_file"
	doc.Meta.Metadata = map[string]any{"bad": true}
	again, _ := s.Load(ctx, "review")
	if again.Meta.AllowedTools[0] != "Bash" {
		t.Fatal("snapshot mutated through returned metadata")
	}
	second, err := Open(ctx, root, config)
	if err != nil {
		t.Fatal(err)
	}
	defer second.Close(ctx)
	if first == second.Fingerprint() {
		t.Fatal("changed instructions retained durable recipe fingerprint")
	}
	template, ok := s.Templates().Get("reviewer")
	if !ok || template.SystemPrompt != "REVIEWER-PROMPT" || strings.Join(template.Tools, ",") != "read_file,search_files" || template.Model != "" {
		t.Fatal(template)
	}
	refs, _ := agentcontext.NewMemoryReferences("native", 1<<20)
	plugin := s.Plugin("native", refs, nil)
	model := &captureModel{}
	agent, err := core.NewBuilder(model, core.NewMemorySession("native")).Plugin(plugin).Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "go")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	request, _ := json.Marshal(model.request)
	if !strings.Contains(string(request), "OUTER") || !strings.Contains(string(request), "Skill trigger") || !strings.Contains(string(request), "reviewer") || !strings.Contains(string(request), "review only") || strings.Contains(string(request), "SECRET-LAZY-BODY") || strings.Contains(string(request), "REVIEWER-PROMPT") {
		t.Fatal("eager/incomplete context", string(request))
	}
	tools := plugin.Tools()
	if len(tools) != 1 || tools[0].Name() != "skill" {
		t.Fatal("declarations granted tools", tools)
	}
	call := wire.ToolCall{ID: "load", Name: "skill", Arguments: json.RawMessage(`{"name":"review"}`)}
	result, err := tools[0].Execute(ctx, abi.ToolExecutionContext{SessionID: "native", ActualTool: tools[0], CallID: call.ID}, call)
	data, _ := json.Marshal(result)
	if err != nil || !strings.Contains(string(data), "SECRET-LAZY-BODY") {
		t.Fatal(result, err)
	}
	if _, err := tools[0].Execute(ctx, abi.ToolExecutionContext{SessionID: "foreign", ActualTool: tools[0], CallID: call.ID}, call); err == nil {
		t.Fatal("foreign session borrowed Skill tool")
	}
	resource, err := s.Resource(ctx, "review", "references/note.md")
	if err != nil || string(resource.Bytes) != "SELECTED-RESOURCE" {
		t.Fatal(resource, err)
	}
	if _, err := s.Resource(ctx, "review", "../SKILL.md"); err == nil {
		t.Fatal("resource escaped")
	}
	if err := s.Close(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Load(ctx, "review"); err == nil {
		t.Fatal("closed snapshot readable")
	}
}

func TestCompatibilityDeniesAmbiguousConfigEscapesAndExecutionPolicy(t *testing.T) {
	for _, raw := range []string{`null`, `{"version":1,"rules":["agents"],"version":1}`, `{"Version":1,"rules":["agents"]}`, `{"version":1,"rules":["agents"],"scope":"../private"}`, `{"version":1,"skills":[{"path":"/home/private","format":"agent-skills"}]}`, `{"version":1,"rules":["agents","agents"]}`, `{"version":1,"rules":["agents"],"exec":"bad"}`} {
		if _, err := ParseConfig([]byte(raw)); err == nil {
			t.Fatal("accepted ambiguous config", raw)
		}
	}
	root := t.TempDir()
	outside := t.TempDir()
	put(t, outside, "AGENTS.md", "EXTERNAL")
	if err := os.Symlink(filepath.Join(outside, "AGENTS.md"), filepath.Join(root, "AGENTS.md")); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(t.Context(), root, Config{Version: 1, Rules: []string{"agents"}}); err == nil {
		t.Fatal("followed rule symlink")
	}
	if err := os.Symlink(outside, filepath.Join(root, "escape")); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(t.Context(), root, Config{Version: 1, Scope: "escape", Rules: []string{"agents"}}); err == nil {
		t.Fatal("followed ancestor symlink")
	}
	put(t, root, ".claude/agents/unsafe.md", "---\nname: unsafe\ndescription: unsafe\npermissionMode: bypassPermissions\n---\nbody")
	if _, err := Open(t.Context(), root, Config{Version: 1, Agents: []string{".claude/agents"}}); err == nil {
		t.Fatal("ignored unsupported execution policy")
	}
	for _, extra := range []string{"model: sonnet", "hooks: {}", "skills: [secret]", "tools: Read(*)"} {
		if _, err := parseAgent([]byte("---\nname: unsafe\ndescription: test\n" + extra + "\n---\nbody")); err == nil {
			t.Fatal("accepted unsupported agent semantics", extra)
		}
	}
	for _, value := range []string{strings.Repeat("large", (64<<10)/5+1), "\xff", "---\npaths: src/**\n---\nconditional"} {
		put(t, root, "GEMINI.md", value)
		if _, err := Open(t.Context(), root, Config{Version: 1, Rules: []string{"gemini"}}); err == nil {
			t.Fatal("unbounded/invalid/conditional rule accepted")
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := Open(ctx, root, Config{Version: 1, Rules: []string{"agents"}}); err == nil {
		t.Fatal("canceled source scan")
	}
}

func TestCompatibilityMergesOneNativeCatalogAndRejectsCollisions(t *testing.T) {
	ctx := t.Context()
	root := t.TempDir()
	put(t, root, "native/first/SKILL.md", "---\nname: first\ndescription: native\n---\nfirst")
	put(t, root, "standard/second/SKILL.md", "---\nname: second\ndescription: standard\nallowed-tools: Read\n---\nsecond")
	base, err := skill.OpenWithin(root, "native", false)
	if err != nil {
		t.Fatal(err)
	}
	defer base.Close()
	s, err := Open(ctx, root, Config{Version: 1, Skills: []SkillSource{{"standard", "agent-skills"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close(ctx)
	refs, _ := agentcontext.NewMemoryReferences("native", 1<<20)
	plugin := s.Plugin("native", refs, base)
	if _, err := core.NewBuilder(&captureModel{}, core.NewMemorySession("native")).Plugin(plugin).Build(ctx); err != nil {
		t.Fatal(err)
	}
	if len(plugin.Tools()) != 1 {
		t.Fatal("duplicated Skill tool")
	}
	put(t, root, "native/second/SKILL.md", "---\nname: second\ndescription: collision\n---\nbody")
	if err := base.Refresh(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := core.NewBuilder(&captureModel{}, core.NewMemorySession("native")).Plugin(s.Plugin("native", refs, base)).Build(ctx); err == nil {
		t.Fatal("silently resolved ambiguous native/compatibility name")
	}
}
