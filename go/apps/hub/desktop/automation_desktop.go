// INPUT: Ordinary-session schedule DTOs, optimistic revisions and shared product AutomationCoordinator
// OUTPUT: Native paused-first CRUD and separate pinned account/billing approval controls
// POS: [L7 Hub product] UI settings do not tick, replay missed tasks, run Workers or implement a second scheduler
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strconv"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/egoist/mygo/ui"
)

type automationPane struct {
	Entries                                   []hub.Automation
	Title, Prompt, Interval, EditID, DeleteID string
	Revision                                  uint64
}

func (p *NativePage) openAutomations() {
	if p.selected == "" || p.paneBusy || p.Service.Automations == nil {
		return
	}
	p.setPane("automations")
	if p.automation.Interval == "" {
		p.automation.Interval = "60"
	}
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		entries, err := p.Service.Automations.Store.ListAutomations(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "automations" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.automation.Entries = entries
		})
	}()
}
func (p *NativePage) automationAction(action string, entry hub.Automation) {
	if p.selected == "" || p.paneBusy || p.Service.Automations == nil {
		return
	}
	minutes, err := strconv.ParseInt(p.automation.Interval, 10, 64)
	draft := hub.AutomationDraft{Title: p.automation.Title, Prompt: p.automation.Prompt, IntervalMinutes: minutes}
	if action == "save" && (err != nil || draft.Validate() != nil) {
		p.problem = "Provide a title, 1..16KiB prompt and interval 5..10080 minutes."
		return
	}
	id, generation, editID, revision := p.selected, p.generation, p.automation.EditID, p.automation.Revision
	p.paneBusy = true
	go func() {
		var err error
		switch action {
		case "save":
			if editID == "" {
				_, err = p.Service.Automations.Store.CreateAutomation(p.ctx, id, draft)
			} else {
				_, err = p.Service.Automations.Store.UpdateAutomation(p.ctx, id, editID, revision, draft)
			}
		case "enable":
			_, err = p.Service.SetAutomationEnabled(p.ctx, id, entry.ID, entry.Revision, true, true)
		case "pause":
			_, err = p.Service.SetAutomationEnabled(p.ctx, id, entry.ID, entry.Revision, false, false)
		case "delete":
			err = p.Service.Automations.Store.DeleteAutomation(p.ctx, id, entry.ID, entry.Revision)
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
			p.automation.DeleteID = ""
			if action == "save" {
				p.automation.EditID = ""
				p.automation.Revision = 0
				p.automation.Title = ""
				p.automation.Prompt = ""
			}
			if p.pane == "automations" {
				p.openAutomations()
			}
		})
	}()
}
func (p *NativePage) buildAutomations(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Local ordinary-session automations").Bold()
		ui.Text(c, "New/edited schedules are paused. Enable separately confirms the exact saved engine/model/account/billing route. Runs use normal durable Turns, never automatic fallback or catch-up. Busy/missed intervals skip; Stop/failure pauses. Closing this app stops its timer; no OS daemon is installed. Team Leads/Workers are not eligible.").FontSize(11)
		if p.Service.Automations.HasError() {
			ui.Text(c, "Scheduler observed an admission error. Review saved status/history; nothing is automatically retried.").FontSize(12)
		}
		ui.Button(c, "Refresh saved schedules").Disabled(p.paneBusy).OnClick(p.openAutomations)
		ui.TextInput(c.Key("automation-title"), &p.automation.Title).Label("Automation title").Disabled(p.paneBusy)
		ui.TextArea(c.Key("automation-prompt"), &p.automation.Prompt).Label("Automation prompt").Height(130).Disabled(p.paneBusy)
		ui.TextInput(c.Key("automation-interval"), &p.automation.Interval).Label("Automation interval minutes").Disabled(p.paneBusy)
		ui.Button(c, "Save schedule paused").Disabled(p.paneBusy).OnClick(func() { p.automationAction("save", hub.Automation{}) })
		if p.automation.EditID != "" {
			ui.Text(c, "Editing revision-pinned schedule "+p.automation.EditID).FontSize(11)
			ui.Button(c, "Cancel schedule edit").OnClick(func() {
				p.automation.EditID = ""
				p.automation.Revision = 0
				p.automation.Title = ""
				p.automation.Prompt = ""
			})
		}
		if len(p.automation.Entries) == 0 && !p.paneBusy {
			ui.Text(c, "No saved schedules. Opening this page never creates or runs work.")
		}
		for _, entry := range p.automation.Entries {
			entry := entry
			ui.Text(c, fmt.Sprintf("%s · every %d min · enabled=%v · %s · next=%d · revision=%d", entry.Title, entry.IntervalMinutes, entry.Enabled, entry.LastStatus, entry.NextRunAt, entry.Revision)).Bold().FontSize(12)
			ui.Text(c, fmt.Sprintf("Pinned: %s / %s / %s · account=%s · billing=%s · branch=%s", entry.Target.EngineID, entry.Target.ProviderID, entry.Target.ModelID, entry.Target.AccountSource, entry.Target.BillingRoute, entry.BranchID)).FontSize(11)
			if entry.LastError != "" {
				ui.Text(c, "Schedule status: "+entry.LastError).FontSize(11)
			}
			ui.Row(c).Gap(6).Children(func() {
				ui.Button(c.Key("edit-"+entry.ID), "Edit saved schedule").Disabled(p.paneBusy || entry.RunningTurnID != "").OnClick(func() {
					p.automation.EditID = entry.ID
					p.automation.Revision = entry.Revision
					p.automation.Title = entry.Title
					p.automation.Prompt = entry.Prompt
					p.automation.Interval = strconv.FormatInt(entry.IntervalMinutes, 10)
				})
				if entry.Enabled {
					ui.Button(c.Key("pause-"+entry.ID), "Pause future intervals (not current Turn)").Disabled(p.paneBusy).OnClick(func() { p.automationAction("pause", entry) })
				} else {
					ui.Button(c.Key("enable-"+entry.ID), "Confirm pinned account/billing and enable schedule").Disabled(p.paneBusy || entry.RunningTurnID != "").OnClick(func() { p.automationAction("enable", entry) })
				}
			})
			if p.automation.DeleteID == entry.ID {
				ui.Button(c.Key("confirm-delete-"+entry.ID), "Confirm delete config only; keep session/history").Disabled(p.paneBusy || entry.RunningTurnID != "").OnClick(func() { p.automationAction("delete", entry) })
			} else {
				ui.Button(c.Key("delete-"+entry.ID), "Review schedule config deletion").Disabled(p.paneBusy || entry.RunningTurnID != "").OnClick(func() { p.automation.DeleteID = entry.ID })
			}
		}
	})
}
