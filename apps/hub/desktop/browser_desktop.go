// INPUT: Native/operator browser JSON and independent product-scoped host/network approval
// OUTPUT: Explicit disabled-save and next-turn browser capability settings controls
// POS: [L7 Hub product] Self-drawn product settings do not discover, start or attach to user browser profiles
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"fmt"

	"github.com/egoist/mygo/ui"
)

func (p *NativePage) openBrowserSettings() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("browser-settings")
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		value, err := p.Service.Store.Settings(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "browser-settings" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.settings = value
			raw, _ := json.MarshalIndent(value.Browser, "", "  ")
			p.browserDraft = string(raw)
		})
	}()
}
func (p *NativePage) browserSetting(save, enabled bool) {
	if p.selected == "" || p.paneBusy || p.inputBlocked() {
		return
	}
	id, raw, generation := p.selected, p.browserDraft, p.generation
	p.paneBusy = true
	go func() {
		var err error
		if save {
			err = p.Service.SaveBrowserDraft(p.ctx, id, raw)
		} else {
			err = p.Service.Store.SetBrowserEnabled(p.ctx, id, enabled, enabled)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			if p.pane == "browser-settings" {
				p.openBrowserSettings()
			}
		})
	}()
}
func (p *NativePage) buildBrowserSettings(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Browser tools · fresh private run-owned profiles").Bold()
		ui.Text(c, "Default off. Explicit executable only: no PATH discovery/install, existing browser attachment, profile import or inherited login/keychain state. Saving does not start a process and clears imported approvals. Independent approval grants full host/network access for this saved workspace; this is not an OS sandbox.").FontSize(11)
		ui.Text(c, "JSON object fields: executable (absolute), env, headless, proxy (no credentials), startup_timeout_ms, operation_timeout_ms. Timeouts <=60000ms; 0 uses defaults. No arbitrary argv/user-data-dir/HOME. Executable is not hashed: same-path binary upgrades are not detected.").FontSize(11)
		ui.TextArea(c.Key("browser-json"), &p.browserDraft).Label("Explicit browser configuration JSON").Height(220).Disabled(p.paneBusy || p.inputBlocked())
		ui.Button(c, "Save browser snapshot disabled").Disabled(p.paneBusy || p.inputBlocked()).OnClick(func() { p.browserSetting(true, false) })
		ui.Text(c, fmt.Sprintf("Saved: enabled=%v · host approved=%v · headless=%v · executable=%s", p.settings.Browser.Enabled, p.settings.Browser.HostAccessApproved, p.settings.Browser.Headless, p.settings.Browser.Executable)).FontSize(12)
		if p.settings.Browser.Enabled {
			ui.Button(c, "Disable browser tools next turn").Disabled(p.paneBusy || p.inputBlocked()).OnClick(func() { p.browserSetting(false, false) })
		} else {
			ui.Button(c, "Approve full host/network access for fresh browser next turn").Disabled(p.paneBusy || p.inputBlocked() || p.settings.Browser.Executable == "").OnClick(func() { p.browserSetting(false, true) })
		}
		ui.Text(c, "Actual tools: start, stop, navigate, action, snapshot, screenshot and status. Navigation/click/type can have external effects and still use actual tool HITL; screenshots/text are same-session references, not automatic image/model input. A prior durable activity records native launch risk before the process starts.").FontSize(11)
		ui.Text(c, "Up to four independent instances per actual run; private cookies/state are not shared with your browser. All are closed/reaped and profiles removed at normal completion/cancel, not retained between turns. Inspector keeps actual cleanup evidence and full bounded source references. No implicit child delegation or route change.").FontSize(11)
	})
}
