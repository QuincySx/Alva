// INPUT: MyGo headless user input, explicit harmless bundle and actual private product settings
// OUTPUT: Native AEP JSON inspection and separate next-turn consent without execution regressions
// POS: [L7 Hub product] Control behavior acceptance, not OS-window visual/accessibility acceptance
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestNativeAEPInspectAndApprovalAreExplicitWithoutExecution(t *testing.T) {
	service, requests := fixture(t)
	ctx := context.Background()
	session, err := service.Create(ctx, "Native Plugin settings", "")
	if err != nil {
		t.Fatal(err)
	}
	source := t.TempDir()
	if err = os.WriteFile(filepath.Join(source, "alva.toml"), []byte("name='native-ui-fixture'\nversion='1'\nruntime='python'\nentry='plugin.py'\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(source, "plugin.py"), []byte("# Never executed during inspection\n"), 0600); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1400, 1300)
	if err = tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if err = tester.Click("Native AEP Plugins (explicit inspected bundles)"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if !tester.HasText("Native AEP Plugins · explicit next-turn snapshots") {
		t.Fatal("native Plugin pane missing")
	}
	raw, _ := json.Marshal([]hub.AEPBundle{{ID: "ui-fixture", SourceDirectory: source, Interpreter: filepath.Join(source, "explicit-not-started-interpreter"), Enabled: true, HostAccessApproved: true}})
	if err = tester.Click("Native AEP configuration JSON"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(string(raw))
	if err = tester.Click("Inspect pins and save all AEP disabled"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || len(page.settings.AEP) != 1 || page.settings.AEP[0].Enabled || len(page.settings.AEP[0].SHA256) != 64 {
		t.Fatal("UI inspect bypassed disabled pin", page.problem)
	}
	if err = tester.Click("Approve inspected code and full host access next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || !page.settings.AEP[0].Enabled || !page.settings.AEP[0].HostAccessApproved {
		t.Fatal("explicit native approval not durable", page.problem)
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("settings observation/inspection/approval started execution")
	}
}
