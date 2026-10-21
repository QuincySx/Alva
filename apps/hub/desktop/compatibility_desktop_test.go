// INPUT: Actual MyGo native controls, saved workspace and a project compatibility config.
// OUTPUT: Explicit next-turn enable/disable remains cold even without a managed Skill directory.
// POS: [L7 product UI] Headless control acceptance; no claim of real OS window or pixel verification.
//go:build desktop

package hubdesktop

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
)

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
	tester := ui.NewTester(page.Build, 1400, 1400)
	if err := tester.Click("Project skills"); err != nil {
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
