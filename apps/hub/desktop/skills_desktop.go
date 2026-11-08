// INPUT: Confined project skill service, the shared SettingsService and native form controls
// OUTPUT: Rules & Skills page: skill list, name/description/body editor, archive/restore, session switch and compatibility file
// POS: [L7 Hub UI] Saved-workspace skill manager; no script execution, global scan or automatic model activation
//go:build desktop

package hubdesktop

import (
	"errors"
	"os"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"github.com/egoist/mygo/ui"
)

// skillFormState holds the skill editor; the body draft is p.skillDraft.
type skillFormState struct {
	open                    bool
	original, name, summary string
	explicit                bool
	errors                  product.FieldErrors
	notice                  string
	session                 product.SessionSettingsView
}

func (p *NativePage) openSkills() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("skills")
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		var session product.SessionSettingsView
		var err error
		if p.Service.Settings == nil {
			err = errors.New("当前程序没有提供设置服务。")
		} else {
			var view product.SettingsView
			view, err = p.Service.Settings.Load(p.ctx, product.SettingsScope{App: true, SessionID: id})
			if err == nil && view.Session != nil {
				session = *view.Session
			}
		}
		var list []skill.Skill
		if err == nil {
			list, err = p.Service.Skills(p.ctx, id)
			if errors.Is(err, os.ErrNotExist) {
				list, err = nil, nil
			}
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "skills" {
				return
			}
			p.skillForm.session = session
			p.settings.SkillsEnabled, p.settings.CompatibilityFile, p.settings.SkillArchives = session.SkillsEnabled, session.CompatibilityFile, session.SkillArchives
			p.compatibilityDraft = session.CompatibilityFile
			if err != nil {
				p.problem = err.Error()
				p.skills = nil
				return
			}
			p.problem = ""
			p.skills = list
		})
	}()
}

func (p *NativePage) skillAction(action, value string) {
	if p.selected == "" || p.paneBusy || p.inputBlocked() {
		return
	}
	id, generation := p.selected, p.generation
	form := product.SkillForm{Name: p.skillForm.name, Description: p.skillForm.summary, Body: p.skillDraft, Explicit: p.skillForm.explicit}
	original := p.skillForm.original
	p.paneBusy = true
	go func() {
		var err error
		receipt, notice := "", ""
		switch action {
		case "initialize":
			err = p.Service.InitializeSkills(p.ctx, id)
		case "save":
			err, notice = p.Service.SaveSkillForm(p.ctx, id, original, form), "技能已保存，下一轮生效。"
		case "archive":
			receipt, err = p.Service.ArchiveSkill(p.ctx, id, value)
			notice = "已归档，可以在下方恢复。"
		case "restore":
			err, notice = p.Service.RestoreSkill(p.ctx, id, value), "已恢复。"
		case "enable", "disable":
			if p.Service.Settings == nil {
				err = errors.New("当前程序没有提供设置服务。")
			} else {
				err = p.Service.Settings.SetSessionSwitch(p.ctx, id, "skills", action == "enable")
			}
		case "compatibility":
			err = p.Service.SetCompatibilityFile(p.ctx, id, value)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "skills" {
				return
			}
			var problems product.FieldErrors
			if errors.As(err, &problems) {
				p.skillForm.errors = problems
				return
			}
			if err != nil {
				p.problem = err.Error()
				if receipt != "" {
					p.problem += "；归档仍可恢复：" + receipt
				}
				return
			}
			p.problem = ""
			if action == "save" {
				p.skillForm.open, p.skillForm.errors = false, nil
				p.skillDraft = ""
			}
			p.skillForm.notice = notice
			p.openSkills()
		})
	}()
}

func (p *NativePage) newSkill() {
	p.skillForm = skillFormState{open: true, session: p.skillForm.session}
	p.skillDraft = ""
}

func (p *NativePage) loadSkill(name string) {
	if p.paneBusy || p.selected == "" {
		return
	}
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		form, err := p.Service.SkillFormFor(p.ctx, id, name)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation || p.pane != "skills" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.skillForm = skillFormState{open: true, original: name, name: form.Name, summary: form.Description, explicit: form.Explicit, session: p.skillForm.session}
			p.skillDraft = form.Body
		})
	}()
}

func (p *NativePage) skillFieldError(c *ui.Context, field string) {
	if message := p.skillForm.errors.For(field); message != "" {
		ui.Text(c, message).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().Danger).Wrap()
	}
}

func (p *NativePage) buildSkills(c *ui.Context) {
	t := c.Theme()
	blocked := p.paneBusy || p.inputBlocked()
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(24).Children(func() {
			settingsPageTitle(c, "规则与 Skills", "技能保存在当前任务工作区的 "+product.ManagedSkillDirectory+"，Alva 只在需要时读取正文，不会运行其中的脚本。")
			if p.skillForm.notice != "" {
				ui.Text(c, p.skillForm.notice).Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(t.Success)
			}
			ui.Row(c.Key("skills-switch")).FillWidth().AlignItems(ui.Center).Gap(12).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Text(c, "在这个任务里启用项目技能").FontSize(workspaceFontBody)
					ui.Text(c, "下一轮生效；技能声明的工具不会因此获得额外权限。").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
				})
				enabled := p.skillForm.session.SkillsEnabled
				workspaceControlSwitch(c, "skills-enabled", &enabled, blocked).Label("Toggle project skills").OnChange(func() {
					action := "disable"
					if enabled {
						action = "enable"
					}
					p.skillAction(action, "")
				})
			})
			settingsGroup(c, "skills-list", "技能", "", func() {
				if len(p.skills) == 0 {
					ui.Text(c, "还没有技能。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
				}
				for _, item := range p.skills {
					item := item
					ui.Row(c.Key("skill-row-"+item.Meta.Name)).FillWidth().AlignItems(ui.Center).Gap(8).Padding(4, 0).Children(func() {
						ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
							ui.Text(c, item.Meta.Name).FontSize(workspaceFontBody)
							ui.Text(c, item.Meta.Description).FontSize(workspaceFontCaption).TextColor(t.TextMuted).Wrap()
						})
						workspaceControlIconButton(c, "skill-edit-"+item.Meta.Name, "编辑", "pencil", "ghost", blocked).Label("Edit skill " + item.Meta.Name).OnClick(func() { p.loadSkill(item.Meta.Name) })
						workspaceControlButton(c, "archive-"+item.Meta.Name, "归档", "ghost", blocked).Label("Archive skill " + item.Meta.Name).OnClick(func() { p.skillAction("archive", item.Meta.Name) })
					})
				}
				ui.Row(c).Gap(8).Children(func() {
					workspaceControlIconButton(c, "skill-new", "新建技能", "plus", "secondary", blocked).Label("New skill").OnClick(p.newSkill)
					workspaceControlButton(c, "skills-initialize", "创建技能目录", "ghost", blocked).Label("Initialize project skill directory").OnClick(func() { p.skillAction("initialize", "") })
					workspaceControlIconButton(c, "skills-refresh", "刷新", "refresh-cw", "ghost", p.paneBusy).Label("Refresh project skills").OnClick(p.openSkills)
				})
			})
			if p.skillForm.open {
				title := "新建技能"
				if p.skillForm.original != "" {
					title = "编辑技能"
				}
				settingsGroup(c, "skill-editor", title, "编辑会先归档旧版本（含附带文件），保存失败会自动恢复。", func() {
					ui.Text(c, "名称").FontSize(workspaceFontBody)
					workspaceControlInput(c, "skill-name", &p.skillForm.name).Label("Skill name").Placeholder("例如 release-notes").FillWidth().Disabled(blocked)
					p.skillFieldError(c, "skill.name")
					ui.Text(c, "说明").FontSize(workspaceFontBody)
					workspaceControlInput(c, "skill-description", &p.skillForm.summary).Label("Skill description").Placeholder("什么时候用这个技能").FillWidth().Disabled(blocked)
					p.skillFieldError(c, "skill.description")
					ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(12).Children(func() {
						ui.Text(c, "只在明确提到时使用").FontSize(workspaceFontBody).Grow(1)
						workspaceControlSwitch(c, "skill-explicit", &p.skillForm.explicit, blocked).Label("Skill explicit invocation")
					})
					ui.Text(c, "正文").FontSize(workspaceFontBody)
					workspaceControlTextArea(c, "skill-document", &p.skillDraft).Label("Skill body").Height(220).FillWidth().Disabled(blocked)
					p.skillFieldError(c, "skill.body")
					p.skillFieldError(c, "skill")
					ui.Row(c).Gap(8).Justify(ui.End).FillWidth().Children(func() {
						workspaceControlButton(c, "skill-cancel", "取消", "secondary", p.paneBusy).Label("Cancel skill edit").OnClick(func() { p.skillForm.open, p.skillForm.errors = false, nil })
						p.workspaceControlAction(c, "skill-save", "保存", "", "primary", p.paneBusy, func() { p.skillAction("save", "") }, p.inputBlocked()).Label("Save skill")
					})
				})
			}
			if len(p.skillForm.session.SkillArchives) > 0 {
				settingsGroup(c, "skill-archives", "已归档", "归档会一直保留，直到你手动恢复。", func() {
					for _, receipt := range p.skillForm.session.SkillArchives {
						receipt := receipt
						ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
							ui.Text(c, receipt).FontSize(workspaceFontCaption).Grow(1).MinWidth(0).SingleLine()
							workspaceControlButton(c, "restore-"+receipt, "恢复", "ghost", blocked).Label("Restore archive " + receipt).OnClick(func() { p.skillAction("restore", receipt) })
						})
					}
				})
			}
			settingsGroup(c, "rules-compatibility", "项目规则兼容", "可选：指定项目里的兼容配置 JSON（相对路径），把已有的规则文件和 Agent 定义带进来。不会全局扫描或运行脚本；Agent 定义还需要开启原生子 Agent。留空表示关闭。", func() {
				ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
					workspaceControlInput(c, "compatibility-file", &p.compatibilityDraft).Label("Compatibility configuration file").Placeholder("例如 .alva/compat.json").Grow(1).MinWidth(0).Disabled(blocked)
					workspaceControlButton(c, "compatibility-apply", "应用", "secondary", blocked).Label("Apply compatibility next turn").OnClick(func() { p.skillAction("compatibility", p.compatibilityDraft) })
				})
			})
		})
	})
}
