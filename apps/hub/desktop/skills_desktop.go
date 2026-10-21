// INPUT: Confined project skill service and native editor controls
// OUTPUT: Explicit initialize/refresh/install/archive/restore and next-turn Skill enable UI
// POS: [L7 Hub product] Saved-workspace skill manager; no script execution, global scan or automatic model activation
//go:build desktop

package hubdesktop

import (
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) openSkills() {
	if p.selected == "" || p.paneBusy {
		return
	}
	p.setPane("skills")
	id, generation := p.selected, p.generation
	p.paneBusy = true
	go func() {
		settings, err := p.Service.Store.Settings(p.ctx, id)
		var list []skill.Skill
		if err == nil {
			list, err = p.Service.Skills(p.ctx, id)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			p.settings = settings
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
	id := p.selected
	p.paneBusy = true
	go func() {
		var err error
		receipt := ""
		switch action {
		case "initialize":
			err = p.Service.InitializeSkills(p.ctx, id)
		case "install":
			err = p.Service.InstallSkill(p.ctx, id, value)
		case "archive":
			receipt, err = p.Service.ArchiveSkill(p.ctx, id, value)
		case "restore":
			err = p.Service.RestoreSkill(p.ctx, id, value)
		case "enable":
			err = p.Service.Store.SetSkillsEnabled(p.ctx, id, true)
		case "disable":
			err = p.Service.Store.SetSkillsEnabled(p.ctx, id, false)
		}
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				if receipt != "" {
					p.problem += "; archive remains recoverable at receipt " + receipt
				}
				return
			}
			p.problem = ""
			p.openSkills()
		})
	}()
}
func (p *NativePage) loadSkill(name string) {
	if p.paneBusy || p.selected == "" {
		return
	}
	id := p.selected
	p.paneBusy = true
	go func() {
		document, err := p.Service.SkillRaw(p.ctx, id, name)
		p.dispatch(func() {
			p.paneBusy = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.skillDraft = document
		})
	}()
}
func (p *NativePage) buildSkills(c *ui.Context) {
	ui.Column(c).Grow(1).MinHeight(0).Gap(8).Children(func() {
		ui.Text(c, "Managed project skills · "+product.ManagedSkillDirectory).Bold()
		ui.Text(c, "Bound to saved workspace. Initializing/refreshing is explicit; no global scan, download or script execution. Enabling affects only the next Alva turn. Metadata is advertised; body/resources are loaded only when requested.").FontSize(11)
		ui.Row(c).Gap(8).Children(func() {
			ui.Button(c, "Initialize project skill directory").Disabled(p.paneBusy || p.inputBlocked()).OnClick(func() { p.skillAction("initialize", "") })
			ui.Button(c, "Refresh project skills").Disabled(p.paneBusy).OnClick(p.openSkills)
			label := "Enable project skills next turn"
			action := "enable"
			if p.settings.SkillsEnabled {
				label = "Disable project skills next turn"
				action = "disable"
			}
			ui.Button(c, label).Disabled(p.paneBusy || p.inputBlocked()).OnClick(func() { p.skillAction(action, "") })
		})
		ui.Scroll(c).Height(130).Children(func() {
			ui.Column(c).Gap(4).Children(func() {
				for _, item := range p.skills {
					item := item
					ui.Row(c).Gap(8).Children(func() {
						ui.Button(c, item.Meta.Name+" · "+item.Meta.Description).Disabled(p.paneBusy).OnClick(func() { p.loadSkill(item.Meta.Name) })
						ui.Button(c.Key("archive-"+item.Meta.Name), "Archive recoverably").Disabled(p.paneBusy || p.inputBlocked()).OnClick(func() { p.skillAction("archive", item.Meta.Name) })
					})
				}
			})
		})
		ui.TextArea(c.Key("skill-document"), &p.skillDraft).Label("Complete SKILL.md with YAML frontmatter").Grow(1).Disabled(p.paneBusy || p.inputBlocked())
		ui.Button(c, "Install new managed skill (never overwrite existing)").Disabled(p.paneBusy || p.inputBlocked() || p.skillDraft == "").OnClick(func() { p.skillAction("install", p.skillDraft) })
		ui.Scroll(c).Height(80).Children(func() {
			ui.Column(c).Gap(4).Children(func() {
				for _, receipt := range p.settings.SkillArchives {
					receipt := receipt
					ui.Button(c, "Restore archive: "+receipt).Disabled(p.paneBusy || p.inputBlocked()).OnClick(func() { p.skillAction("restore", receipt) })
				}
			})
		})
		ui.Text(c, "Skill tool declarations never grant host permissions. Editing existing bundles requires archive then explicit new install; archives remain until manually restored. Binary resources are not interpreted or fed as media.").FontSize(11)
	})
}
