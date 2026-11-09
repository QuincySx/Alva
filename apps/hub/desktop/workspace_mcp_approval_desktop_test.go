// INPUT: Real product service and settings, an application MCP server not yet approved in the task's workspace and the headless native window
// OUTPUT: The pending-approval line above the composer, its two-step approve action and its disappearance
// POS: [L7 Hub UI test] Proves the line is a view over SettingsService approvals and never approves silently
//go:build desktop

package hubdesktop

import (
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func saveMCPNoticeScreenshot(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_W1C_SCREENSHOT_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tester.Image()); err != nil {
		t.Fatal(err)
	}
}

func TestWorkspaceShowsPendingMCPApprovalAboveTheComposerAndApprovesAfterConfirmation(t *testing.T) {
	s, _ := fixture(t)
	first, err := s.Create(t.Context(), "approved workspace", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	second, err := s.Create(t.Context(), "new workspace", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	scope := product.SettingsScope{App: true}
	view, err := s.Settings.Load(t.Context(), scope)
	if err != nil {
		t.Fatal(err)
	}
	desired := view.Capabilities
	desired.MCP = []hub.MCPServer{{ID: "files", Name: "文件", Command: "/bin/echo"}, {ID: "docs", Name: "文档", Command: "/bin/echo"}}
	if view, err = s.Settings.Save(t.Context(), scope, product.SectionMCP, view.Fingerprints[product.SectionMCP], desired); err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{"files", "docs"} {
		if _, err = s.Settings.SetEnabled(t.Context(), scope, hub.CapabilityMCP, id, true); err != nil {
			t.Fatal(err)
		}
	}
	// The first workspace already approved the server; the new one has not.
	if _, err = s.Settings.Approve(t.Context(), first.ID, hub.CapabilityMCP, "files", true); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Settings.Approve(t.Context(), first.ID, hub.CapabilityMCP, "docs", true); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.ctx = t.Context()
	p.selected = second.ID
	if p.detail, err = s.Store.Detail(t.Context(), second.ID); err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	waitWorkspace(t, p, tester, func() bool { return p.mcpNotice.loaded && !p.mcpNotice.loading })
	const line = "2 个 MCP 服务器在此工作区待批准 · "
	if !tester.HasText(line) {
		t.Fatal("pending approval line missing", tester.Texts())
	}
	saveMCPNoticeScreenshot(t, tester, "mcp-pending-line.png")
	if got := effectiveMCPFor(t, s, second.ID); got[0].Enabled || got[0].HostAccessApproved {
		t.Fatal("showing the line approved something")
	}
	if err = tester.Click("Review workspace approvals"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("允许 2 个 MCP 服务器 在工作区 " + second.Workspace + " 中以你的权限运行当前配置？配置改变后需要重新批准，其他工作区仍要单独批准。") {
		t.Fatal("approving did not ask for confirmation", tester.Texts())
	}
	saveMCPNoticeScreenshot(t, tester, "mcp-pending-confirm.png")
	if got := effectiveMCPFor(t, s, second.ID); got[0].Enabled || got[0].HostAccessApproved {
		t.Fatal("approved before the confirmation")
	}
	if err = tester.Click("Cancel workspace approvals"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if _, open := tester.Find("Confirm workspace approvals"); open {
		t.Fatal("cancel left the confirmation open")
	}
	if err = tester.Click("Review workspace approvals"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err = tester.Click("Confirm workspace approvals"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.mcpNotice.busy && p.mcpNotice.loaded && len(p.mcpNotice.pending) == 0 })
	if tester.HasText(line) {
		t.Fatal("line stayed after the approval", tester.Texts())
	}
	for _, server := range effectiveMCPFor(t, s, second.ID) {
		if !server.Enabled || !server.HostAccessApproved {
			t.Fatalf("approval did not reach the workspace: %+v", server)
		}
	}
	// The first workspace never had a pending approval.
	p.selected = first.ID
	if p.detail, err = s.Store.Detail(t.Context(), first.ID); err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	waitWorkspace(t, p, tester, func() bool { return p.mcpNotice.session == first.ID && p.mcpNotice.loaded })
	if tester.HasText("待批准") {
		t.Fatal("an approved workspace shows the line", tester.Texts())
	}
}

func effectiveMCPFor(t *testing.T, s *product.HubService, id string) []hub.MCPServer {
	t.Helper()
	settings, err := s.EffectiveSettings(t.Context(), id)
	if err != nil {
		t.Fatal(err)
	}
	return settings.MCP
}

// A decided Alva approval stays in the conversation as one line shaped like an
// external engine's, named by the call's ToolDisplay and never by tool name.
func TestWorkspaceShowsSettledAlvaApprovalLines(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "settled alva approvals", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	if p.detail, err = s.Store.Detail(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	edit := &engine.ToolDisplay{Kind: engine.ToolEdit, Path: "a.go"}
	command := &engine.ToolDisplay{Kind: engine.ToolCommand, Command: "printf hi"}
	seq := uint64(0)
	var entries []hub.HistoryEntry
	add := func(entry hub.HistoryEntry) {
		seq++
		entry.Sequence, entry.SessionID, entry.TurnID, entry.AttemptID = seq, session.ID, "turn-1", "attempt-1"
		entries = append(entries, entry)
	}
	settled := func(call string, allow bool, effect engine.Effect, display *engine.ToolDisplay) hub.HistoryEntry {
		return hub.HistoryEntry{Timeline: &hub.TimelineEvent{Kind: hub.TimelineApprovalSettled, Text: "Approval settled by the user", Approval: &hub.ApprovalSettled{CallID: call, Allowed: allow, Effect: effect, Tool: display}}}
	}
	add(hub.HistoryEntry{Event: engine.Event{Kind: engine.ToolStart, ID: "c1", Name: "write_file", Display: edit}})
	add(settled("c1", true, engine.EffectWrite, edit))
	add(hub.HistoryEntry{Event: engine.Event{Kind: engine.ToolEnd, ID: "c1", Name: "write_file", Display: edit}})
	add(hub.HistoryEntry{Event: engine.Event{Kind: engine.ToolStart, ID: "c2", Name: "execute_shell", Display: command}})
	add(settled("c2", false, engine.EffectExecute, command))
	add(settled("c3", true, engine.EffectWrite, nil))
	add(hub.HistoryEntry{Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "Done."}})
	replaceTranscript(t, p, entries...)
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	for _, text := range []string{"已允许 · 修改 a.go", "已拒绝 · 运行命令 `printf hi`", "已允许 · 修改文件"} {
		if !tester.HasText(text) {
			t.Fatal("settled Alva approval line missing", text, tester.Texts())
		}
	}
	for _, name := range []string{"write_file", "execute_shell"} {
		for _, text := range tester.Texts() {
			if strings.Contains(text, "已允许 ·") && strings.Contains(text, name) || strings.Contains(text, "已拒绝 ·") && strings.Contains(text, name) {
				t.Fatal("a settled line shows the raw tool name", text)
			}
		}
	}
	saveMCPNoticeScreenshot(t, tester, "alva-approval-settled-lines.png")
}
