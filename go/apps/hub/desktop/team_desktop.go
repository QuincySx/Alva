// INPUT: Shared durable Team service, explicit user actions and MyGo native controls
// OUTPUT: Collaboration Workers, FIFO editing/paging, lifecycle controls and actual session focus
// POS: [L7 Hub product] Native Team surface does not invent SDK children, auto-dispatch on observation or duplicate runtime facts
//go:build desktop

package hubdesktop

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	view "github.com/QuincySx/Alva/go/apps/desktop"
	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) selectedWorker() (hub.Worker, bool) {
	for _, w := range p.team.Workers {
		if w.SessionID == p.selected {
			return w, true
		}
	}
	return hub.Worker{}, false
}
func (p *NativePage) openTeam() {
	if p.Service.Teams == nil || p.selected == "" || p.paneBusy {
		return
	}
	lead := p.selected
	if _, ok := p.selectedWorker(); ok {
		lead = p.team.Team.LeadSessionID
	}
	p.setPane("team")
	p.teamLead = lead
	p.teamWorker = ""
	p.taskPage = hub.WorkerTaskPage{}
	p.taskBack = nil
	p.taskCursor = 0
	p.editTaskID = ""
	p.loadTeam()
}
func (p *NativePage) loadTeam() {
	if p.paneBusy || p.Service.Teams == nil {
		return
	}
	lead := p.teamLead
	p.paneBusy = true
	go func() {
		d, err := p.Service.Teams.Store.TeamForLead(p.ctx, lead)
		p.dispatch(func() {
			p.paneBusy = false
			if p.teamLead != lead || p.pane != "team" {
				return
			}
			if errors.Is(err, sql.ErrNoRows) {
				p.team = hub.TeamDetail{}
				p.problem = ""
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.team = d
			p.problem = ""
		})
	}()
}
func (p *NativePage) loadWorkerTasks(worker string, after uint64) {
	if p.paneBusy || p.Service.Teams == nil {
		return
	}
	lead, team := p.teamLead, p.team.Team.ID
	if p.teamWorker != worker {
		p.worktreePatch = product.WorktreePatchPage{}
		p.worktreeOffset = 0
		p.worktreeConfirm = ""
		p.worktreePatchID = ""
	}
	p.paneBusy = true
	go func() {
		page, err := p.Service.Teams.Store.WorkerTasks(p.ctx, lead, team, worker, after, 8)
		p.dispatch(func() {
			p.paneBusy = false
			if p.teamLead != lead || p.team.Team.ID != team || p.pane != "team" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.teamWorker = worker
			p.taskPage = page
			p.taskCursor = after
		})
	}()
}
func (p *NativePage) teamAction(action, id string) {
	if p.paneBusy || p.Service.Teams == nil {
		return
	}
	lead, team, worker := p.teamLead, p.team.Team.ID, p.teamWorker
	name, draft, path, edit := p.workerName, p.workerDraft, p.workerPath, p.editTaskID
	if path == "" {
		path = "."
	}
	claims := []hub.ResourceClaim{{Path: path, Write: !p.workerReadOnly}}
	if edit != "" && len(p.editClaims) > 1 {
		claims = append([]hub.ResourceClaim(nil), p.editClaims...)
		claims[0] = hub.ResourceClaim{Path: path, Write: !p.workerReadOnly}
	}
	var target hub.Target
	if p.workerTarget >= 0 && p.workerTarget < len(p.targets) {
		target = p.targets[p.workerTarget]
	}
	p.paneBusy = true
	go func() {
		var err error
		switch action {
		case "create":
			_, err = p.Service.Teams.Store.CreateTeam(p.ctx, lead)
		case "add":
			_, err = p.Service.AddWorker(p.ctx, lead, team, name, target, true)
		case "enqueue":
			if edit != "" {
				err = p.Service.Teams.Store.EditQueuedTask(p.ctx, lead, team, worker, edit, draft, claims)
			} else {
				_, err = p.Service.Teams.Store.EnqueueWorker(p.ctx, lead, team, worker, draft, claims)
			}
		case "resume":
			err = p.Service.Teams.Resume(p.ctx, lead, team, worker)
		case "stop":
			ctx, cancel := context.WithTimeout(p.ctx, 10*time.Second)
			defer cancel()
			err = p.Service.Teams.Stop(ctx, lead, team, worker)
		case "cancel":
			err = p.Service.Teams.Store.CancelQueuedTask(p.ctx, lead, team, worker, id)
		case "active", "hibernated", "archived":
			err = p.Service.Teams.Store.SetWorkerLifecycle(p.ctx, lead, team, worker, hub.WorkerLifecycle(action))
		case "end":
			err = p.Service.Teams.Store.EndTeam(p.ctx, lead, team)
		default:
			err = errors.New("unknown Team action")
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.teamLead != lead || p.pane != "team" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			if action == "enqueue" {
				p.workerDraft = ""
				p.editTaskID = ""
			}
			p.loadTeam()
			p.refreshSessions()
		})
	}()
}
func (p *NativePage) sendWorkerDraft(worker hub.Worker) {
	id, prompt, lead, team := p.selected, p.model.Draft, p.team.Team.LeadSessionID, p.team.Team.ID
	if worker.SessionID != id {
		p.problem = "Worker draft does not belong to the selected session."
		return
	}
	p.busy = true
	p.chat.Busy = true
	go func() {
		task, err := p.Service.EnqueueWorkerDraft(p.ctx, lead, team, worker.ID, prompt, nil)
		var startErr error
		if err == nil {
			startErr = p.Service.Teams.Resume(p.ctx, lead, team, worker.ID)
		}
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.model.Draft = ""
			delete(p.drafts, id)
			p.problem = "Queued actual task " + task.ID + "; waiting for its FIFO owner, not claiming completion."
			if startErr != nil {
				p.problem += " Start failed (task retained; do not resend): " + startErr.Error()
			}
			p.model.Status = view.Idle
			p.poll()
		})
	}()
}
func excerpt(text string, limit int) string {
	if len(text) <= limit {
		return text
	}
	text = text[:limit]
	for !utf8.ValidString(text) {
		text = text[:len(text)-1]
	}
	return text + "…"
}
func (p *NativePage) buildTeam(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(6).Children(func() {
		ui.Text(c, "Collaboration · durable Lead / Workers").Bold()
		ui.Text(c, "Workers are independent Hub sessions with their own native records, branches, queue and permissions. Viewing does not execute; queued work survives restart but requires explicit Resume. No autonomous/bypass permission is implied.").FontSize(11)
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Refresh Team").Disabled(p.paneBusy).OnClick(p.loadTeam)
			if p.team.Team.ID == "" || p.team.Team.Status != "active" {
				ui.Button(c, "Create Team on this Lead").Disabled(p.paneBusy).OnClick(func() { p.teamAction("create", "") })
			}
			if p.team.Team.ID != "" {
				ui.Button(c, "Focus Lead").OnClick(func() { p.selectSession(p.teamLead) })
			}
		})
		if p.team.Team.ID == "" {
			return
		}
		ui.Text(c, p.team.Team.ID+" · "+p.team.Team.Status).FontSize(11)
		ui.TextInput(c.Key("worker-name"), &p.workerName).Label("New Worker name")
		ui.Row(c).Gap(4).Children(func() {
			for i, target := range p.targets {
				i, target := i, target
				label := "Worker route: " + target.EngineID
				if p.workerTarget == i {
					label = "Selected " + target.EngineID
				}
				ui.Button(c.Key(fmt.Sprint(i)), label).OnClick(func() { p.workerTarget = i })
			}
		})
		if p.workerTarget < len(p.targets) {
			route := p.targets[p.workerTarget]
			ui.Text(c, "Adding confirms this actual account/billing route: "+route.ModelID+" · "+route.AccountSource+" · "+route.BillingRoute).FontSize(11)
		}
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Approve route and add Worker").Disabled(p.paneBusy || strings.TrimSpace(p.workerName) == "" || p.team.Team.Status != "active").OnClick(func() { p.teamAction("add", "") })
			ui.Button(c, "End Team and archive idle Workers").Disabled(p.paneBusy || p.team.Team.Status != "active").OnClick(func() { p.teamAction("end", "") })
		})
		p.workerList.Key = func(i int) any { return p.team.Workers[i].ID }
		ui.List(c, &p.workerList, len(p.team.Workers), func(i int) {
			w := p.team.Workers[i]
			ui.Button(c.Key(w.ID), w.Name+" · "+string(w.Lifecycle)).Disabled(p.paneBusy).OnClick(func() { p.taskBack = nil; p.editTaskID = ""; p.loadWorkerTasks(w.ID, 0) })
		}).Height(110).Shrink(0)
		if p.teamWorker == "" {
			return
		}
		var selected hub.Worker
		for _, w := range p.team.Workers {
			if w.ID == p.teamWorker {
				selected = w
			}
		}
		status := p.Service.Teams.Status(p.teamWorker)
		ui.Text(c, fmt.Sprintf("%s · owner active=%v · %s", selected.Name, status.Active, status.Error)).FontSize(11)
		p.buildWorkerWorktree(c, selected)
		ui.Column(c).Gap(4).Children(func() {
			ui.Button(c, "Focus Worker history").OnClick(func() { p.selectSession(selected.SessionID) })
			for _, entry := range []struct{ label, action string }{{"Resume queued tasks", "resume"}, {"Stop Worker and pump", "stop"}, {"Hibernate idle", "hibernated"}, {"Archive idle", "archived"}, {"Restore Worker", "active"}} {
				entry := entry
				ui.Button(c, entry.label).Disabled(p.paneBusy).OnClick(func() { p.teamAction(entry.action, "") })
			}
		})
		ui.TextInput(c.Key("worker-path"), &p.workerPath).Label("Declared workspace-relative resource (blank = whole workspace)")
		ui.Row(c).Gap(4).Children(func() {
			ui.Button(c, "Writer claim (default)").OnClick(func() { p.workerReadOnly = false })
			ui.Button(c, "Reader claim").OnClick(func() { p.workerReadOnly = true })
			ui.Text(c, fmt.Sprintf("read-only intent=%v; not a permission grant", p.workerReadOnly)).FontSize(11)
		})
		ui.TextArea(c.Key("worker-task"), &p.workerDraft).Label("Worker task / queued edit (16 KiB)").Height(70).Disabled(p.paneBusy)
		if p.editTaskID != "" && len(p.editClaims) > 1 {
			ui.Text(c, fmt.Sprintf("Editing %d claims: only the first path/mode changes here; remaining claims are preserved.", len(p.editClaims))).FontSize(11)
		}
		label := "Queue task only"
		if p.editTaskID != "" {
			label = "Save queued edit"
		}
		ui.Button(c, label).Disabled(p.paneBusy || strings.TrimSpace(p.workerDraft) == "" || len(p.workerDraft) > 16<<10).OnClick(func() { p.teamAction("enqueue", "") })
		p.teamList.Key = func(i int) any { return p.taskPage.Tasks[i].ID }
		ui.List(c, &p.teamList, len(p.taskPage.Tasks), func(i int) {
			task := p.taskPage.Tasks[i]
			ui.Column(c).Gap(3).Padding(4).Children(func() {
				ui.Text(c, task.ID+" · "+string(task.Status)+" · "+excerpt(task.Scope, 160)).FontSize(12)
				if task.RequiredBranchID != "" {
					ui.Text(c, "Pinned branch: "+task.RequiredBranchID+". Return to that saved branch before dispatch; attachments never follow another route.").FontSize(11)
				}
				if task.Outcome != nil {
					ui.Text(c, task.Outcome.Summary+"\n"+task.Outcome.Processing).FontSize(11)
				}
				if task.Status == hub.TaskQueued {
					ui.Row(c).Gap(4).Children(func() {
						ui.Button(c.Key("edit-"+task.ID), "Edit queued").Disabled(p.paneBusy).OnClick(func() {
							p.editTaskID = task.ID
							p.editClaims = append([]hub.ResourceClaim(nil), task.Claims...)
							p.workerDraft = task.Scope
							if len(task.Claims) > 0 {
								p.workerPath = task.Claims[0].Path
								p.workerReadOnly = !task.Claims[0].Write
							}
						})
						ui.Button(c.Key("cancel-"+task.ID), "Cancel queued").Disabled(p.paneBusy).OnClick(func() { p.teamAction("cancel", task.ID) })
					})
				}
			})
		}).Height(220).Shrink(0)
		ui.Row(c).Gap(6).Children(func() {
			ui.Button(c, "Previous tasks").Disabled(p.paneBusy || len(p.taskBack) == 0).OnClick(func() {
				last := len(p.taskBack) - 1
				cursor := p.taskBack[last]
				p.taskBack = p.taskBack[:last]
				p.loadWorkerTasks(p.teamWorker, cursor)
			})
			ui.Button(c, "More tasks").Disabled(p.paneBusy || !p.taskPage.HasMore).OnClick(func() {
				p.taskBack = append(p.taskBack, p.taskCursor)
				p.loadWorkerTasks(p.teamWorker, p.taskPage.Next)
			})
			ui.Button(c, "Refresh task page").Disabled(p.paneBusy).OnClick(func() { p.loadWorkerTasks(p.teamWorker, p.taskCursor) })
		})
	})
}
