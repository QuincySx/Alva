// INPUT: Shared Goal service, scoped keyset pages and page-local project/session drafts.
// OUTPUT: One native Goal dialog with cold saved-goal browsing and removable typed draft associations.
// POS: [L7 Hub UI] Creation and selection prepare source references; scope changes reject old callbacks and only Send executes.
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

type workspaceGoalState struct {
	open, saving bool
	request      hub.GoalCreate
	session      string
	workspace    string
	problem      string
	receipt      *hub.GoalReceipt
	focus        ui.Handle
	scope        string
	browsing     bool
	loading      bool
	preparing    bool
	sequence     uint64
	cancel       context.CancelFunc
	listScope    hub.GoalScope
	rows         []hub.Goal
	cursor, next hub.GoalCursor
	previous     []hub.GoalCursor
	hasMore      bool
}

func (p *NativePage) openWorkspaceGoal() {
	if p.workspacePickerBlocked() || p.goal.saving || p.workspaceComposerTarget().EngineID != "alva" {
		return
	}
	if p.goal.open {
		return
	}
	id, err := hub.NewID("goal-request-")
	if err != nil {
		p.problem = err.Error()
		return
	}
	workspace := p.workspace
	scope := hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: workspace}
	if p.selected != "" {
		workspace = p.detail.Session.Workspace
		scope = hub.GoalScope{Kind: hub.GoalSessionScope, Workspace: workspace, SessionID: p.selected}
	}
	p.cancelWorkspaceGoalRead()
	p.goal = workspaceGoalState{open: true, session: p.selected, workspace: workspace, scope: p.workspacePickerScope(), sequence: p.goal.sequence, request: hub.GoalCreate{RequestID: id, Scope: scope}}
	p.goal.focus.Focus()
}

func (p *NativePage) cancelWorkspaceGoal() {
	if p.goal.saving {
		return
	}
	p.goal.open = false
	p.cancelWorkspaceGoalRead()
}

func (p *NativePage) cancelWorkspaceGoalRead() {
	if p.goal.cancel != nil {
		p.goal.cancel()
		p.goal.cancel = nil
	}
	p.goal.sequence++
	p.goal.loading, p.goal.preparing = false, false
}

func (p *NativePage) openSavedWorkspaceGoals() {
	p.openWorkspaceGoal()
	if p.goal.open && !p.goal.saving && p.goal.scope == p.workspacePickerScope() {
		p.goal.browsing = true
		p.goal.listScope = p.goal.request.Scope
		p.queryWorkspaceGoals(hub.GoalCursor{}, nil)
	}
}

func (p *NativePage) queryWorkspaceGoals(cursor hub.GoalCursor, previous []hub.GoalCursor) {
	v := &p.goal
	if !v.open || !v.browsing || v.saving || p.Service.Goals == nil || v.workspace == "" || v.scope != p.workspacePickerScope() {
		return
	}
	p.cancelWorkspaceGoalRead()
	v.loading, v.problem = true, ""
	ctx, cancel := context.WithCancel(p.ctx)
	v.cancel = cancel
	sequence, scope, listScope := v.sequence, v.scope, v.listScope
	go func() {
		page, err := p.Service.Goals.Page(ctx, listScope, cursor, 20)
		p.dispatchWorkspacePicker(ctx, func() {
			v := &p.goal
			if !v.open || !v.browsing || v.sequence != sequence || v.scope != scope || p.workspacePickerScope() != scope {
				return
			}
			v.loading = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.rows, v.next, v.hasMore = page.Entries, page.Next, page.HasMore
			v.cursor, v.previous = cursor, previous
		})
	}()
}

func (p *NativePage) chooseWorkspaceGoal(goal hub.Goal) {
	v := &p.goal
	if !v.open || !v.browsing || v.loading || v.preparing || v.scope != p.workspacePickerScope() || p.workspacePickerBlocked() {
		return
	}
	if len(p.composerSelections[v.session]) >= 8 {
		v.problem = "草稿最多关联 8 项命令、Skill 或目标；请先移除一项引用。"
		return
	}
	for _, selected := range p.composerSelections[v.session] {
		if selected.EntryID == "goal:"+goal.ID {
			v.problem = "该目标已在草稿中。"
			return
		}
	}
	p.cancelWorkspaceGoalRead()
	v.preparing, v.problem = true, ""
	ctx, cancel := context.WithCancel(p.ctx)
	v.cancel = cancel
	sequence, scope, session, workspace := v.sequence, v.scope, v.session, v.workspace
	go func() {
		prepared, err := p.Service.PrepareComposerGoal(ctx, session, workspace, goal.ID)
		p.dispatchWorkspacePicker(ctx, func() {
			v := &p.goal
			if !v.open || !v.browsing || v.sequence != sequence || v.scope != scope || p.workspacePickerScope() != scope {
				return
			}
			v.preparing = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			if p.workspacePickerBlocked() || len(p.composerSelections[session]) >= 8 {
				v.problem = "草稿或运行状态已变化，请检查后重新选择。"
				return
			}
			if p.composerSelections == nil {
				p.composerSelections = make(map[string][]product.ComposerPreparation)
			}
			p.composerSelections[session] = append(p.composerSelections[session], prepared)
			p.cancelWorkspaceGoal()
		})
	}()
}

func (p *NativePage) createWorkspaceGoal() {
	v := &p.goal
	if !v.open || v.saving || p.Service.Goals == nil || v.scope != p.workspacePickerScope() {
		return
	}
	if len(p.composerSelections[v.session]) >= 8 {
		v.problem = "草稿最多关联 8 项命令、Skill 或目标；请先移除一项引用。"
		return
	}
	request := v.request
	request.Name = strings.TrimSpace(request.Name)
	request.CompletionCriteria = strings.TrimSpace(request.CompletionCriteria)
	if err := request.Validate(); err != nil {
		v.problem = err.Error()
		return
	}
	v.saving, v.problem = true, ""
	session, workspace, receipt, scope := v.session, v.workspace, v.receipt, v.scope
	go func() {
		var err error
		if receipt == nil {
			var result hub.GoalReceipt
			result, err = p.Service.Goals.Create(p.ctx, request)
			if err == nil {
				receipt = &result
			}
		}
		var prepared product.ComposerPreparation
		if err == nil {
			prepared, err = p.Service.PrepareComposerGoal(p.ctx, session, workspace, receipt.Goal.ID)
		}
		p.dispatch(func() {
			v := &p.goal
			if v.request.RequestID != request.RequestID {
				return
			}
			v.saving, v.receipt = false, receipt
			if p.workspacePickerScope() != scope || p.workspacePickerBlocked() {
				v.problem = "项目、会话或运行状态已变化；已保存的目标可在对应范围重新选择。"
				return
			}
			if err != nil {
				v.problem = "创建或关联失败，表单已保留：" + err.Error()
				return
			}
			if len(p.composerSelections[session]) >= 8 {
				v.problem = "目标已保存；草稿引用已满，请移除一项后重试关联。"
				return
			}
			if p.composerSelections == nil {
				p.composerSelections = make(map[string][]product.ComposerPreparation)
			}
			for _, selected := range p.composerSelections[session] {
				if selected.EntryID == prepared.EntryID {
					v.open = false
					return
				}
			}
			p.composerSelections[session] = append(p.composerSelections[session], prepared)
			v.open, v.problem = false, ""
		})
	}()
}

func (p *NativePage) buildWorkspaceGoal(c *ui.Context) {
	v := &p.goal
	if v.open && v.scope != p.workspacePickerScope() {
		v.open = false
		p.cancelWorkspaceGoalRead()
	}
	modal := ui.DialogBase(c.Key("workspace-goal"), &v.open, func(backdrop, panel ui.Element) {
		t := c.Theme()
		backdrop.Background(ui.RGBA(0, 0, 0, .35))
		panel.Width(480).MaxWidth(480).Height(592).Padding(24).Gap(16).Radius(16).Background(t.Surface)
		ui.Row(c).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
			ui.Icon(c, workspaceIcon("target")).Size(workspaceIconSize, workspaceIconSize).TextColor(t.Accent)
			title := "新建目标"
			if v.browsing {
				title = "已保存目标"
			}
			ui.Text(c, title).FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5).Grow(1)
			workspaceIconAction(c, "goal-close", "Close new goal", "x").Disabled(v.saving).OnClick(p.cancelWorkspaceGoal)
		})
		ui.Row(c).FillWidth().Gap(8).Shrink(0).Children(func() {
			workspaceControlButton(c, "goal-show-saved", "已保存目标", "secondary", v.saving || v.browsing || v.workspace == "").Label("Saved goals").OnClick(p.openSavedWorkspaceGoals)
			workspaceControlButton(c, "goal-show-new", "新建目标", "secondary", v.saving || !v.browsing).Label("New goal form").OnClick(func() {
				p.cancelWorkspaceGoalRead()
				v.browsing, v.problem = false, ""
				v.focus.Focus()
			})
		})
		if v.browsing {
			p.buildWorkspaceSavedGoals(c)
			return
		}
		locked := v.saving || v.receipt != nil
		ui.Scroll(c.Key("goal-form-details")).Label("Goal form details").Grow(1).MinHeight(0).Gap(12).Children(func() {
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				ui.Text(c, "目标名称").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
				workspaceControlInput(c, "goal-name", &v.request.Name).Label("Goal name").FillWidth().Ref(&v.focus).Disabled(locked)
			})
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				ui.Text(c, "完成标准").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
				workspaceControlTextArea(c, "goal-criteria", &v.request.CompletionCriteria).Label("Goal completion criteria").FillWidth().Height(112).Disabled(locked)
			})
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				ui.Text(c, "范围").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
				labels, selected := []string{"项目"}, "项目"
				if v.session != "" {
					labels = []string{"当前会话", "项目"}
					if v.request.Scope.Kind == hub.GoalSessionScope {
						selected = "当前会话"
					}
				}
				workspaceControlSelect(c, "goal-scope", &selected, labels).Label("Goal scope").FillWidth().Disabled(locked).OnChange(func() {
					v.request.Scope = hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: v.workspace}
					if selected == "当前会话" {
						v.request.Scope.Kind, v.request.Scope.SessionID = hub.GoalSessionScope, v.session
					}
				})
				if v.workspace == "" {
					ui.Text(c, "请先在工作台选择项目文件夹，再创建项目目标。").Label("Goal requires selected project").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
				} else {
					ui.Text(c, v.workspace).Label("Goal project scope").FontSize(workspaceFontCaption).TextColor(t.TextMuted).SingleLine()
				}
			})
			if v.receipt != nil {
				ui.Text(c, fmt.Sprintf("已创建 %s · revision %d；关联失败时可重试关联。", v.receipt.Goal.ID, v.receipt.Goal.Revision)).Label("Goal creation receipt").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
			}
			if v.problem != "" {
				ui.Text(c, v.problem).Label("Goal form error").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
			}
		})
		ui.Row(c).FillWidth().Gap(8).Justify(ui.End).Shrink(0).Children(func() {
			workspaceControlButton(c, "goal-cancel", "取消", "secondary", v.saving).Label("Cancel new goal").OnClick(p.cancelWorkspaceGoal)
			label := "创建目标"
			if v.receipt != nil {
				label = "重试关联"
			}
			workspaceControlButton(c, "goal-create", label, "primary", v.saving || v.workspace == "" || p.Service.Goals == nil || strings.TrimSpace(v.request.Name) == "" || strings.TrimSpace(v.request.CompletionCriteria) == "").Label("Create goal").OnClick(p.createWorkspaceGoal)
		})
	})
	if modal.Dismissed() && v.saving {
		v.open = true
	}
}

func (p *NativePage) buildWorkspaceSavedGoals(c *ui.Context) {
	v, t := &p.goal, c.Theme()
	labels, selected := []string{"项目"}, "项目"
	if v.session != "" {
		labels = []string{"当前会话", "项目"}
		if v.listScope.Kind == hub.GoalSessionScope {
			selected = "当前会话"
		}
	}
	workspaceControlSelect(c, "saved-goal-scope", &selected, labels).Label("Saved goal scope").FillWidth().Disabled(v.preparing).OnChange(func() {
		v.listScope = hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: v.workspace}
		if selected == "当前会话" {
			v.listScope.Kind, v.listScope.SessionID = hub.GoalSessionScope, v.session
		}
		v.rows = nil
		v.cursor, v.next = hub.GoalCursor{}, hub.GoalCursor{}
		v.previous, v.hasMore = nil, false
		p.queryWorkspaceGoals(hub.GoalCursor{}, nil)
	})
	ui.Scroll(c.Key("saved-goals")).Label("Saved goal list").FillWidth().Grow(1).MinHeight(0).Gap(8).Children(func() {
		if v.loading {
			ui.Text(c, "正在读取…").Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(t.TextMuted)
		}
		for _, goal := range v.rows {
			ui.ButtonBase(c.Key(goal.ID)).Label("Select saved goal "+goal.Name).FillWidth().Padding(12).Gap(4).Radius(8).
				Border(1, t.Border).Disabled(v.loading || v.preparing).OnClick(func() { p.chooseWorkspaceGoal(goal) }).Children(func() {
				ui.Text(c, goal.Name).FontSize(workspaceFontBody).FontWeight(500).LineHeight(1.5).TextColor(t.Text).SingleLine()
				ui.Text(c, goal.CompletionCriteria).FontSize(workspaceFontCaption).TextColor(t.TextMuted).LineHeight(1.5).MaxHeight(54)
			})
		}
		if !v.loading && len(v.rows) == 0 && v.problem == "" {
			ui.Text(c, "此范围还没有保存的目标。").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
		}
		if v.problem != "" {
			ui.Text(c, v.problem).Label("Saved goal error").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
		}
	})
	ui.Row(c).FillWidth().Gap(8).Shrink(0).Children(func() {
		workspaceControlButton(c, "saved-goal-refresh", "刷新", "secondary", v.loading || v.preparing).Label("Refresh saved goals").OnClick(func() { p.queryWorkspaceGoals(hub.GoalCursor{}, nil) })
		ui.Spacer(c).Grow(1)
		workspaceControlButton(c, "saved-goal-previous", "上一页", "secondary", v.loading || v.preparing || len(v.previous) == 0).Label("Previous saved goals").OnClick(func() {
			previous := append([]hub.GoalCursor(nil), v.previous...)
			cursor := previous[len(previous)-1]
			p.queryWorkspaceGoals(cursor, previous[:len(previous)-1])
		})
		workspaceControlButton(c, "saved-goal-next", "下一页", "secondary", v.loading || v.preparing || !v.hasMore).Label("Next saved goals").OnClick(func() {
			previous := append(append([]hub.GoalCursor(nil), v.previous...), v.cursor)
			p.queryWorkspaceGoals(v.next, previous)
		})
	})
}
