// INPUT: Existing NativePage advanced pane state and shared product actions.
// OUTPUT: Existing tools and evidence destinations behind the quiet workspace header menu, and whether one replaces the conversation.
// POS: [L7 Hub UI] Retains existing workflows without exposing implementation instructions on the home screen.
//go:build desktop

package hubdesktop

import "github.com/egoist/mygo/ui"

// workspacePaneBuilder is the advanced pane the current selection shows in
// place of the conversation, or nil when the conversation is shown.
func (p *NativePage) workspacePaneBuilder() func(*ui.Context) {
	switch p.pane {
	case "waits":
		return p.buildWaits
	case "settings":
		return p.buildSettings
	case "plugins":
		return p.buildAEP
	case "commands":
		return p.buildCommands
	case "tools":
		return p.buildTools
	case "gateway":
		return p.buildGateway
	case "media-settings":
		return p.buildMediaSettings
	case "browser-settings":
		return p.buildBrowserSettings
	case "media":
		return p.buildMedia
	case "handoffs":
		return p.buildHandoffs
	case "automations":
		return p.buildAutomations
	case "inspector":
		return p.buildInspector
	case "skills":
		return p.buildSkills
	case "team":
		return p.buildTeam
	case "routing":
		return p.buildWorkspaceRouting
	}
	return nil
}

func (p *NativePage) buildWorkspacePane(c *ui.Context) bool {
	build := p.workspacePaneBuilder()
	if build == nil {
		return false
	}
	build(c)
	return true
}
