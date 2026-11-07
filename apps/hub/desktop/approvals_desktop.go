// INPUT: Actual bounded pending approvals already resolved by product ownership
// OUTPUT: Bounded complete permission review and shared loading actions above every product pane
// POS: [L7 Hub product] One-call explicit consent is not hidden while reviewing settings, Inspector or collaboration
//go:build desktop

package hubdesktop

import (
	"fmt"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) buildApprovals(c *ui.Context) {
	p.buildEnginePermissions(c)
	if p.approvalReadProblem != "" {
		ui.Text(c, p.approvalReadProblem).Role(ui.RoleStatus).TextColor(c.Theme().Danger).FontSize(workspaceFontCaption)
	}
	shownApprovals := p.approvals
	if len(shownApprovals) > 1 {
		ui.Text(c, fmt.Sprintf("%d pending permissions; showing the first request. Remaining requests stay blocked.", len(shownApprovals))).FontSize(11)
		shownApprovals = shownApprovals[:1]
	}
	for _, request := range shownApprovals {
		request := request
		submitting := p.approvalSubmitting[request.ID]
		ui.Column(c).Gap(4).Children(func() {
			ui.Text(c, "Permission required: "+request.ToolName+" · "+string(request.Effect.Kind)).Bold()
			ui.Scroll(c.Key("approval-review-" + request.ID)).Label("Permission request details").MaxHeight(180).Shrink(0).Gap(4).Children(func() {
				ui.Text(c, request.Effect.Description).FontSize(workspaceFontCaption)
				summary := request.InputSummary
				workspaceControlTextArea(c, "approval-input-"+request.ID, &summary).Label("Requested input — review before allowing").ReadOnly(true).Height(90)
				for _, resource := range request.Effect.Resources {
					ui.Text(c, string(resource.Access)+": "+resource.Resource).FontSize(11)
				}
				ui.Text(c, fmt.Sprintf("Actual native session %s, call %s. One call only; no automatic approval.", request.SessionID, request.CallID)).FontSize(11)
			})
			ui.Row(c).Gap(8).Children(func() {
				p.workspaceControlAction(c, "allow-"+request.ID, "Allow once", "", "primary", submitting, func() { p.respondApproval(request.ID, true) })
				p.workspaceControlAction(c, "deny-"+request.ID, "Deny once", "", "secondary", submitting, func() { p.respondApproval(request.ID, false) })
			})
		})
	}
}

func (p *NativePage) respondApproval(requestID string, allowed bool) {
	if p.selected == "" || p.approvalSubmitting[requestID] {
		return
	}
	if p.approvalSubmitting == nil {
		p.approvalSubmitting = make(map[string]bool)
	}
	p.approvalSubmitting[requestID] = true
	p.approvalEpoch++ // An earlier observation cannot restore this submitted request.
	id, generation, branch := p.selected, p.generation, p.detail.Session.ActiveBranchID
	go func() {
		err := p.Service.RespondApproval(p.ctx, id, requestID, allowed)
		p.dispatch(func() {
			delete(p.approvalSubmitting, requestID)
			p.approvalEpoch++ // Also reject observations begun while consent was in flight.
			if p.selected != id || p.generation != generation || p.detail.Session.ActiveBranchID != branch {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			for i, request := range p.approvals {
				if request.ID == requestID {
					p.approvals = append(p.approvals[:i], p.approvals[i+1:]...)
					break
				}
			}
			p.problem = ""
			p.poll()
		})
	}()
}
