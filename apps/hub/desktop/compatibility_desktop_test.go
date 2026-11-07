// INPUT: Actual MyGo native controls, saved workspace and a project compatibility config.
// OUTPUT: Optional compatibility stays cold and late managed-Skill reads preserve the later view's draft.
// POS: [L7 product UI] Headless control acceptance; no claim of real OS window or pixel verification.
//go:build desktop

package hubdesktop

import (
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestNativeSkillReadKeepsLaterDraftAfterNavigation(t *testing.T) {
	for _, scope := range []string{"different-pane", "reopened-session"} {
		t.Run(scope, func(t *testing.T) {
			p, _, tester := settingsLifecyclePage(t)
			if err := p.Service.InitializeSkills(t.Context(), p.selected); err != nil {
				t.Fatal(err)
			}
			const original = "---\nname: lifecycle\ndescription: Explicit saved skill\n---\nOriginal saved body.\n"
			if err := p.Service.InstallSkill(t.Context(), p.selected, original); err != nil {
				t.Fatal(err)
			}
			store := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
			p.Service.Store = store
			store.block.Store(true)
			var release sync.Once
			defer release.Do(func() { close(store.release) })
			p.setPane("skills")
			p.loadSkill("lifecycle")
			select {
			case <-store.entered:
			case <-time.After(time.Second):
				t.Fatal("skill read did not reach its saved workspace")
			}
			if scope == "different-pane" {
				p.setPane("routing")
			} else {
				p.generation++
			}
			p.skillDraft, p.problem = "later unsaved skill draft", "current view message"
			release.Do(func() { close(store.release) })
			settlePage(t, tester, p)
			if p.skillDraft != "later unsaved skill draft" || p.problem != "current view message" || p.Service.Runtime.HasActive() {
				t.Fatal("late skill read replaced the current draft or started execution", p.skillDraft, p.problem)
			}
		})
	}
}

func TestNativeCompatibilityControlIsOptionalAndDoesNotExecute(t *testing.T) {
	service, requests := fixture(t)
	s, err := service.Create(t.Context(), "compatibility UI", "")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(s.Workspace, "compat.json"), []byte(`{"version":1,"rules":["agents"]}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(s.Workspace, "AGENTS.md"), []byte("UI-RULE"), 0600); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = s.ID
	page.detail, err = service.Store.Detail(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(workspaceStaticView(page), 1400, 1400)
	if err := workspaceTestMenu(tester, "项目技能"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.settings.CompatibilityFile != "" {
		t.Fatal("uninitialized native Skills blocked compatibility", page.problem)
	}
	if err := tester.Click("Compatibility configuration file"); err != nil {
		t.Fatal(err)
	}
	tester.Type("compat.json")
	if err := tester.Click("Apply compatibility next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.settings.CompatibilityFile != "compat.json" {
		t.Fatal(page.problem, page.settings.CompatibilityFile)
	}
	if err := tester.Click("Compatibility configuration file"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Key(0, ui.KeyBackspace)
	if err := tester.Click("Apply compatibility next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.settings.CompatibilityFile != "" {
		t.Fatal(page.problem, page.settings.CompatibilityFile)
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("UI configuration executed work")
	}
}
