// INPUT: Explicit native/operator command JSON, product settings and independent full-host consent
// OUTPUT: Disabled hook/diagnostics edits and next-turn lifecycle/profile activation controls
// POS: [L7 Hub product] Native-only host command editor; no discovery, HTTP arbitrary-path import or automatic test execution
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"fmt"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) openCommands() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("commands")
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		value, err := p.Service.Store.Settings(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.settings = value
			commands := value.Commands
			if commands.Hooks == nil {
				commands.Hooks = []hub.HookSpec{}
			}
			if commands.Diagnostics == nil {
				commands.Diagnostics = []hub.DiagnosticSpec{}
			}
			raw, _ := json.MarshalIndent(commands, "", "  ")
			p.commandDraft = string(raw)
		})
	}()
}
func (p *NativePage) commandAction(kind, id string, enabled bool) {
	if p.selected == "" || p.paneBusy || p.model.ActiveRunID != "" {
		return
	}
	session, raw := p.selected, p.commandDraft
	p.paneBusy = true
	go func() {
		var err error
		if kind == "" {
			err = p.Service.SaveCommandsDraft(p.ctx, session, raw)
		} else {
			err = p.Service.Store.SetCommandEnabled(p.ctx, session, kind, id, enabled, enabled)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != session {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.openCommands()
		})
	}()
}
func (p *NativePage) buildCommands(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Explicit native Hooks and diagnostics").Bold()
		ui.Text(c, "Saving never starts a process and clears all approvals in this command editor. Commands use the saved session workspace and only explicit argv/env (no ambient credentials). Absolute executable paths are required, but binaries are not content-hashed or sandboxed. HTTP clients cannot import host paths/commands.").FontSize(11)
		ui.Text(c, "JSON object: hooks [{id,event,matcher,command,args,env,timeout_ms}], diagnostics [{id,description,command,args,env,timeout_ms}]. Up to 32 Hooks/16 profiles; 0 timeout means Hook 10s/diagnostic 30s, maximum 60s/300s. Hook events: PreToolUse, PostToolUse, PostToolUseFailure, SessionStart, SessionEnd, Stop, UserPromptSubmit, Notification; matcher supports exact/edge * only.").FontSize(11)
		ui.TextArea(c.Key("command-json"), &p.commandDraft).Label("Native command configuration JSON").Height(230).Disabled(p.paneBusy || p.model.ActiveRunID != "")
		ui.Button(c, "Save all native commands disabled").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.commandAction("", "", false) })
		for _, entry := range p.settings.Commands.Hooks {
			entry := entry
			ui.Text(c, fmt.Sprintf("Hook %s · %s · matcher=%q · %s · enabled=%v", entry.ID, entry.Event, entry.Matcher, entry.Command, entry.Enabled)).FontSize(12)
			if entry.Enabled {
				ui.Button(c.Key("hook-"+entry.ID), "Disable Hook next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.commandAction("hook", entry.ID, false) })
			} else {
				ui.Button(c.Key("hook-"+entry.ID), "Approve full host access and configured lifecycle Hook").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.commandAction("hook", entry.ID, true) })
			}
		}
		for _, profile := range p.settings.Commands.Diagnostics {
			profile := profile
			ui.Text(c, fmt.Sprintf("Diagnostic %s · %s · %s · enabled=%v", profile.ID, profile.Description, profile.Command, profile.Enabled)).FontSize(12)
			if profile.Enabled {
				ui.Button(c.Key("diagnostic-"+profile.ID), "Disable diagnostic profile next turn").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.commandAction("diagnostic", profile.ID, false) })
			} else {
				ui.Button(c.Key("diagnostic-"+profile.ID), "Approve full host access for diagnostic profile").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.commandAction("diagnostic", profile.ID, true) })
			}
		}
		ui.Text(c, "Enabled Hooks execute automatically only at the explicitly selected events of the next Alva run; this host approval is consent for those commands. Hook allow never grants Tool rights, and modified pre-input gets fresh security/scheduling checks. Diagnostics run only via an explicit project_diagnostics call with tool HITL. Every native start commits Activity risk and native intent before launch; bounded full output is retained as references, not automatically loaded. Exit0 is not parsed diagnostics or proof of zero problems. No implicit SDK-child delegation.").FontSize(11)
	})
}
