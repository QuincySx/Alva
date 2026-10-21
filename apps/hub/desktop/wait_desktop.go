// INPUT: The same product wait DTO/commands as HTTP and explicit MyGo operator input.
// OUTPUT: Observed durable checkpoints, typed question/JSON result drafts and explicit same-Turn continuation controls.
// POS: [L7 presentation] Observes checkpoints without restoring an Agent; never treats a result as execution or cancellation.
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"fmt"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
	"time"
)

type waitPane struct {
	View    product.WaitView
	Answers map[string]string
	Confirm string
	Epoch   uint64
	Current bool
	Problem string
}

// Observation can advance a checkpoint from another window. Keep only drafts
// for the same unresolved request and binding; a new revision needs a new
// abandonment review even when it belongs to the same Turn.
func (p *NativePage) applyWaitView(view product.WaitView) {
	previous := p.wait.View
	answers := map[string]string{}
	if previous.BranchID == view.BranchID && previous.TurnID == view.TurnID {
		for _, request := range view.Snapshot.State.Requests {
			if len(request.Result) != 0 {
				continue
			}
			for _, old := range previous.Snapshot.State.Requests {
				if old.ID == request.ID && old.Binding == request.Binding {
					if answer, ok := p.wait.Answers[request.ID]; ok {
						answers[request.ID] = answer
					}
					break
				}
			}
		}
	}
	if previous.BranchID != view.BranchID || previous.TurnID != view.TurnID || previous.Snapshot.Revision != view.Snapshot.Revision {
		p.wait.Confirm = ""
	}
	p.wait.View, p.wait.Answers, p.wait.Current = view, answers, true
	p.wait.Problem = ""
}

func (p *NativePage) inputBlocked() bool {
	return p.model.ActiveRunID != "" || p.detail.Session.Status == engine.Waiting
}
func (p *NativePage) openWaits() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("waits")
	p.wait.Epoch++
	p.wait.Current = false
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		view, err := p.Service.Waits(p.ctx, id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.wait.Problem = err.Error()
				return
			}
			p.applyWaitView(view)
		})
	}()
}
func (p *NativePage) waitAction(action func(string) error) {
	if p.paneBusy || p.selected == "" {
		return
	}
	id, generation := p.selected, p.generation
	p.wait.Epoch++
	p.wait.Current = false
	p.paneBusy = true
	go func() {
		err := action(id)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
			} else {
				p.problem = ""
				p.wait.Confirm = ""
			}
			p.openWaits()
			p.poll()
		})
	}()
}
func (p *NativePage) waitTarget() product.WaitTarget {
	return product.WaitTarget{BranchID: p.wait.View.BranchID, TurnID: p.wait.View.TurnID, Revision: p.wait.View.Snapshot.Revision}
}
func (p *NativePage) resolveWait(id, binding string, result json.RawMessage) {
	target := p.waitTarget()
	p.waitAction(func(session string) error {
		return p.Service.ResolveWait(p.ctx, session, product.WaitResolution{Target: target, RequestID: id, Binding: binding, Result: result})
	})
}
func (p *NativePage) buildWaits(c *ui.Context) {
	v := p.wait.View
	blocked := p.paneBusy || !p.wait.Current
	resolvable := !blocked && v.Status == engine.Waiting && v.Snapshot.State.Status == durable.Waiting && p.model.ActiveRunID == ""
	ui.Row(c).Gap(8).Children(func() {
		ui.Button(c, "Refresh waiting tasks").Disabled(p.paneBusy).OnClick(p.openWaits)
		ui.Button(c, "Continue this waiting Turn").Disabled(blocked || !v.CanResume).OnClick(func() {
			target := p.waitTarget()
			p.waitAction(func(id string) error { _, err := p.Service.ResumeWait(p.ctx, id, target); return err })
		})
	})
	ui.Scroll(c).Label("Waiting task details").Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Durable waiting tasks").Bold()
		ui.Text(c, "Waiting exits execution and retains the checkpoint. Supplying a result does not run a model/tool. Continue explicitly resumes the same Turn; old tools and input are not replayed.").FontSize(12)
		if !p.wait.Current {
			ui.Text(c, "Checkpoint unavailable or refreshing. Actions require a current successful read.").FontSize(12)
		}
		if p.wait.Problem != "" {
			ui.Text(c, "Checkpoint read failed: "+p.wait.Problem).FontSize(12)
		}
		if !v.Supported {
			if p.wait.Current {
				ui.Text(c, "This engine has no durable-wait capability.")
			}
			return
		}
		label := "Enable durable waits next Turn"
		if v.Enabled {
			label = "Disable durable waits next Turn"
		}
		ui.Button(c, label).Disabled(blocked || p.inputBlocked() || v.Snapshot.State.Status != durable.Idle).OnClick(func() {
			p.waitAction(func(id string) error { return p.Service.EnableDurableWaits(p.ctx, id, !v.Enabled) })
		})
		ui.Text(c, fmt.Sprintf("Task: %s · checkpoint: %s · revision: %d", v.TurnID, v.Snapshot.State.Status, v.Snapshot.Revision))
		if v.TurnID != "" {
			turn := v.TurnID
			ui.TextInput(c.Key("wait-turn-id"), &turn).Label("Waiting Turn ID").ReadOnly(true)
		}
		if v.ResumeProblem != "" {
			ui.Text(c, v.ResumeProblem).FontSize(12)
		}
		for _, request := range v.Snapshot.State.Requests {
			r := request
			live := time.Now().Before(r.ExpiresAt)
			canResolve := resolvable && live
			ui.Text(c, fmt.Sprintf("%s · %s · expires %s", r.Kind, r.ID, r.ExpiresAt.Format("2006-01-02 15:04:05 MST"))).Bold()
			if !live {
				ui.Text(c, "Request expired. Review the checkpoint before explicitly abandoning it.").FontSize(12)
			}
			if r.Kind == "question" {
				ui.Column(c.Key("wait-question-" + r.ID)).Gap(4).Children(func() {
					p.buildWaitQuestion(c, r, canResolve)
				})
			}
			payload := string(r.Payload)
			ui.TextArea(c.Key("wait-payload-"+r.ID), &payload).Label("Complete stored request — untrusted data").ReadOnly(true).Height(160)
			ui.Text(c, "Binding: "+r.Binding).FontSize(11)
			if len(r.Result) != 0 {
				ui.Text(c, "Result saved. Execution remains suspended until Continue.")
				result := string(r.Result)
				ui.TextArea(c.Key("wait-result-"+r.ID), &result).Label("Complete saved result — untrusted data").ReadOnly(true).Height(80)
				continue
			}
			if r.Kind == "question" {
				continue
			} else if r.Kind == "approval" {
				ui.Row(c).Gap(8).Children(func() {
					ui.Button(c.Key("wait-allow-"+r.ID), "Allow this exact action once").Disabled(!canResolve).OnClick(func() { p.resolveWait(r.ID, r.Binding, json.RawMessage(`{"allow":true}`)) })
					ui.Button(c.Key("wait-deny-"+r.ID), "Deny this exact action").Disabled(!canResolve).OnClick(func() { p.resolveWait(r.ID, r.Binding, json.RawMessage(`{"allow":false}`)) })
				})
			} else {
				answer := p.wait.Answers[r.ID]
				ui.TextArea(c.Key("wait-answer-"+r.ID), &answer).Label("Explicit result JSON (at most 16 KiB)").Height(100).Disabled(!canResolve).OnChange(func() { p.wait.Answers[r.ID] = answer })
				ui.Button(c.Key("wait-save-"+r.ID), "Save bound result without executing").Disabled(!canResolve || len(answer) == 0 || len(answer) > 16<<10).OnClick(func() { p.resolveWait(r.ID, r.Binding, json.RawMessage(answer)) })
			}
		}
		if v.Snapshot.State.Status != durable.Idle || v.Status == engine.Waiting {
			ui.Text(c, "Abandonment acknowledges unknown effects. It neither undoes actions nor cancels external work. Review native Inspector/history first; type the exact Turn ID below.").FontSize(12)
			ui.TextInput(c.Key("wait-abandon-confirm"), &p.wait.Confirm).Label("Reviewed Turn ID for abandonment").Disabled(blocked)
			ui.Button(c, "Abandon reviewed checkpoint").Disabled(blocked || v.TurnID == "" || p.wait.Confirm != v.TurnID || p.model.ActiveRunID != "").OnClick(func() {
				target := p.waitTarget()
				p.waitAction(func(id string) error { return p.Service.AbandonWait(p.ctx, id, target) })
			})
		}
	})
}
