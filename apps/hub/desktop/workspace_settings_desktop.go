// INPUT: Existing workspace panes, minimal media index locations and native MyGo controls.
// OUTPUT: Shared Settings navigation and one return destination retained across category changes.
// POS: [L7 Hub UI] Navigation preserves drafts and execution owners while releasing hidden media bodies.
//go:build desktop

package hubdesktop

import (
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/egoist/mygo/ui"
)

type workspaceMediaLocation struct {
	branch, mode, source, selection string
	cursor, highWater, request      uint64
	back                            []uint64
}

type workspaceReturnState struct {
	pane, selected string
	generation     uint64
	media          workspaceMediaLocation
	restoreMedia   bool
}

func workspaceSettingsPane(name string) bool {
	return name == "preferences"
}

func (p *NativePage) isWorkspaceSettings() bool {
	return workspaceSettingsPane(p.pane)
}

// Settings categories share a single origin; switching categories never creates
// another return step or reconstructs an Inspector page from its first cursor.
func (p *NativePage) workspacePaneDestination(name string) string {
	if workspaceSettingsPane(name) {
		if !p.isWorkspaceSettings() {
			p.composer.closePopovers()
			p.workspaceReturn = workspaceReturnState{pane: p.pane, selected: p.selected, generation: p.generation}
			if p.pane == "media" {
				v := &p.media
				p.workspaceReturn.media = workspaceMediaLocation{branch: v.Branch, mode: v.Mode, source: v.SourceURI, selection: v.SelectionID,
					cursor: v.Cursor, highWater: v.Page.HighWater, request: v.Request, back: append([]uint64(nil), v.Back...)}
			}
		}
		return name
	}
	if !p.isWorkspaceSettings() {
		return name
	}
	origin := p.workspaceReturn
	p.workspaceReturn = workspaceReturnState{}
	if name != "" || origin.selected != p.selected || origin.generation != p.generation {
		return name
	}
	if origin.pane == "media" {
		v := origin.media
		p.media = MediaPane{Branch: v.branch, Mode: v.mode, SourceURI: v.source, SelectionID: v.selection,
			Cursor: v.cursor, Back: v.back, Request: v.request + 1, Page: storage.ArtifactIndexPage{HighWater: v.highWater}}
		origin.restoreMedia = true
		p.workspaceReturn = origin
	}
	return origin.pane
}

// An explicit return reads only its prior metadata page. If a read from before
// Settings is still settling, keep the location until that owner releases it.
func (p *NativePage) restoreWorkspacePane() {
	origin := p.workspaceReturn
	if !origin.restoreMedia {
		return
	}
	if p.pane != "media" || p.selected != origin.selected || p.generation != origin.generation {
		p.workspaceReturn = workspaceReturnState{}
		return
	}
	if p.paneBusy {
		return
	}
	p.workspaceReturn = workspaceReturnState{}
	p.loadMedia(origin.media.cursor, origin.media.highWater)
}

func (p *NativePage) buildWorkspaceSettings(c *ui.Context) {
	t := c.Theme()
	ui.Row(c).Fill().Gap(0).Children(func() {
		ui.Column(c).Label("Settings navigation").Width(workspaceSidebarWidth).Shrink(0).
			FillHeight().Padding(12).Gap(4).Background(workspacePalette(c).sidebar).Children(func() {
			ui.Row(c).Children(func() {
				back := ui.ButtonBase(c.Key("settings-back-workspace")).Label("Back to workspace").
					Padding(8, 12).Gap(8).AlignItems(ui.Center).Radius(8).TextColor(t.Text)
				if back.Hovered() {
					back.Background(t.SurfaceHover)
				}
				back.OnClick(func() { p.setPane("") }).Children(func() {
					ui.Icon(c, workspaceIcon("chevron-left")).Size(20, 20).TextColor(t.TextMuted).Shrink(0)
					ui.Text(c, "返回工作台").FontSize(14).LineHeight(1.5).SingleLine()
				})
			})
			ui.Text(c, "设置").FontSize(20).FontWeight(600).Padding(12, 12, 16)
			workspaceAction(c, "settings-general", "通用", "sliders-horizontal", p.pane == "preferences").
				Label("General settings").Disabled(p.Service.Preferences == nil).OnClick(p.openPreferences)
		})
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().Padding(24).Gap(16).Children(func() {
			if p.playerBusy {
				ui.Button(c, "暂停本地播放").Label("Pause local video and wait for decoder cleanup").Disabled(p.playerStopping).OnClick(p.stopPlayback)
			}
			if p.problem != "" {
				ui.Text(c, p.problem).Role(ui.RoleStatus).TextColor(t.Danger).FontSize(14)
			}
			p.buildPreferences(c)
		})
	})
}
