// INPUT: Bounded shared automation index and explicit existing-session navigation.
// OUTPUT: Home-reachable real schedule overview and scope selection for existing CRUD.
// POS: [L7 Hub UI] Listing plans does not tick, create sessions, enable schedules or start an engine.
//go:build desktop

package hubdesktop

import (
	"context"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

type workspaceAutomationIndexState struct {
	items   []hub.Automation
	loading bool
	problem string
	epoch   uint64
	cancel  context.CancelFunc
	list    ui.ListState
}

func (p *NativePage) openWorkspaceAutomations() {
	if p.busy || p.paneBusy {
		return
	}
	p.setPane("automation-index")
	p.refreshWorkspaceAutomationIndex()
}

func (p *NativePage) refreshWorkspaceAutomationIndex() {
	v := &p.workspaceAutomationIndex
	if v.cancel != nil {
		v.cancel()
	}
	ctx, cancel := context.WithCancel(p.ctx)
	v.cancel, v.loading, v.problem = cancel, true, ""
	v.epoch++
	epoch := v.epoch
	go func() {
		defer cancel()
		items, err := p.Service.BrowseAutomations(ctx)
		p.dispatch(func() {
			if p.workspaceAutomationIndex.epoch != epoch {
				return
			}
			v.cancel, v.loading = nil, false
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.items = items
		})
	}()
}

func (p *NativePage) buildWorkspaceAutomationIndex(c *ui.Context) bool {
	if p.pane != "automation-index" {
		return false
	}
	v := &p.workspaceAutomationIndex
	ui.Column(c.Key("workspace-automation-index")).Label("Automation workspace").FillWidth().Grow(1).MinHeight(0).Padding(24).Gap(16).Children(func() {
		ui.Row(c).FillWidth().Gap(8).Children(func() {
			workspaceControlIconButton(c, "automation-index-new", "新建自动化", "plus", "primary", p.Service.Automations == nil || p.busy).
				Label("Choose task for new automation").OnClick(func() { p.openWorkspaceSessionChooser("automation") })
			ui.Box(c).Grow(1)
			workspaceControlIconButton(c, "automation-index-refresh", "刷新", "refresh-cw", "ghost", v.loading).
				Label("Refresh automation index").OnClick(p.refreshWorkspaceAutomationIndex)
		})
		if v.loading {
			p.workspaceLoadingIcon(c, 16, c.Theme().TextMuted)
			return
		}
		if v.problem != "" {
			ui.Text(c, v.problem).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().Danger)
			return
		}
		if len(v.items) == 0 {
			ui.Text(c, "还没有自动化").Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
			return
		}
		v.list.Key = func(i int) any { return v.items[i].ID }
		v.list.Label = func(i int) string { return v.items[i].Title }
		ui.List(c.Key("automation-index-list"), &v.list, len(v.items)).Label("Saved automations").FillWidth().Grow(1).MinHeight(0).Gap(8).Rows(func(row ui.ListRow) {
			item := v.items[row.Index]
			button := ui.ButtonBase(row.Context.Key(item.ID)).Label("Manage automation " + item.ID).FillWidth().Padding(12).Gap(12).Radius(8).
				AlignItems(ui.Center).TextColor(c.Theme().Text).OnClick(func() { p.openWorkspaceSessionChooserForSession("automation", item.SessionID) })
			if button.Hovered() {
				button.Background(workspacePalette(c).subtle)
			}
			button.Children(func() {
				ui.Column(row.Context).Grow(1).MinWidth(0).Gap(4).Children(func() {
					ui.Text(row.Context, item.Title).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine()
					status := "已暂停"
					if item.RunningTurnID != "" {
						status = "正在运行"
					} else if item.Enabled {
						status = "下次 " + time.UnixMilli(item.NextRunAt).Local().Format("01-02 15:04")
					}
					ui.Text(row.Context, status).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
				})
				ui.Icon(row.Context, workspaceIcon("chevron-right")).Size(16, 16).TextColor(c.Theme().TextMuted)
			})
		})
	})
	return true
}
