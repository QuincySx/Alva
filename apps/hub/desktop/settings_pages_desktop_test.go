// INPUT: MyGo headless input, the real private product store and harmless never-started fixtures
// OUTPUT: Settings form regressions: saving never approves, enabling asks and approves one workspace, edits revoke it
// POS: [L7 Hub UI] Headless control acceptance; no OS window, accessibility or real process claim
//go:build desktop

package hubdesktop

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func settingsPageFixture(t *testing.T, title string) (*product.HubService, *NativePage, *ui.Tester, chan struct{}) {
	t.Helper()
	service, requests := fixture(t)
	session, err := service.Create(t.Context(), title, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.selected = session.ID
	p.detail, err = service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1500, 2200)
	if err = workspaceTestMenu(tester, "会话设置"); err != nil {
		t.Fatal(err)
	}
	awaitSettingsForm(t, tester, p)
	done := make(chan struct{})
	t.Cleanup(func() {
		close(done)
		if service.Runtime.HasActive() || len(requests) != 0 {
			t.Error("settings observation, save or approval started execution")
		}
	})
	return service, p, tester, done
}

func settingsClick(t *testing.T, tester *ui.Tester, p *NativePage, label string) {
	t.Helper()
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
	awaitSettingsForm(t, tester, p)
}

func settingsType(t *testing.T, tester *ui.Tester, label, text string) {
	t.Helper()
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(text)
	tester.Frame()
}

func effective(t *testing.T, service *product.HubService, p *NativePage) hub.Settings {
	t.Helper()
	value, err := service.EffectiveSettings(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	return value
}

// enableWithConfirmation toggles a host switch, which must ask before acting.
func enableWithConfirmation(t *testing.T, tester *ui.Tester, p *NativePage, kind, item string) {
	t.Helper()
	toggle := "Toggle " + kind
	if item != "" {
		toggle += " " + item
	}
	settingsClick(t, tester, p, toggle)
	if p.settingsForm.confirm == nil {
		t.Fatal("host switch enabled without asking")
	}
	if state, _ := p.settingsForm.view.Item(kind, item); state.Enabled {
		t.Fatal("switch changed before confirmation")
	}
	settingsClick(t, tester, p, "Confirm enable-"+kind+"-"+item)
	if p.settingsForm.problem != "" || len(p.settingsForm.errors) != 0 {
		t.Fatal("enable failed", p.settingsForm.problem, p.settingsForm.errors)
	}
}

func TestNativeBrowserSettingsNeverLaunchOnSaveOrApproval(t *testing.T) {
	service, p, tester, _ := settingsPageFixture(t, "browser preferences")
	settingsClick(t, tester, p, "Open 浏览器工具")
	browser := filepath.Join(t.TempDir(), "not-started-browser")
	settingsType(t, tester, "浏览器可执行文件输入", browser)
	settingsClick(t, tester, p, "Save browser")
	if value := effective(t, service, p); value.Browser.Executable != browser || value.Browser.Enabled || value.Browser.HostAccessApproved {
		t.Fatal("browser save self-approved", value.Browser, p.settingsForm.problem, p.settingsForm.errors)
	}
	enableWithConfirmation(t, tester, p, hub.CapabilityBrowser, "")
	if value := effective(t, service, p); !value.Browser.Enabled || !value.Browser.HostAccessApproved {
		t.Fatal("browser approval for this workspace not saved")
	}
	settingsType(t, tester, "浏览器可执行文件输入", browser+"-changed")
	settingsClick(t, tester, p, "Save browser")
	if value := effective(t, service, p); value.Browser.Enabled || value.Browser.HostAccessApproved {
		t.Fatal("browser edit retained its approval")
	}
	if state, _ := p.settingsForm.view.Item(hub.CapabilityBrowser, ""); !state.Enabled || !state.NeedsApproval || !tester.HasText("尚未批准") {
		t.Fatal("changed configuration does not ask for workspace approval", state)
	}
}

func TestNativeCommandsAndMemorySettingsDoNotRunOnSaveOrApprove(t *testing.T) {
	service, p, tester, _ := settingsPageFixture(t, "capability preferences")
	settingsClick(t, tester, p, "Toggle memory")
	if p.settingsForm.confirm != nil || !effective(t, service, p).PersistentMemory {
		t.Fatal("memory opt-in not saved")
	}
	if _, err := os.Stat(filepath.Join(service.DataDirectory, "memory")); !os.IsNotExist(err) {
		t.Fatal("preference created memory authority before execution", err)
	}
	settingsClick(t, tester, p, "Open Hooks 与诊断")
	settingsClick(t, tester, p, "Add hook")
	settingsType(t, tester, "标识输入", "explicit")
	settingsType(t, tester, "命令输入", filepath.Join(t.TempDir(), "not-started-fixture"))
	settingsClick(t, tester, p, "Save command")
	if value := effective(t, service, p); len(value.Commands.Hooks) != 1 || value.Commands.Hooks[0].Enabled || value.Commands.Hooks[0].HostAccessApproved {
		t.Fatal("command form imported approval", value.Commands, p.settingsForm.errors)
	}
	enableWithConfirmation(t, tester, p, hub.CapabilityHook, "explicit")
	if value := effective(t, service, p); !value.Commands.Hooks[0].Enabled || !value.Commands.Hooks[0].HostAccessApproved {
		t.Fatal("command consent not saved")
	}
}

func TestNativeCommandFormReportsChineseFieldErrors(t *testing.T) {
	service, p, tester, _ := settingsPageFixture(t, "command errors")
	settingsClick(t, tester, p, "Open Hooks 与诊断")
	settingsClick(t, tester, p, "Add hook")
	settingsType(t, tester, "标识输入", "bad id")
	settingsType(t, tester, "命令输入", "relative/tool")
	settingsClick(t, tester, p, "Save command")
	if !tester.HasText("请先修正标红的字段") || !tester.HasText("命令须是绝对路径") || !tester.HasText("标识须唯一") {
		t.Fatal("field errors were not shown", p.settingsForm.errors)
	}
	if value := effective(t, service, p); len(value.Commands.Hooks) != 0 {
		t.Fatal("invalid command was saved")
	}
	if p.settingsForm.editor == nil {
		t.Fatal("invalid save discarded the draft")
	}
}

func TestNativeAEPInspectAndApprovalAreExplicitWithoutExecution(t *testing.T) {
	service, p, tester, _ := settingsPageFixture(t, "Native Plugin settings")
	source := t.TempDir()
	if err := os.WriteFile(filepath.Join(source, "alva.toml"), []byte("name='native-ui-fixture'\nversion='1'\nruntime='python'\nentry='plugin.py'\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(source, "plugin.py"), []byte("# Never executed during inspection\n"), 0600); err != nil {
		t.Fatal(err)
	}
	settingsClick(t, tester, p, "Open 扩展插件")
	settingsClick(t, tester, p, "Add AEP plugin")
	settingsType(t, tester, "源目录输入", source)
	settingsType(t, tester, "解释器（JS/Python）输入", filepath.Join(source, "explicit-not-started-interpreter"))
	settingsClick(t, tester, p, "Inspect and save AEP")
	value := effective(t, service, p)
	if len(value.AEP) != 1 || value.AEP[0].Enabled || len(value.AEP[0].SHA256) != 64 || value.AEP[0].Name != "native-ui-fixture" {
		t.Fatal("inspection bypassed the disabled pin", value.AEP, p.settingsForm.problem, p.settingsForm.errors)
	}
	enableWithConfirmation(t, tester, p, hub.CapabilityAEP, value.AEP[0].ID)
	if value = effective(t, service, p); !value.AEP[0].Enabled || !value.AEP[0].HostAccessApproved {
		t.Fatal("explicit approval not durable")
	}
}

func TestNativeMediaSettingsSaveDisabledAndRequireIndependentDecoderConsent(t *testing.T) {
	service, p, tester, _ := settingsPageFixture(t, "media preferences")
	settingsClick(t, tester, p, "Open 媒体工具")
	if effective(t, service, p).MediaEnabled {
		t.Fatal("media enabled by opening preferences")
	}
	settingsClick(t, tester, p, "Toggle media")
	settingsType(t, tester, "ffmpeg 路径输入", filepath.Join(t.TempDir(), "not-started-ffmpeg"))
	settingsType(t, tester, "ffprobe 路径输入", filepath.Join(t.TempDir(), "not-started-ffprobe"))
	settingsClick(t, tester, p, "Save media")
	if value := effective(t, service, p); !value.MediaEnabled || value.MediaDecoder.FFmpeg == "" || value.MediaDecoder.Enabled || value.MediaDecoder.HostAccessApproved {
		t.Fatal("decoder save self-approved", value.MediaDecoder, p.settingsForm.errors)
	}
	enableWithConfirmation(t, tester, p, hub.CapabilityDecoder, "")
	if value := effective(t, service, p); !value.MediaDecoder.Enabled || !value.MediaDecoder.HostAccessApproved {
		t.Fatal("separate host consent not saved")
	}
	settingsType(t, tester, "ffmpeg 路径输入", filepath.Join(t.TempDir(), "changed-ffmpeg"))
	settingsClick(t, tester, p, "Save media")
	if value := effective(t, service, p); value.MediaDecoder.Enabled || value.MediaDecoder.HostAccessApproved {
		t.Fatal("decoder edits retained prior approval")
	}
}

func TestNativeSettingsResetAsksAndRestoresDefaults(t *testing.T) {
	service, p, tester, _ := settingsPageFixture(t, "reset")
	settingsClick(t, tester, p, "Open 浏览器工具")
	settingsType(t, tester, "浏览器可执行文件输入", "/Applications/Fixture")
	settingsClick(t, tester, p, "Save browser")
	settingsClick(t, tester, p, "Reset browser to defaults")
	if p.settingsForm.confirm == nil || effective(t, service, p).Browser.Executable == "" {
		t.Fatal("reset acted without confirmation")
	}
	settingsClick(t, tester, p, "Confirm reset-browser")
	if effective(t, service, p).Browser.Executable != "" || *p.settingsForm.editor.str("executable") != "" {
		t.Fatal("reset did not restore defaults")
	}
}
