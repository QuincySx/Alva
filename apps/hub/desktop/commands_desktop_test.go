// INPUT: MyGo headless JSON input and real product capability settings with no runtime registered
// OUTPUT: Explicit command approval and persistent-memory opt-in controls never start work
// POS: [L7 Hub product] Native control behavior, not an OS window/pixel acceptance claim
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

func TestNativeCommandsAndMemorySettingsDoNotRunOnSaveOrApprove(t *testing.T) {
	service, requests := fixture(t)
	ctx := context.Background()
	session, err := service.Create(ctx, "capability preferences", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1400, 1400)
	if err = tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if err = tester.Click("Enable explicit persistent workspace memory next turn"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if !page.settings.PersistentMemory {
		t.Fatal("memory opt-in not saved")
	}
	if _, err = os.Stat(filepath.Join(service.DataDirectory, "memory")); !os.IsNotExist(err) {
		t.Fatal("preference created memory authority before execution", err)
	}
	if err = tester.Click("Hooks and diagnostics (explicit commands)"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	commands := hub.CommandSettings{Hooks: []hub.HookSpec{{ID: "explicit", Event: "SessionStart", NativeCommand: hub.NativeCommand{Command: filepath.Join(t.TempDir(), "not-started-fixture"), Enabled: true, HostAccessApproved: true}}}}
	raw, _ := json.Marshal(commands)
	if err = tester.Click("Native command configuration JSON"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(string(raw))
	if err = tester.Click("Save all native commands disabled"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || len(page.settings.Commands.Hooks) != 1 || page.settings.Commands.Hooks[0].Enabled {
		t.Fatal("command UI imported approval", page.problem)
	}
	if err = tester.Click("Approve full host access and configured lifecycle Hook"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || !page.settings.Commands.Hooks[0].Enabled || !page.settings.Commands.Hooks[0].HostAccessApproved {
		t.Fatal("command consent not saved", page.problem)
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("preference/command save or approval started execution")
	}
}
