// INPUT: Existing NativePage advanced pane state and shared product actions.
// OUTPUT: Existing tools and evidence destinations behind the quiet workspace header menu.
// POS: [L7 Hub UI] Retains existing workflows without exposing implementation instructions on the home screen.
//go:build desktop

package hubdesktop

import "github.com/egoist/mygo/ui"

func (p *NativePage) buildWorkspacePane(c *ui.Context) bool {
	switch p.pane {
	case "waits":
		p.buildWaits(c)
	case "settings":
		p.buildSettings(c)
	case "plugins":
		p.buildAEP(c)
	case "commands":
		p.buildCommands(c)
	case "tools":
		p.buildTools(c)
	case "gateway":
		p.buildGateway(c)
	case "media-settings":
		p.buildMediaSettings(c)
	case "browser-settings":
		p.buildBrowserSettings(c)
	case "media":
		p.buildMedia(c)
	case "handoffs":
		p.buildHandoffs(c)
	case "automations":
		p.buildAutomations(c)
	case "inspector":
		p.buildInspector(c)
	case "skills":
		p.buildSkills(c)
	case "team":
		p.buildTeam(c)
	case "routing":
		p.buildWorkspaceRouting(c)
	default:
		return false
	}
	return true
}
