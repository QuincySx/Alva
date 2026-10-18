// INPUT: Actual bounded pending approvals already resolved by product ownership
// OUTPUT: Shared native permission controls visible above every product pane
// POS: [L7 Hub product] One-call explicit consent is not hidden while reviewing settings, Inspector or collaboration
//go:build desktop

package hubdesktop

import (
	"fmt"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) buildApprovals(c *ui.Context) {
	shownApprovals := p.approvals
	if len(shownApprovals) > 1 {
		ui.Text(c, fmt.Sprintf("%d pending permissions; showing the first request. Remaining requests stay blocked.", len(shownApprovals))).FontSize(11)
		shownApprovals = shownApprovals[:1]
	}
	for _, request := range shownApprovals {
		request := request
		ui.Column(c).Gap(4).Children(func() {
			ui.Text(c, "Permission required: "+request.ToolName+" · "+string(request.Effect.Kind)).Bold()
			ui.Text(c, request.Effect.Description).FontSize(12)
			summary := request.InputSummary
			ui.TextArea(c.Key("approval-input-"+request.ID), &summary).Label("Requested input — review before allowing").ReadOnly(true).Height(90)
			for _, resource := range request.Effect.Resources {
				ui.Text(c, string(resource.Access)+": "+resource.Resource).FontSize(11)
			}
			ui.Text(c, fmt.Sprintf("Actual native session %s, call %s. One call only; no automatic approval.", request.SessionID, request.CallID)).FontSize(11)
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c.Key("allow-"+request.ID), "Allow once").OnClick(func() { p.respondApproval(request.ID, true) })
				ui.Button(c.Key("deny-"+request.ID), "Deny once").OnClick(func() { p.respondApproval(request.ID, false) })
			})
		})
	}
}
