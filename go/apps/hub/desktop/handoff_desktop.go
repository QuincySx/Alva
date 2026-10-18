// INPUT: Bounded loaded timeline handoff identities and owner-scoped durable product snapshots
// OUTPUT: Explicit read-only engine/Team handoff detail without native-log or runtime restoration
// POS: [L7 Hub product] Presentation shows exact saved redacted handoff; it never switches, cancels or replays execution
//go:build desktop

package hubdesktop

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
)

func (p *NativePage) readHandoff(handoffID string) {
	if p.selected == "" || p.paneBusy {
		return
	}
	id, generation := p.selected, p.generation
	p.paneBusy = true
	p.handoffText = ""
	go func() {
		value, err := p.Service.Store.ReadHandoff(p.ctx, id, handoffID)
		var raw []byte
		if err == nil {
			raw, err = json.MarshalIndent(value, "", "  ")
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "handoffs" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.handoffText = string(raw)
		})
	}()
}
func (p *NativePage) buildHandoffs(c *ui.Context) {
	ui.Scroll(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Exact saved handoff evidence").Bold()
		ui.Text(c, "Only handoff IDs in the currently loaded bounded history page are listed. Use Earlier/Latest events in Chat to change that range. Clicking reads the exact durable redacted snapshot; it does not start or resume an engine. Team/task metadata reflects capture time, not current liveness or network receipt.").FontSize(11)
		count := 0
		for _, entry := range p.transcript.Entries {
			if entry.Timeline == nil || entry.Timeline.HandoffID == "" {
				continue
			}
			id := entry.Timeline.HandoffID
			count++
			ui.Button(c.Key(id), "Read handoff "+id).Disabled(p.paneBusy).OnClick(func() { p.readHandoff(id) })
		}
		if count == 0 {
			ui.Text(c, "No handoff in this loaded range. No session/branch is inferred or restored.").FontSize(11)
		}
		if p.handoffText != "" {
			ui.Text(c, p.handoffText).FontSize(11)
		}
	})
}
