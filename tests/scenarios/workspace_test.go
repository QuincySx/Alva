// INPUT: hubFixture real Alva assembly, fakeModel file-tool calls, temporary project/outside directories and symlinks
// OUTPUT: Scenarios for chat/project mode workspaces, workspace escape attempts and moving between workspaces
// POS: [scenario] The file tools touch only the session's workspace; sessions keep their own workspace across switches and reopen.
package scenarios

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

// promptedCalls answers "do <key>" prompts with the mapped call and any tool
// result with a short text, so each Turn is one decision independent of order.
func promptedCalls(calls map[string]toolCall) func(modelRequest) reply {
	return func(r modelRequest) reply {
		if r.answersTool() {
			return text("done")
		}
		for key, c := range calls {
			if r.typed("do " + key) {
				return reply{Calls: []toolCall{c}}
			}
		}
		return text("nothing to do")
	}
}

func TestToolsCannotEscapeTheWorkspace(t *testing.T) {
	root := t.TempDir()
	workspace := filepath.Join(root, "project")
	outside := filepath.Join(root, "outside")
	for _, dir := range []string{workspace, outside} {
		if err := os.Mkdir(dir, 0o700); err != nil {
			t.Fatal(err)
		}
	}
	secret := filepath.Join(outside, "secret.txt")
	if err := os.WriteFile(secret, []byte("OUTSIDE-SECRET-CONTENT"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(workspace, "link-dir")); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(secret, filepath.Join(workspace, "link-file")); err != nil {
		t.Fatal(err)
	}
	attempts := map[string]toolCall{
		"parent-write":   {ID: "c-parent-write", Name: "write_file", Args: `{"path":"../outside/parent.txt","content":"x"}`},
		"absolute-write": {ID: "c-absolute-write", Name: "write_file", Args: `{"path":"` + filepath.Join(outside, "absolute.txt") + `","content":"x"}`},
		"symlink-write":  {ID: "c-symlink-write", Name: "write_file", Args: `{"path":"link-dir/linked.txt","content":"x"}`},
		"symlink-edit":   {ID: "c-symlink-edit", Name: "edit_file", Args: `{"path":"link-file","old":"OUTSIDE","new":"CHANGED"}`},
		"parent-read":    {ID: "c-parent-read", Name: "read_file", Args: `{"path":"../outside/secret.txt"}`},
		"absolute-read":  {ID: "c-absolute-read", Name: "read_file", Args: `{"path":"` + secret + `"}`},
		"symlink-read":   {ID: "c-symlink-read", Name: "read_file", Args: `{"path":"link-file"}`},
		"parent-list":    {ID: "c-parent-list", Name: "list_directory", Args: `{"path":".."}`},
		"symlink-search": {ID: "c-symlink-search", Name: "search_files", Args: `{"path":"link-dir","query":"SECRET"}`},
	}
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = promptedCalls(attempts)
	h := openHub(t, liveConfig(t, model))
	s := h.create("escape", workspace)
	// Full widens approval only; the workspace boundary must still hold.
	if err := h.service().SetPermissionPolicy(context.Background(), s.ID, hub.PermissionFull, true); err != nil {
		t.Fatal(err)
	}
	for key, attempt := range attempts {
		h.send(s.ID, "do "+key)
		outcome := h.settle(s.ID)
		b := toolBlock(t, h.blocks(s.ID), attempt.ID)
		if b.Status != product.BlockStatusFailed {
			t.Errorf("%s: escape call shows %s (%s), turn %s", key, b.Status, b.Text, outcome)
		}
		t.Logf("%s: turn %s, tool result %q", key, outcome, b.Text)
	}
	entries, err := os.ReadDir(outside)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 {
		t.Fatalf("outside directory changed: %v", entries)
	}
	if data, _ := os.ReadFile(secret); string(data) != "OUTSIDE-SECRET-CONTENT" {
		t.Fatalf("outside file changed: %q", data)
	}
	for _, r := range model.all() {
		if r.has("OUTSIDE-SECRET-CONTENT") {
			t.Fatalf("outside content reached the model in request %d", r.N)
		}
	}
}

func TestChatAndProjectModeSessionsWriteIntoTheirOwnWorkspace(t *testing.T) {
	project := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = promptedCalls(map[string]toolCall{"write": {ID: "c-write", Name: "write_file", Args: `{"path":"made.txt","content":"here"}`}})
	h := openHub(t, liveConfig(t, model))
	chat := h.create("chat mode", "")
	proj := h.create("project mode", project)
	canonicalProject, _ := filepath.EvalSymlinks(project)
	canonicalData, _ := filepath.EvalSymlinks(h.config.DataDirectory)
	if proj.Workspace != canonicalProject {
		t.Fatalf("project workspace %q, want %q", proj.Workspace, canonicalProject)
	}
	if !strings.HasPrefix(chat.Workspace, canonicalData+string(filepath.Separator)) || chat.Workspace == proj.Workspace {
		t.Fatalf("chat workspace %q is not a private directory under %q", chat.Workspace, canonicalData)
	}
	for _, s := range []hub.Session{chat, proj} {
		if err := h.service().SetPermissionPolicy(context.Background(), s.ID, hub.PermissionAuto, false); err != nil {
			t.Fatal(err)
		}
		h.send(s.ID, "do write")
		h.expect(s.ID, engine.Succeeded)
		if data, err := os.ReadFile(filepath.Join(s.Workspace, "made.txt")); err != nil || string(data) != "here" {
			t.Fatalf("%s: file not in its own workspace: %q %v", s.Title, data, err)
		}
	}
	// A relative or missing directory is not a workspace.
	for _, bad := range []string{filepath.Join(project, "missing"), "."} {
		if s, err := h.service().Create(context.Background(), "bad", bad); err == nil {
			t.Errorf("workspace %q accepted as %q", bad, s.Workspace)
		}
	}
}

func TestSwitchingWorkspacesKeepsEarlierSessionsUsable(t *testing.T) {
	wsA, wsB := t.TempDir(), t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = promptedCalls(map[string]toolCall{
		"a1": {ID: "c-a1", Name: "write_file", Args: `{"path":"a1.txt","content":"A"}`},
		"b1": {ID: "c-b1", Name: "write_file", Args: `{"path":"b1.txt","content":"B"}`},
		"a2": {ID: "c-a2", Name: "write_file", Args: `{"path":"a2.txt","content":"A again"}`},
		"a3": {ID: "c-a3", Name: "write_file", Args: `{"path":"a3.txt","content":"after reopen"}`},
	})
	h := openHub(t, liveConfig(t, model))
	a := h.create("in A", wsA)
	b := h.create("in B", wsB)
	for _, s := range []hub.Session{a, b} {
		if err := h.service().SetPermissionPolicy(context.Background(), s.ID, hub.PermissionAuto, false); err != nil {
			t.Fatal(err)
		}
	}
	run := func(s hub.Session, key string) {
		t.Helper()
		h.send(s.ID, "do "+key)
		h.expect(s.ID, engine.Succeeded)
	}
	run(a, "a1")
	run(b, "b1")
	run(a, "a2")
	h.reopen()
	run(a, "a3")
	for path, want := range map[string]string{filepath.Join(wsA, "a1.txt"): "A", filepath.Join(wsB, "b1.txt"): "B", filepath.Join(wsA, "a2.txt"): "A again", filepath.Join(wsA, "a3.txt"): "after reopen"} {
		if data, err := os.ReadFile(path); err != nil || string(data) != want {
			t.Fatalf("%s = %q %v", path, data, err)
		}
	}
	for _, name := range []string{"a1.txt", "a2.txt", "a3.txt"} {
		if _, err := os.Stat(filepath.Join(wsB, name)); !os.IsNotExist(err) {
			t.Fatalf("%s leaked into workspace B", name)
		}
	}
	// The workspace's session list shows only its own conversations.
	canonicalA, _ := filepath.EvalSymlinks(wsA)
	page, err := h.service().BrowseWorkspaceSessions(context.Background(), canonicalA, "", hub.SessionCursor{}, 20)
	if err != nil {
		t.Fatal(err)
	}
	if len(page.Sessions) != 1 || page.Sessions[0].ID != a.ID {
		t.Fatalf("workspace A lists %+v", page.Sessions)
	}
}
