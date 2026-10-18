// INPUT: Headless MyGo JSON editor and actual private product browser settings
// OUTPUT: Disabled-save, independent host approval and edit-revocation UI regressions
// POS: [L7 Hub product] Real native controls do not start or discover browser/model work on configuration
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestNativeBrowserSettingsNeverLaunchOnSaveOrApproval(t *testing.T) {
	service, requests := fixture(t)
	s, err := service.Create(context.Background(), "browser preferences", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.selected = s.ID
	p.detail, err = service.Store.Detail(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1600, 1800)
	if err = tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if err = tester.Click("Browser tools (fresh private profiles)"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	raw, _ := json.Marshal(hub.BrowserSettings{Executable: filepath.Join(t.TempDir(), "not-started-browser"), Headless: true, Enabled: true, HostAccessApproved: true})
	if err = tester.Click("Explicit browser configuration JSON"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(string(raw))
	if err = tester.Click("Save browser snapshot disabled"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || p.settings.Browser.Enabled || p.settings.Browser.HostAccessApproved {
		t.Fatal("browser import self-approved", p.problem)
	}
	if err = tester.Click("Approve full host/network access for fresh browser next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || !p.settings.Browser.Enabled || !p.settings.Browser.HostAccessApproved {
		t.Fatal("browser independent approval not saved", p.problem)
	}
	if err = tester.Click("Save browser snapshot disabled"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || p.settings.Browser.Enabled || p.settings.Browser.HostAccessApproved {
		t.Fatal("browser edits retained approval", p.problem)
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("browser preferences started work")
	}
}
