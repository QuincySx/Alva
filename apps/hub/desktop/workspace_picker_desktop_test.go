// INPUT: Real product/Skill/file fixtures and MyGo headless editor, keyboard and clipboard events
// OUTPUT: Scoped picker query/selection, typed references, cancellation, IME and pagination regressions
// POS: [L7 Hub UI] No OS window, external process, model call or automatic Send
//go:build desktop

package hubdesktop

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func TestWorkspacePickerCommandsEnterOnlyPreparesTypedDraft(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("/")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading && len(p.picker.rows) == 2 })
	tester.Key(ui.Cmd, ui.KeyEnter)
	if !p.picker.open || p.busy || p.selected != "" || p.model.Draft != "/" {
		t.Fatal("send shortcut must not send or edit a draft while its command menu is open")
	}
	tester.Key(0, ui.KeyDown)
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerSelections[""]) == 1 })
	prepared := p.composerSelections[""][0]
	if prepared.EntryID != "builtin:review" || !strings.Contains(p.model.Draft, "审查") || p.model.Draft != prepared.Text || !tester.Focused("Message") {
		t.Fatal("Enter did not insert the selected typed draft and restore editor focus", prepared, p.model.Draft)
	}
	if p.selected != "" || p.busy || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("picker Enter sent or created a conversation")
	}
	p.openWorkspacePicker("commands")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading })
	before := p.model.Draft
	tester.Key(0, ui.KeyEscape)
	if p.picker.open || p.model.Draft != before || len(p.composerSelections[""]) != 1 || !tester.Focused("Message") {
		t.Fatal("Escape lost existing draft or selections")
	}
	p.openWorkspacePicker("commands")
	waitWorkspace(t, p, tester, func() bool { return !p.picker.loading })
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerSelections[""]) == 2 })
	if p.composerSelections[""][1].Intent != product.ComposerIntentPlan {
		t.Fatal("plan selection lost its real typed intent")
	}
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 {
		t.Fatal("plan selection persisted a session or policy", sessions, err)
	}
}

func TestWorkspaceComposerSendShortcutSharesAdmissionAndKeepsDraft(t *testing.T) {
	service, _ := fixture(t)
	session, err := service.Create(t.Context(), "shortcut admission", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(session.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
	wrapped := &workspaceDelayedDetail{ServiceStore: service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	service.Store = wrapped
	wrapped.block.Store(true)
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("快捷键发送的第一版草稿")
	tester.Key(ui.Cmd, ui.KeyEnter)
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("platform Send shortcut did not enter the existing admission path")
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("，继续编辑")
	newDraft := p.model.Draft
	tester.Key(ui.Cmd, ui.KeyEnter)
	if p.model.Draft != newDraft {
		t.Fatal("repeated shortcut while busy edited or resent the newer draft")
	}
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && !p.busy && p.pendingFirstSend == "" })
	if p.model.Draft != newDraft {
		t.Fatal("shortcut receipt erased a newer draft", p.model.Draft, newDraft)
	}
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 {
		t.Fatal("shortcut created duplicate sessions", rows, err)
	}
}

func TestWorkspaceSendShortcutCannotBypassBlockedComposerStates(t *testing.T) {
	for _, state := range []string{"busy", "running", "waiting", "recording", "composing"} {
		t.Run(state, func(t *testing.T) {
			service, _ := fixture(t)
			p := NewNativePage(service)
			p.model.Draft = "保留这份草稿"
			switch state {
			case "busy":
				p.busy = true
			case "running":
				p.model.ActiveRunID = "observed-run"
			case "waiting":
				p.selected, p.detail.Session.ID, p.detail.Session.Status = "observed-session", "observed-session", engine.Waiting
			case "recording":
				p.voice.recorder = newWorkspaceFakeRecorder()
			}
			tester := composerTester(p, 760)
			if err := tester.Click("Message"); err != nil {
				t.Fatal(err)
			}
			if state == "composing" {
				tester.Compose("输入法", 3)
			}
			tester.Key(ui.Cmd, ui.KeyEnter)
			rows, err := service.Store.ListSessions(t.Context(), 8)
			if err != nil || len(rows) != 0 || service.Runtime.HasActive() {
				t.Fatal("shortcut bypassed composer readiness", state, rows, err)
			}
		})
	}
}

func TestWorkspacePickerDoesNotTriggerForPasteIMEURLsOrExistingText(t *testing.T) {
	service, _ := fixture(t)
	for _, text := range []string{"https://example.test/path", "person@example.test", "/tmp/project/file", "code := a / b", "/existing-command"} {
		p := NewNativePage(service)
		p.model.Draft = text
		tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
		if p.picker.open {
			t.Fatalf("existing text triggered picker: %q", text)
		}
		if err := tester.Click("Message"); err != nil {
			t.Fatal(err)
		}
		tester.Command("selectAll")
		tester.Type(text)
		if p.picker.open {
			t.Fatalf("multi-rune insert triggered picker: %q", text)
		}
	}
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.SetClipboard("/")
	tester.Command("paste")
	if p.picker.open || p.model.Draft != "/" {
		t.Fatal("pasted trigger opened a picker or changed text", p.model.Draft)
	}
	tester.Command("selectAll")
	tester.Type("")
	tester.Key(0, ui.KeyBackspace)
	tester.Compose("/", 1)
	tester.Type("/")
	tester.Compose("", 0)
	if p.picker.open {
		t.Fatal("IME composition opened a picker")
	}
	p.model.Draft = ""
	tester.Frame()
	tester.Type("/")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading })
	tester.Key(0, ui.KeyLeft)
	if p.picker.open || p.model.Draft != "/" {
		t.Fatal("moving outside the trigger failed to close without changing the draft")
	}
	for _, blocked := range []string{"busy", "loading"} {
		p.model.Draft = ""
		if blocked == "busy" {
			p.busy = true
		} else {
			p.selected = "not-yet-loaded"
		}
		tester.Frame()
		p.openWorkspacePicker("commands")
		if p.picker.open {
			t.Fatalf("%s composer opened a selectable picker", blocked)
		}
		p.busy, p.selected = false, ""
	}
}

func TestWorkspacePickerSkillsShowDisabledDetailsAndKeepOriginal(t *testing.T) {
	service, requests := fixture(t)
	project, err := service.Create(t.Context(), "picker skills", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = service.InitializeSkills(t.Context(), project.ID); err != nil {
		t.Fatal(err)
	}
	raw := "---\nname: exact-review\ndescription: Check fixtures carefully\ninvocation: explicit\n---\nOriginal instructions; never run a script just by selecting.\n"
	if err = service.InstallSkill(t.Context(), project.ID, raw); err != nil {
		t.Fatal(err)
	}
	for index := range 20 {
		extra := fmt.Sprintf("---\nname: fixture-%02d\ndescription: bounded extra Skill\n---\nFixture instructions.\n", index)
		if err = service.InstallSkill(t.Context(), project.ID, extra); err != nil {
			t.Fatal(err)
		}
	}
	p := NewNativePage(service)
	p.selected, p.detail = project.ID, mustPickerDetail(t, service, project.ID)
	p.model.Draft = "保留草稿"
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openWorkspacePicker("skills")
	waitWorkspace(t, p, tester, func() bool { return !p.picker.loading && len(p.picker.rows) == 20 })
	if p.picker.rows[0].available() || p.picker.active != -1 {
		t.Fatal("disabled Skill became the active executable choice")
	}
	if err = tester.Click("Select /exact-review"); err != nil {
		t.Fatal(err)
	}
	if len(p.composerSelections[project.ID]) != 0 || p.model.Draft != "保留草稿" {
		t.Fatal("disabled selection altered draft")
	}
	if err = service.Store.SetSkillsEnabled(t.Context(), project.ID, true); err != nil {
		t.Fatal(err)
	}
	p.openWorkspacePicker("skills")
	waitWorkspace(t, p, tester, func() bool { return !p.picker.loading && len(p.picker.rows) == 20 && p.picker.rows[0].available() })
	parameter, present := tester.Find("Composer request argument")
	panel, _ := tester.Find("Workspace picker")
	if !present || parameter.Y < panel.Y || parameter.Y+parameter.H > panel.Y+panel.H {
		t.Fatal("parameters are hidden behind a long result list", parameter, panel)
	}
	if err = tester.Click("Composer item details"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.picker.detailLoading && p.picker.detail != nil })
	if p.picker.detail.Document == nil || p.picker.detail.Document.Body == "" || p.picker.detail.SHA256 == "" || len(p.composerSelections[project.ID]) != 0 {
		t.Fatal("detail viewing selected a Skill or omitted exact document evidence")
	}
	if err = tester.Click("Select /exact-review"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerSelections[project.ID]) == 1 })
	prepared := p.composerSelections[project.ID][0]
	if prepared.SkillRef == nil || prepared.SkillRef.Name != "exact-review" || !strings.HasPrefix(p.model.Draft, "保留草稿") {
		t.Fatal("Skill insertion lost typed reference or existing draft", prepared)
	}
	actual, err := service.SkillRaw(t.Context(), project.ID, "exact-review")
	if err != nil || actual != raw || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("selection rewrote original or executed", err)
	}
}

func TestWorkspacePickerReferencesRetainBytesAndBoundRealFileCandidates(t *testing.T) {
	service, requests := fixture(t)
	workspace := t.TempDir()
	for index := range 75 {
		if err := os.WriteFile(filepath.Join(workspace, fmt.Sprintf("report-%02d.txt", index)), []byte("exact original body"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	project, err := service.Create(t.Context(), "picker references", workspace)
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(project.ID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == project.ID })
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("保留 ")
	tester.Type("@")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading && len(p.picker.rows) == 50 })
	if !p.picker.truncated || p.picker.hasMore {
		t.Fatal("file candidate scan concealed its bounded result set")
	}
	list, _ := tester.Find("Composer picker results")
	firstLabel := "Select " + p.picker.rows[0].referenceName()
	beforeScroll, _ := tester.Find(firstLabel)
	tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, 240)
	afterScroll, _ := tester.Find(firstLabel)
	tester.Frame()
	settledScroll, _ := tester.Find(firstLabel)
	if afterScroll.H > 0 && afterScroll.Y >= beforeScroll.Y || settledScroll != afterScroll {
		t.Fatal("wheel did not scroll bounded candidates or cached active row pulled it back", beforeScroll, afterScroll, settledScroll)
	}
	p.picker.query = "report-74"
	p.queryWorkspacePicker(false)
	waitWorkspace(t, p, tester, func() bool { return !p.picker.loading && len(p.picker.rows) == 1 })
	selection := *p.picker.rows[0].file
	if err = tester.Click("Select @" + selection.DisplayPath); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerFiles[project.ID]) == 1 })
	if p.model.Draft != "保留 " || strings.Contains(p.model.Draft, "exact original body") {
		t.Fatal("file capture inserted a body or removed unrelated manual text", p.model.Draft)
	}
	captured := p.composerFiles[project.ID][0]
	record := p.composerFileRecords[captured.ID]
	if record.SourcePath != selection.Path || record.SHA256 == "" || captured.Scope.Kind != "session" || captured.Scope.ID != project.ID {
		t.Fatal("file selection lost real source and durable scope", record)
	}
	actual, err := os.ReadFile(selection.Path)
	if err != nil || string(actual) != "exact original body" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("capture changed original or executed an engine", err)
	}
}

func TestWorkspacePickerSupersededQueriesAndScopeCannotReplaceCurrentResults(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openWorkspacePicker("commands")
	p.openWorkspacePicker("skills")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading })
	if p.picker.kind != "skills" || len(p.picker.rows) != 0 {
		t.Fatal("cancelled commands query overwrote newer Skills query")
	}
	p.openWorkspacePicker("references")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading })
	if p.picker.problem != "" {
		t.Fatal("homepage local file picker incorrectly required an existing session", p.picker.problem)
	}
	if rows, err := service.Store.ListSessions(t.Context(), 8); err != nil || len(rows) != 0 {
		t.Fatal("cold home file browsing created a session", err)
	}
	p.openWorkspacePicker("commands")
	p.workspace = t.TempDir()
	tester.Frame()
	if p.picker.open {
		t.Fatal("project context change kept a stale picker open")
	}
	p.poll()
	tester.Frame()
	if p.picker.open {
		t.Fatal("late query reopened a picker from a different project")
	}
}

func mustPickerDetail(t *testing.T, service *product.HubService, id string) hub.Detail {
	t.Helper()
	detail, err := service.Store.Detail(t.Context(), id)
	if err != nil {
		t.Fatal(err)
	}
	return detail
}
