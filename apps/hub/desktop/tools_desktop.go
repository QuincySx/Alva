// INPUT: Native explicit tool JSON, independent capability consent and actual product question snapshots
// OUTPUT: Next-turn tool settings and human clarification controls visible from every pane
// POS: [L7 Hub product] Native UI consumes shared services without owning execution or permission grants.
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) openTools() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("tools")
	p.shellJobs = nil
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		value, err := p.Service.Store.Settings(p.ctx, id)
		jobs, jobsErr := p.Service.ShellJobs(p.ctx, id)
		err = errors.Join(err, jobsErr)
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
			p.shellJobs = jobs
			raw, _ := json.MarshalIndent(value.Tools, "", "  ")
			p.toolsDraft = string(raw)
		})
	}()
}
func (p *NativePage) changeTools(kind string, enabled bool) {
	if p.selected == "" || p.paneBusy || p.model.ActiveRunID != "" {
		return
	}
	id, draft, generation := p.selected, p.toolsDraft, p.generation
	p.paneBusy = true
	go func() {
		var err error
		if kind == "" {
			err = p.Service.SaveToolsDraft(p.ctx, id, draft)
		} else {
			err = p.Service.SetToolEnabled(p.ctx, id, kind, enabled, enabled)
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
			p.openTools()
		})
	}()
}
func (p *NativePage) buildTools(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Optional tools · next turn").Bold()
		ui.Text(c, "Configure the shell executable/environment and web origins here. Saving clears tool enablement. Shell access and web access each need separate consent below; every call still follows permission rules. Nothing runs on save.").FontSize(12)
		ui.TextArea(c.Key("tool-json"), &p.toolsDraft).Label("Optional tool configuration JSON").Height(220).Disabled(p.paneBusy || p.model.ActiveRunID != "")
		ui.Button(c, "Save tool configuration disabled").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.changeTools("", false) })
		for _, v := range []struct {
			kind, label string
			enabled     bool
		}{
			{"planning", "Planning tasks and notes", p.settings.Tools.Planning},
			{"notebook", "Notebook cell editing", p.settings.Tools.Notebook},
			{"interaction", "Human questions", p.settings.Tools.Interaction},
			{"utility", "Preferences, tool search and sleep", p.settings.Tools.Utility},
			{"shell", "Full host shell access", p.settings.Tools.Shell.Enabled},
			{"web", "Network access to configured web tools", p.settings.Tools.Web.Enabled},
		} {
			v := v
			label := "Enable " + v.label + " next turn"
			if v.kind == "shell" || v.kind == "web" {
				label = "Approve " + v.label + " next turn"
			}
			if v.enabled {
				label = "Disable " + v.label + " next turn"
			}
			ui.Button(c.Key("tool-"+v.kind), label).Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.changeTools(v.kind, !v.enabled) })
		}
		p.buildShellJobs(c)
	})
}

func (p *NativePage) shellJobAction(branch, job string, reset bool) {
	if p.selected == "" || p.paneBusy {
		return
	}
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		var err error
		if reset {
			err = p.Service.ResetShellOwner(p.ctx, id, branch)
		} else if job != "" {
			err = p.Service.StopShellJob(p.ctx, id, branch, job)
		}
		jobs, listErr := p.Service.ShellJobs(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err = errors.Join(err, listErr); err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.shellJobs = jobs
		})
	}()
}
func (p *NativePage) buildShellJobs(c *ui.Context) {
	ui.Text(c, "Owned background Shell jobs").Bold()
	ui.Text(c, "Successful turns leave background jobs under this application's native-session owner. Each retains global host execution exclusion until actual reap. Stop works while another tool waits; it never accepts arbitrary PIDs. Closing the application stops jobs. Interrupted jobs are recorded unknown after restart and never adopted/replayed.").FontSize(11)
	ui.Button(c, "Refresh owned Shell jobs").Disabled(p.paneBusy).OnClick(func() { p.shellJobAction("", "", false) })
	p.shellJobList.Key = func(i int) any { return p.shellJobs[i].BranchID + "/" + p.shellJobs[i].Job.Status.Task.ID }
	ui.List(c, &p.shellJobList, len(p.shellJobs), func(i int) {
		entry := p.shellJobs[i]
		status := entry.Job.Status
		ui.Text(c, fmt.Sprintf("%s · %s · reaped=%v · %d output frames\nbranch %s · origin run %s", status.Task.ID, status.Task.Status, status.Reaped, entry.Job.Frames, entry.BranchID, entry.Job.RunID)).FontSize(11)
		ui.Button(c.Key("stop-"+status.Task.ID), "Stop owned Shell job "+status.Task.ID).Disabled(p.paneBusy || status.Reaped).OnClick(func() { p.shellJobAction(entry.BranchID, status.Task.ID, false) })
	}).Height(180).Shrink(0)
	if branch, err := product.ActiveBranch(p.detail); err == nil && branch.Target.EngineID == "alva" && branch.NativeSessionID != "" {
		ui.Button(c, "Stop all jobs in active branch and reset Shell owner").Disabled(p.paneBusy || p.model.ActiveRunID != "").OnClick(func() { p.shellJobAction(branch.ID, "", true) })
	}
}
func (p *NativePage) buildQuestions(c *ui.Context) {
	if len(p.questions) == 0 {
		return
	}
	q := p.questions[0]
	if p.questionID != q.ID {
		p.questionID = q.ID
		p.questionText = ""
		p.questionSelected = map[int]bool{}
	}
	ui.Scroll(c.Key("human-question")).Height(240).Gap(4).Children(func() {
		ui.Text(c, "Clarification: "+q.Header).Bold()
		ui.Text(c, q.Prompt).FontSize(12)
		ui.Text(c, "Your answer is a clarification. File, shell and network permissions use separate requests.").FontSize(11)
		for i, choice := range q.Options {
			index := i + 1
			label := fmt.Sprintf("%d. %s", index, choice.Label)
			if p.questionSelected[index] {
				label = "✓ " + label
			}
			ui.Button(c.Key(fmt.Sprintf("choice-%d", index)), label).OnClick(func() {
				if !q.MultiSelect {
					p.respondQuestion(q.ID, interaction.Answer{Selected: []int{index}})
					return
				}
				p.questionSelected[index] = !p.questionSelected[index]
			})
		}
		if q.MultiSelect {
			ui.Button(c, "Send selected choices").OnClick(func() {
				var selected []int
				for i := 1; i <= len(q.Options); i++ {
					if p.questionSelected[i] {
						selected = append(selected, i)
					}
				}
				p.respondQuestion(q.ID, interaction.Answer{Selected: selected})
			})
		}
		ui.TextArea(c.Key(q.ID), &p.questionText).Label("Clarification answer").Height(60)
		ui.Button(c, "Send clarification answer").OnClick(func() { text := p.questionText; p.respondQuestion(q.ID, interaction.Answer{Text: &text}) })
	})
}
func (p *NativePage) respondQuestion(question string, answer interaction.Answer) {
	id, generation := p.selected, p.generation
	go func() {
		err := p.Service.RespondQuestion(p.ctx, id, question, answer)
		p.dispatch(func() {
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.questionID = ""
			p.problem = ""
			p.poll()
		})
	}()
}
