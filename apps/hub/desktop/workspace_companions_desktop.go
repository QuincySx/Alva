// INPUT: Committed companion/group facts, separate group drafts, Foundation controls and explicit shared product actions.
// OUTPUT: Live/deleted discovery, receipt-bound scope changes, isolated keyset history and explicit execution controls.
// POS: [L7 Hub UI] Opening or editing never executes; finite group rounds and private chats retain their original Hub owners.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type workspaceCompanionState struct {
	busy, running, stopping                        bool
	groupReloadPending                             bool
	loaded                                         bool
	archive                                        hub.CompanionArchive
	memberHistory                                  string
	archiveProblem                                 string
	historyWarning                                 string
	messageBefore, roundBefore                     string
	moreMessages, moreRounds                       bool
	profiles                                       []hub.CompanionProfile
	groups                                         []hub.CompanionGroup
	profile                                        hub.CompanionProfile
	group                                          hub.CompanionGroup
	groupDraft                                     hub.CompanionGroup
	groupEpoch                                     uint64
	selected, kind, editor, problem, query, prompt string
	approved, deleteConfirm                        bool
	members                                        map[string]bool
	mentions                                       map[string]bool
	messages                                       []hub.CompanionMessage
	rounds                                         []hub.CompanionRound
	conversation                                   hub.CompanionConversation
	scope                                          uint64
	skillSource, skillName                         string
	skillGroupAllowed                              bool
	skillCatalog                                   []product.ComposerCatalogEntry
	approvedProfiles                               []hub.CompanionProfile
	roundReviews                                   map[string]product.CompanionRoundReview
	roundApprovals                                 map[string]uint64
	terminationRound                               string
}

func (p *NativePage) openWorkspaceCompanions() {
	p.setPane("companions")
	if !p.companions.loaded {
		p.reloadWorkspaceCompanions()
	}
}
func (p *NativePage) reloadWorkspaceCompanions() {
	v := &p.companions
	if v.busy || p.Service.Companions == nil {
		return
	}
	v.busy = true
	go func() {
		profiles, err := p.Service.Companions.List(p.ctx, hub.LocalCompanionOwner)
		var archive hub.CompanionArchive
		var archiveErr error
		var groups []hub.CompanionGroup
		if err == nil {
			groups, err = p.Service.Companions.Groups(p.ctx, hub.LocalCompanionOwner)
		}
		if err == nil {
			archive, archiveErr = p.Service.Companions.Archive(p.ctx, hub.LocalCompanionOwner)
		}
		p.dispatch(func() {
			v.busy = false
			defer func() {
				if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
			}()
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.profiles, v.groups, v.loaded = profiles, groups, true
			v.archiveProblem = ""
			if archiveErr == nil {
				v.archive = archive
			} else if errors.Is(archiveErr, engine.ErrUnsupported) {
				v.archive = hub.CompanionArchive{}
				v.archiveProblem = "当前存储不支持已删除历史浏览"
			} else {
				v.archiveProblem = archiveErr.Error()
			}
			v.problem = ""
		})
	}()
}
func (p *NativePage) selectWorkspaceCompanion(profile hub.CompanionProfile) {
	v := &p.companions
	profile.Skills = append([]hub.CompanionSkill(nil), profile.Skills...)
	v.profile = profile
	v.group = hub.CompanionGroup{}
	v.groupDraft = hub.CompanionGroup{}
	v.groupEpoch++
	v.kind = "profile"
	v.memberHistory = ""
	v.selected = profile.ID
	v.editor = ""
	v.conversation = hub.CompanionConversation{}
	p.clearWorkspaceCompanionGroupHistory()
	v.problem = ""
	v.approved = false
	v.deleteConfirm = false
	v.prompt = ""
	p.loadWorkspaceCompanionPrivate(profile.ID)
}
func (p *NativePage) selectWorkspaceCompanionGroup(group hub.CompanionGroup) {
	v := &p.companions
	group.Members = append([]string(nil), group.Members...)
	v.group = group
	v.groupDraft = hub.CompanionGroup{}
	v.groupEpoch++
	v.profile = hub.CompanionProfile{}
	v.kind = "group"
	v.memberHistory = ""
	v.selected = group.ID
	v.editor = ""
	v.conversation = hub.CompanionConversation{}
	p.clearWorkspaceCompanionGroupHistory()
	v.scope = group.ScopeRevision
	v.mentions = make(map[string]bool)
	v.approved = false
	v.approvedProfiles = nil
	v.deleteConfirm = false
	v.prompt = ""
	p.loadWorkspaceCompanionGroup()
}
func (p *NativePage) clearWorkspaceCompanionGroupHistory() {
	v := &p.companions
	v.memberHistory = ""
	v.messages, v.rounds = nil, nil
	v.messageBefore, v.roundBefore = "", ""
	v.moreMessages, v.moreRounds = false, false
	v.roundReviews, v.roundApprovals = nil, nil
	v.terminationRound = ""
	v.historyWarning = ""
}
func (p *NativePage) selectWorkspaceCompanionGroupScope(scope uint64) {
	v := &p.companions
	v.scope = scope
	v.groupEpoch++
	p.clearWorkspaceCompanionGroupHistory()
	v.problem = ""
	p.loadWorkspaceCompanionGroup()
}
func (p *NativePage) loadWorkspaceCompanionGroup() {
	v := &p.companions
	if v.group.ID == "" {
		return
	}
	if v.busy {
		v.groupReloadPending = true
		return
	}
	v.groupReloadPending = false
	v.busy = true
	id, scope, epoch := v.group.ID, v.scope, v.groupEpoch
	go func() {
		messages, err := p.Service.Companions.MessagePage(p.ctx, hub.LocalCompanionOwner, id, scope, hub.CompanionHistoryQuery{Limit: 256})
		var rounds hub.CompanionRoundPage
		if err == nil {
			rounds, err = p.Service.Companions.RoundPage(p.ctx, hub.LocalCompanionOwner, id, scope, hub.CompanionHistoryQuery{Limit: 32})
		}
		historyWarning := ""
		if errors.Is(err, engine.ErrUnsupported) {
			// The same provider's original bounded observation remains available for
			// owner-bound Stop/review. It must never claim complete historical access.
			messages = hub.CompanionMessagePage{}
			rounds = hub.CompanionRoundPage{}
			messages.Messages, err = p.Service.Companions.Messages(p.ctx, hub.LocalCompanionOwner, id, scope, 256)
			if err == nil {
				rounds.Rounds, err = p.Service.Companions.Rounds(p.ctx, hub.LocalCompanionOwner, id, scope, 32)
			}
			historyWarning = "当前存储仅提供最近 256 条消息 / 32 轮；不支持加载更早历史"
		}
		reviews := make(map[string]product.CompanionRoundReview)
		if err == nil {
			for _, r := range rounds.Rounds {
				if companionRoundUnresolved(r.Status) {
					review, e := p.Service.ReviewCompanionRound(p.ctx, hub.LocalCompanionOwner, r.ID)
					if e != nil {
						err = e
						break
					}
					reviews[r.ID] = review
				}
			}
		}
		p.dispatch(func() {
			v.busy = false
			// A completed operation needs a read started after its real owner
			// returned. An earlier observer snapshot cannot satisfy that refresh.
			if v.groupReloadPending {
				p.loadWorkspaceCompanionGroup()
				return
			}
			if v.group.ID != id || v.scope != scope || v.groupEpoch != epoch {
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.messages, v.rounds = messages.Messages, rounds.Rounds
			v.messageBefore, v.moreMessages = messages.NextBefore, messages.HasMore
			v.roundBefore, v.moreRounds = rounds.NextBefore, rounds.HasMore
			v.roundReviews = reviews
			v.historyWarning = historyWarning
			v.problem = ""
		})
	}()
}
func (p *NativePage) newWorkspaceCompanion() {
	v := &p.companions
	v.group, v.groupDraft = hub.CompanionGroup{}, hub.CompanionGroup{}
	v.groupEpoch++
	p.clearWorkspaceCompanionGroupHistory()
	v.kind = "profile"
	v.selected = ""
	v.editor = "profile"
	v.profile = hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Target: p.Service.DefaultTarget, Enabled: true, PermissionPolicy: hub.PermissionAsk}
	v.conversation = hub.CompanionConversation{}
	v.problem = ""
	v.deleteConfirm = false
}
func (p *NativePage) newWorkspaceCompanionGroup() {
	v := &p.companions
	v.group = hub.CompanionGroup{}
	v.groupEpoch++
	p.clearWorkspaceCompanionGroupHistory()
	v.kind = "group"
	v.selected = ""
	v.editor = "group"
	v.groupDraft = hub.CompanionGroup{OwnerID: hub.LocalCompanionOwner, Enabled: true, ReplyMode: "mentions", ReplyLimit: 3, Workspace: p.workspace}
	v.members = make(map[string]bool)
	v.mentions = make(map[string]bool)
	v.approved = false
	v.approvedProfiles = nil
	v.problem = ""
	v.deleteConfirm = false
}
func (p *NativePage) saveWorkspaceCompanion() {
	v := &p.companions
	if v.busy {
		return
	}
	v.busy = true
	profile, epoch := v.profile, v.groupEpoch
	go func() {
		saved, err := p.Service.SaveCompanionProfile(p.ctx, profile, profile.Revision)
		p.dispatch(func() {
			v.busy = false
			if v.groupEpoch != epoch || v.kind != "profile" || v.editor != "profile" || v.profile.ID != profile.ID {
				if err == nil {
					p.reloadWorkspaceCompanions()
				} else if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.profile = saved
			v.approved = false
			v.approvedProfiles = nil
			v.selected = saved.ID
			v.editor = ""
			v.problem = ""
			p.reloadWorkspaceCompanions()
		})
	}()
}
func (p *NativePage) saveWorkspaceCompanionGroup() {
	v := &p.companions
	if v.busy || v.editor != "group" {
		return
	}
	g, epoch := v.groupDraft, v.groupEpoch
	g.Members = nil
	for _, profile := range v.profiles {
		if v.members[profile.ID] {
			g.Members = append(g.Members, profile.ID)
		}
	}
	v.busy = true
	go func() {
		saved, err := p.Service.SaveCompanionGroup(p.ctx, g, g.Revision)
		p.dispatch(func() {
			v.busy = false
			if v.groupEpoch != epoch || v.editor != "group" || v.groupDraft.ID != g.ID {
				if err == nil {
					p.reloadWorkspaceCompanions()
				} else if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			scopeChanged := v.group.ID != saved.ID || v.group.ScopeRevision != saved.ScopeRevision
			v.group = saved
			v.groupDraft = hub.CompanionGroup{}
			v.members = nil
			v.approved = false
			v.approvedProfiles = nil
			v.mentions = make(map[string]bool)
			v.selected = saved.ID
			v.editor = ""
			v.problem = ""
			if scopeChanged {
				v.scope = saved.ScopeRevision
				v.groupEpoch++
				p.clearWorkspaceCompanionGroupHistory()
				v.groupReloadPending = true
			}
			p.reloadWorkspaceCompanions()
		})
	}()
}
func (p *NativePage) openWorkspaceCompanionChat() {
	v := &p.companions
	if v.busy {
		return
	}
	v.busy = true
	id, approved, epoch, generation := v.profile.ID, v.approved, v.groupEpoch, p.generation
	go func() {
		conversation, err := p.Service.OpenCompanion(p.ctx, hub.LocalCompanionOwner, id, approved)
		p.dispatch(func() {
			v.busy = false
			if v.profile.ID != id || v.kind != "profile" || v.groupEpoch != epoch {
				if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.conversation = conversation
			if p.pane != "companions" || p.generation != generation {
				return
			}
			p.selectSession(conversation.SessionID)
			p.pane = "companions"
		})
	}()
}
func (p *NativePage) sendWorkspaceCompanion() {
	v := &p.companions
	if v.busy || v.conversation.SessionID == "" {
		return
	}
	v.busy = true
	id, prompt, approved := v.profile.ID, v.prompt, v.approved
	snapshots := append([]hub.CompanionProfile(nil), v.approvedProfiles...)
	go func() {
		_, err := p.Service.SubmitCompanion(context.Background(), hub.LocalCompanionOwner, id, prompt, approved, snapshots...)
		p.dispatch(func() {
			v.busy = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			if v.profile.ID == id && v.prompt == prompt {
				v.prompt = ""
			}
			v.problem = ""
			p.poll()
		})
	}()
}
func (p *NativePage) runWorkspaceCompanionGroup() {
	v := &p.companions
	if v.busy || v.running {
		return
	}
	g, prompt, approved := v.group, v.prompt, v.approved
	snapshots := append([]hub.CompanionProfile(nil), v.approvedProfiles...)
	var members []string
	if g.ReplyMode == "mentions" {
		for _, id := range g.Members {
			if v.mentions[id] {
				members = append(members, id)
			}
		}
	}
	v.running = true
	v.problem = ""
	go func() {
		_, err := p.Service.RunCompanionGroup(context.Background(), hub.LocalCompanionOwner, g.ID, g.Revision, prompt, members, approved, snapshots...)
		p.dispatch(func() {
			v.running = false
			v.stopping = false
			if v.group.ID != g.ID {
				return
			}
			if err != nil {
				v.problem = err.Error()
			} else if v.prompt == prompt {
				v.prompt = ""
			}
			p.loadWorkspaceCompanionGroup()
		})
	}()
}
func (p *NativePage) stopWorkspaceCompanionGroup() {
	v := &p.companions
	if v.stopping {
		return
	}
	var target hub.CompanionRound
	for _, r := range v.rounds {
		if companionRoundUnresolved(r.Status) && r.OperationID != "" {
			target = r
			break
		}
	}
	if target.ID == "" {
		p.loadWorkspaceCompanionGroup()
		return
	}
	v.stopping = true
	go func() {
		err := p.Service.StopCompanionGroup(context.Background(), hub.LocalCompanionOwner, target.GroupID, target.ID, target.Revision, target.OperationID)
		p.dispatch(func() {
			v.stopping = false
			if err != nil {
				v.problem = err.Error()
			}
			p.loadWorkspaceCompanionGroup()
		})
	}()
}

func (p *NativePage) workspaceCompanionGroupRunning() bool {
	if p.companions.running {
		return true
	}
	for _, r := range p.companions.rounds {
		if companionRoundUnresolved(r.Status) {
			return true
		}
	}
	return false
}
func (p *NativePage) editWorkspaceCompanionGroup() {
	v := &p.companions
	v.editor = "group"
	v.groupDraft = v.group
	v.groupDraft.Members = append([]string(nil), v.group.Members...)
	v.members = make(map[string]bool)
	for _, id := range v.group.Members {
		v.members[id] = true
	}
}
func (p *NativePage) cancelWorkspaceCompanionGroup() {
	v := &p.companions
	v.groupDraft = hub.CompanionGroup{}
	v.members = nil
	v.editor = ""
	v.problem = ""
}
func (p *NativePage) deleteWorkspaceCompanion() {
	v := &p.companions
	if v.busy {
		return
	}
	v.busy = true
	kind, id, epoch := v.kind, v.selected, v.groupEpoch
	profile, group := v.profile, v.group
	go func() {
		var err error
		if kind == "profile" {
			err = p.Service.Companions.Delete(p.ctx, hub.LocalCompanionOwner, id, profile.Revision)
		} else {
			err = p.Service.Companions.DeleteGroup(p.ctx, hub.LocalCompanionOwner, id, group.Revision)
		}
		p.dispatch(func() {
			v.busy = false
			if v.groupEpoch != epoch || v.kind != kind || v.selected != id {
				if err == nil {
					p.reloadWorkspaceCompanions()
				} else if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.selected = ""
			v.editor = ""
			v.deleteConfirm = false
			p.reloadWorkspaceCompanions()
		})
	}()
}
func (p *NativePage) toggleWorkspaceCompanion() {
	v := &p.companions
	if v.busy {
		return
	}
	v.busy = true
	kind, id, epoch := v.kind, v.selected, v.groupEpoch
	profile, group := v.profile, v.group
	go func() {
		var err error
		if kind == "profile" {
			profile.Enabled = !profile.Enabled
			profile, err = p.Service.SaveCompanionProfile(p.ctx, profile, profile.Revision)
		} else {
			group.Enabled = !group.Enabled
			group, err = p.Service.SaveCompanionGroup(p.ctx, group, group.Revision)
		}
		p.dispatch(func() {
			v.busy = false
			if v.groupEpoch != epoch || v.kind != kind || v.selected != id {
				if err == nil {
					p.reloadWorkspaceCompanions()
				} else if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			if kind == "profile" {
				v.profile = profile
			} else {
				v.group = group
			}
			p.reloadWorkspaceCompanions()
		})
	}()
}

func (p *NativePage) buildWorkspaceCompanions(c *ui.Context) {
	v := &p.companions
	t := c.Theme()
	if p.Service.Companions == nil {
		ui.Text(c, "伙伴服务不可用")
		return
	}
	ui.Row(c).Fill().Gap(0).Children(func() {
		ui.Column(c.Key("companion-roster")).Width(220).Shrink(0).FillHeight().Padding(16).Gap(12).Background(workspacePalette(c).subtle).Children(func() {
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				ui.Text(c, "伙伴与群聊").FontSize(workspaceFontSubtitle).Grow(1)
				workspaceIconAction(c, "companion-refresh", "Refresh companions", "refresh-cw").Disabled(v.busy).OnClick(p.reloadWorkspaceCompanions)
			})
			if v.archiveProblem != "" {
				ui.Text(c, v.archiveProblem).Label("Companion archive unavailable").FontSize(11).TextColor(t.TextMuted).Wrap()
			}
			workspaceControlInput(c, "companion-search", &v.query).Label("Search companions and groups").Placeholder("搜索伙伴和群聊…").FillWidth()
			ui.Scroll(c).Grow(1).FillWidth().Children(func() {
				ui.Column(c).FillWidth().Gap(6).Children(func() {
					for _, profile := range append(append([]hub.CompanionProfile(nil), v.profiles...), v.archive.Profiles...) {
						profile := profile
						if v.query != "" && !strings.Contains(strings.ToLower(profile.Name), strings.ToLower(v.query)) {
							continue
						}
						name := profile.Name
						label := "Open companion " + profile.Name
						if profile.Deleted {
							name += " · 已删除历史"
							label = "Open archived companion " + profile.Name
						} else if !profile.Enabled {
							name += " · 已停用"
						}
						workspaceAction(c, "companion-"+profile.ID, name, "message-square", v.selected == profile.ID).Label(label).Disabled(v.busy || v.running).OnClick(func() { p.selectWorkspaceCompanion(profile) })
					}
					for _, g := range append(append([]hub.CompanionGroup(nil), v.groups...), v.archive.Groups...) {
						g := g
						if v.query != "" && !strings.Contains(strings.ToLower(g.Name), strings.ToLower(v.query)) {
							continue
						}
						name := g.Name
						label := "Open group " + g.Name
						if g.Deleted {
							name += " · 已删除历史"
							label = "Open archived group " + g.Name
						} else if !g.Enabled {
							name += " · 已停用"
						}
						workspaceAction(c, "group-"+g.ID, name, "users-round", v.selected == g.ID).Label(label).Disabled(v.busy || v.running).OnClick(func() { p.selectWorkspaceCompanionGroup(g) })
					}
				})
			})
			workspaceControlIconButton(c, "companion-new", "新建伙伴", "plus", "secondary", v.busy || v.running).Label("New companion").FillWidth().OnClick(p.newWorkspaceCompanion)
			workspaceControlIconButton(c, "group-new", "新建群聊", "users-round", "secondary", v.busy || v.running).Label("New companion group").FillWidth().OnClick(p.newWorkspaceCompanionGroup)
			ui.Text(c, "伙伴身份与模型、Agent 分开管理").FontSize(11).TextColor(t.TextMuted).Wrap()
		})
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().Padding(24).Gap(16).Children(func() {
			if v.problem != "" {
				ui.Text(c, v.problem).Label("Companion operation error").TextColor(t.Danger).Wrap()
			}
			if v.editor == "profile" {
				p.buildWorkspaceCompanionProfile(c)
				return
			}
			if v.editor == "group" {
				p.buildWorkspaceCompanionGroupEditor(c)
				return
			}
			if v.selected == "" {
				ui.Text(c, "选择伙伴，或创建一个伙伴和群聊").FontSize(20)
				ui.Text(c, "保存资料和创建群聊不会运行模型。").TextColor(t.TextMuted)
				return
			}
			p.buildWorkspaceCompanionHeader(c)
			if v.kind == "group" {
				p.buildWorkspaceCompanionGroupChat(c)
			} else {
				p.buildWorkspaceCompanionChat(c)
			}
		})
	})
}
func (p *NativePage) buildWorkspaceCompanionHeader(c *ui.Context) {
	v := &p.companions
	name := v.profile.Name
	enabled := v.profile.Enabled
	if v.kind == "group" {
		name = v.group.Name
		enabled = v.group.Enabled
	}
	if v.profile.Deleted || v.group.Deleted {
		ui.Text(c, name+" · 已删除历史（只读）").FontSize(20)
		ui.Text(c, "保留原会话与群范围；不会恢复身份、账号授权或执行。").TextColor(c.Theme().TextMuted).Wrap()
		return
	}
	ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		ui.Text(c, name).FontSize(20).Grow(1)
		workspaceControlIconButton(c, "companion-edit", "设置", "pencil", "secondary", v.busy || p.workspaceCompanionGroupRunning()).Label("Edit companion or group").OnClick(func() {
			if v.kind == "group" {
				p.editWorkspaceCompanionGroup()
			} else {
				v.editor = "profile"
			}
		})
		label := "停用"
		if !enabled {
			label = "启用"
		}
		workspaceControlButton(c, "companion-toggle", label, "secondary", v.busy || p.workspaceCompanionGroupRunning()).Label("Toggle companion or group").OnClick(p.toggleWorkspaceCompanion)
		workspaceIconAction(c, "companion-delete", "Delete companion or group", "trash-2").Disabled(v.busy || p.workspaceCompanionGroupRunning()).OnClick(func() { v.deleteConfirm = true })
	})
	if v.deleteConfirm {
		ui.Row(c).Gap(8).Children(func() {
			ui.Text(c, "移除名册项；已有会话和历史仍保留。").Wrap()
			workspaceControlButton(c, "companion-delete-confirm", "确认删除", "danger", v.busy).Label("Confirm companion deletion").OnClick(p.deleteWorkspaceCompanion)
			workspaceControlButton(c, "companion-delete-cancel", "取消", "secondary").OnClick(func() { v.deleteConfirm = false })
		})
	}
}
func (p *NativePage) buildWorkspaceCompanionProfile(c *ui.Context) {
	v := &p.companions
	ui.Text(c, "伙伴资料 · 身份与能力").FontSize(workspaceFontHeading)
	ui.Scroll(c).Grow(1).FillWidth().Children(func() {
		ui.Column(c).FillWidth().Gap(12).Children(func() {
			ui.Text(c, "名字")
			workspaceControlInput(c, "companion-name", &v.profile.Name).Label("Companion name").FillWidth().Disabled(v.busy)
			ui.Text(c, "职责与做事方式")
			workspaceControlTextArea(c, "companion-role", &v.profile.Role).Label("Companion role").FillWidth().Height(100).Disabled(v.busy)
			labels := []string{}
			targets := p.Service.AvailableTargets()
			selected := ""
			for _, target := range targets {
				label := workspaceTargetLabel(target) + " · " + target.AccountSource + " · " + target.BillingRoute
				labels = append(labels, label)
				if hub.RouteKey(target) == hub.RouteKey(v.profile.Target) {
					selected = label
				}
			}
			workspaceControlSelect(c, "companion-target", &selected, labels).Label("Companion model and Agent").FillWidth().Disabled(v.busy).OnChange(func() {
				for i, label := range labels {
					if label == selected {
						v.profile.Target = targets[i]
					}
				}
			})
			ui.Text(c, "私聊工作区（首次打开后固定）")
			workspaceControlInput(c, "companion-workspace", &v.profile.Workspace).Label("Companion workspace").Placeholder("留空使用此伙伴的独立私有工作区").FillWidth().Disabled(v.busy || v.conversation.SessionID != "")
			policy := "每次询问"
			if v.profile.PermissionPolicy == hub.PermissionAuto {
				policy = "自动批准低风险"
			}
			workspaceControlSelect(c, "companion-policy", &policy, []string{"每次询问", "自动批准低风险"}).Label("Companion permission policy").Disabled(v.busy).OnChange(func() {
				v.profile.PermissionPolicy = hub.PermissionAsk
				if policy == "自动批准低风险" {
					v.profile.PermissionPolicy = hub.PermissionAuto
				}
			})
			p.buildWorkspaceCompanionSkills(c)
			ui.Text(c, "Skill 文本不会授予工具、文件、MCP 或账号权限；资料保存后在下一轮生效。Full 访问须在真实会话中单独确认。").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Wrap()
		})
	})
	ui.Row(c).Gap(8).Children(func() {
		workspaceControlButton(c, "companion-cancel", "取消", "secondary", v.busy).Label("Cancel companion profile").OnClick(p.cancelWorkspaceCompanionProfile)
		ui.Spacer(c)
		workspaceControlButton(c, "companion-save", "保存伙伴", "primary", v.busy).Label("Save companion profile").OnClick(p.saveWorkspaceCompanion)
	})
}
func (p *NativePage) buildWorkspaceCompanionSkills(c *ui.Context) {
	v := &p.companions
	ui.Text(c, "这个伙伴的 Skills").FontSize(workspaceFontSubtitle)
	for i, ref := range v.profile.Skills {
		i, ref := i, ref
		ui.Row(c).FillWidth().Gap(8).Children(func() {
			ui.Text(c, ref.Name+" · 固定 "+ref.SHA256[:12]).Grow(1).Wrap()
			share := ref.GroupAllowed
			workspaceControlCheckbox(c, "companion-skill-share-"+ref.Name, &share, "允许在群使用").Label("Share companion Skill " + ref.Name).Disabled(v.busy).OnChange(func() { v.profile.Skills[i].GroupAllowed = share })
			workspaceControlButton(c, "companion-skill-remove-"+ref.Name, "移除", "ghost", v.busy).Label("Remove companion Skill " + ref.Name).OnClick(func() { v.profile.Skills = append(v.profile.Skills[:i], v.profile.Skills[i+1:]...) })
		})
	}
	sources := []string{}
	sourceIDs := []string{}
	for _, session := range p.sessions {
		if v.profile.Workspace != "" && session.Workspace == v.profile.Workspace {
			sources = append(sources, session.Title)
			sourceIDs = append(sourceIDs, session.ID)
		}
	}
	sourceLabel := ""
	for i, id := range sourceIDs {
		if id == v.skillSource {
			sourceLabel = sources[i]
		}
	}
	workspaceControlSelect(c, "companion-skill-source", &sourceLabel, sources).Label("Companion Skill source session").FillWidth().Disabled(v.busy).OnChange(func() {
		for i, label := range sources {
			if label == sourceLabel {
				v.skillSource = sourceIDs[i]
				p.loadWorkspaceCompanionSkills()
			}
		}
	})
	if len(sources) == 0 {
		ui.Text(c, "先选择一个现有项目工作区；仅从其中已启用的项目 Skills 明确添加引用。").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Wrap()
	}
	for _, entry := range v.skillCatalog {
		entry := entry
		workspaceControlButton(c, "companion-skill-add-"+entry.ID, "添加 "+entry.Name, "secondary", v.busy || !entry.Available).Label("Add companion Skill " + entry.Name).OnClick(func() { p.pinWorkspaceCompanionSkill(entry.Name) })
	}
}
func (p *NativePage) loadWorkspaceCompanionSkills() {
	v := &p.companions
	source := v.skillSource
	if source == "" || v.busy {
		return
	}
	v.busy = true
	go func() {
		page, err := p.Service.ComposerCatalog(p.ctx, source, product.ComposerCatalogQuery{Kind: product.ComposerSkillEntry, Limit: 100})
		p.dispatch(func() {
			v.busy = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			if v.skillSource == source {
				v.skillCatalog = page.Entries
			}
		})
	}()
}
func (p *NativePage) pinWorkspaceCompanionSkill(name string) {
	v := &p.companions
	if v.busy {
		return
	}
	v.busy = true
	source := v.skillSource
	go func() {
		ref, err := p.Service.PinCompanionSkill(p.ctx, source, name, false)
		p.dispatch(func() {
			v.busy = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			for _, old := range v.profile.Skills {
				if old.Name == ref.Name {
					return
				}
			}
			v.profile.Skills = append(v.profile.Skills, ref)
		})
	}()
}
func (p *NativePage) buildWorkspaceCompanionGroupEditor(c *ui.Context) {
	v := &p.companions
	ui.Text(c, "群设置").FontSize(workspaceFontHeading)
	ui.Scroll(c).Grow(1).FillWidth().Children(func() {
		ui.Column(c).FillWidth().Gap(12).Children(func() {
			ui.Text(c, "群名称")
			workspaceControlInput(c, "companion-group-name", &v.groupDraft.Name).Label("Companion group name").FillWidth().Disabled(v.busy)
			ui.Text(c, "选择伙伴 · 按名册顺序发言")
			for _, profile := range v.profiles {
				profile := profile
				selected := v.members[profile.ID]
				disabled := v.busy || !profile.Enabled && !selected
				workspaceControlCheckbox(c, "group-member-"+profile.ID, &selected, profile.Name).Label("Group member " + profile.Name).Disabled(disabled).OnChange(func() { v.members[profile.ID] = selected })
			}
			mode := "仅 @ 的伙伴"
			if v.groupDraft.ReplyMode == "all" {
				mode = "所有成员"
			}
			workspaceControlSelect(c, "group-reply-mode", &mode, []string{"仅 @ 的伙伴", "所有成员"}).Label("Group reply policy").Disabled(v.busy).OnChange(func() {
				v.groupDraft.ReplyMode = "mentions"
				if mode == "所有成员" {
					v.groupDraft.ReplyMode = "all"
				}
			})
			limit := fmt.Sprint(v.groupDraft.ReplyLimit)
			workspaceControlSelect(c, "group-reply-limit", &limit, []string{"1", "2", "3", "4", "8", "16"}).Label("Group reply limit").Disabled(v.busy).OnChange(func() { fmt.Sscan(limit, &v.groupDraft.ReplyLimit) })
			ui.Text(c, "共享工作区")
			workspaceControlInput(c, "group-workspace", &v.groupDraft.Workspace).Label("Group workspace").Placeholder("留空创建本群独立私有工作区").FillWidth().Disabled(v.busy)
			ui.Text(c, "群仅使用本群消息与明确允许在此工作区使用的 Skills；不带入私聊或私人记忆。更换成员/工作区后开启独立记录范围，旧范围仍可查看。").TextColor(c.Theme().TextMuted).Wrap()
		})
	})
	ui.Row(c).Gap(8).Children(func() {
		workspaceControlButton(c, "group-cancel", "取消", "secondary", v.busy).Label("Cancel companion group").OnClick(p.cancelWorkspaceCompanionGroup)
		ui.Spacer(c)
		workspaceControlButton(c, "group-save", "保存群聊", "primary", v.busy).Label("Save companion group").OnClick(p.saveWorkspaceCompanionGroup)
	})
}
func (p *NativePage) buildWorkspaceCompanionChat(c *ui.Context) {
	v := &p.companions
	id := p.selected
	executionID, _ := p.Service.Turns.Active(id)
	if v.profile.Deleted {
		if v.conversation.SessionID == "" {
			ui.Text(c, "此伙伴没有已保存的私聊记录")
		} else if p.selected == v.conversation.SessionID {
			p.chat.BuildMessages(c)
			if p.workspaceComposerRunning() {
				workspaceControlButton(c, "companion-stop-"+id+"-"+executionID, "停止已有执行", "danger", executionID == "" || p.modelStopping()).Label("Stop companion execution").OnClick(func() { p.stopExecution(id, executionID) })
			}
		} else {
			workspaceControlButton(c, "companion-observe-private", "查看私聊记录", "secondary").OnClick(func() { p.selectSession(v.conversation.SessionID); p.pane = "companions" })
		}
		return
	}
	p.buildWorkspaceCompanionApproval(c, v.profile.Target)
	if v.conversation.SessionID == "" {
		ui.Text(c, v.profile.Role).Wrap()
		workspaceControlButton(c, "companion-open-chat", "打开私聊", "primary", v.busy || !v.profile.Enabled || !v.approved).Label("Open companion private conversation").OnClick(p.openWorkspaceCompanionChat)
		ui.Spacer(c)
		return
	}
	if p.selected == v.conversation.SessionID {
		p.buildApprovals(c)
		p.buildQuestions(c)
		p.chat.BuildMessages(c)
	} else {
		ui.Text(c, "返回此伙伴的独立会话以查看历史")
		workspaceControlButton(c, "companion-observe-private", "查看私聊记录", "secondary").OnClick(func() { p.selectSession(v.conversation.SessionID); p.pane = "companions" })
	}
	workspaceControlTextArea(c, "companion-prompt", &v.prompt).Label("Companion message").Height(96).FillWidth().Disabled(v.busy)
	ui.Row(c).Gap(8).Children(func() {
		ui.Text(c, "长期私聊与群聊记录相互隔离").FontSize(11).TextColor(c.Theme().TextMuted).Grow(1)
		if p.selected == v.conversation.SessionID && p.workspaceComposerRunning() {
			workspaceControlButton(c, "companion-stop-"+id+"-"+executionID, "停止", "danger", executionID == "" || p.modelStopping()).Label("Stop companion execution").OnClick(func() { p.stopExecution(id, executionID) })
		} else {
			workspaceControlButton(c, "companion-send", "发送", "primary", v.busy || !v.approved || !v.profile.Enabled || strings.TrimSpace(v.prompt) == "").Label("Send companion message").OnClick(p.sendWorkspaceCompanion)
		}
	})
}
func (p *NativePage) buildWorkspaceCompanionApproval(c *ui.Context, target hub.Target) {
	v := &p.companions
	ui.Text(c, workspaceTargetLabel(target)+" · "+target.AccountSource+" · "+target.BillingRoute).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Wrap()
	workspaceControlCheckbox(c, "companion-route-approved", &v.approved, "确认本次所选账号与计费路由").Label("Approve companion account and billing route").Disabled(v.busy || v.running).OnChange(func() {
		v.approvedProfiles = nil
		if v.approved {
			v.approvedProfiles = []hub.CompanionProfile{v.profile}
		}
	})
}
func (p *NativePage) buildWorkspaceCompanionGroupChat(c *ui.Context) {
	v := &p.companions
	running := p.workspaceCompanionGroupRunning()
	ui.Row(c).FillWidth().Gap(8).Children(func() {
		ui.Text(c, "仅用户明确发送开启本轮；成员发言不会自动开启下一轮").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Grow(1).Wrap()
		workspaceIconAction(c, "group-refresh", "Refresh companion group messages", "refresh-cw").Disabled(v.busy).OnClick(p.loadWorkspaceCompanionGroup)
		if !v.group.Deleted {
			workspaceControlButton(c, "group-stop", "停止本轮", "danger", !running || v.stopping).Label("Stop companion group round").OnClick(p.stopWorkspaceCompanionGroup)
		}
	})
	if v.group.ScopeRevision > 1 {
		scopes := []string{}
		current := fmt.Sprint(v.scope)
		for i := uint64(1); i <= v.group.ScopeRevision; i++ {
			scopes = append(scopes, fmt.Sprint(i))
		}
		workspaceControlSelect(c, "group-scope", &current, scopes).Label("Group history scope").Disabled(v.busy || running).OnChange(func() {
			var scope uint64
			fmt.Sscan(current, &scope)
			p.selectWorkspaceCompanionGroupScope(scope)
		})
	}
	if v.historyWarning != "" {
		ui.Text(c, v.historyWarning).Label("Companion group pagination unavailable").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Wrap()
	}
	if v.memberHistory != "" {
		ui.Row(c).FillWidth().Gap(8).Children(func() {
			ui.Text(c, "成员原始记录").FontSize(workspaceFontSubtitle).Grow(1)
			workspaceControlButton(c, "group-member-back", "返回群记录", "secondary").Label("Back to companion group messages").OnClick(func() { v.memberHistory = "" })
		})
		if p.selected == v.memberHistory {
			if !v.group.Deleted {
				p.buildApprovals(c)
				p.buildQuestions(c)
			}
			p.chat.BuildMessages(c)
		} else {
			ui.Text(c, "正在读取成员原始记录…")
		}
		return
	}
	if v.moreMessages || v.moreRounds {
		ui.Row(c).FillWidth().Gap(8).Children(func() {
			if v.moreMessages {
				workspaceControlButton(c, "group-older-messages", "加载更早消息", "secondary", v.busy || running).Label("Load older companion messages").OnClick(func() { p.loadOlderWorkspaceCompanionHistory(false) })
			}
			if v.moreRounds {
				workspaceControlButton(c, "group-older-rounds", "加载更早回合", "secondary", v.busy || running).Label("Load older companion rounds").OnClick(func() { p.loadOlderWorkspaceCompanionHistory(true) })
			}
		})
	}
	ui.Scroll(c).Label("Companion group conversation").Grow(1).MinHeight(0).FillWidth().Children(func() {
		ui.Column(c).FillWidth().Gap(12).Children(func() {
			if len(v.messages) == 0 && v.problem == "" {
				ui.Text(c, "本群还没有消息").TextColor(c.Theme().TextMuted)
			}
			for _, m := range v.messages {
				m := m
				ui.Column(c.Key(m.ID)).FillWidth().Gap(6).Padding(12, 0).Children(func() {
					ui.Text(c, m.Author).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
					ui.Text(c, m.Text).FontSize(15).Wrap()
				})
			}
			for _, r := range v.rounds {
				r := r
				ui.Text(c, "本轮状态："+r.Status).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
				if r.Error != "" {
					ui.Text(c, r.Error).TextColor(c.Theme().Danger).Wrap()
				}
				if !v.group.Deleted {
					p.buildWorkspaceCompanionRoundRecovery(c, r)
				}
				for _, member := range r.Members {
					member := member
					workspaceControlButton(c, "group-observe-"+r.ID+member.CompanionID, companionMemberHistoryLabel(member.Profile.Name, v.group.Deleted), "secondary").Label("Observe group member " + member.SessionID).OnClick(func() { v.memberHistory = member.SessionID; p.selectSession(member.SessionID); p.pane = "companions" })
				}
			}
			if p.selected != "" && !v.group.Deleted {
				for _, r := range v.rounds {
					for _, member := range r.Members {
						if member.SessionID == p.selected {
							p.buildApprovals(c)
							p.buildQuestions(c)
						}
					}
				}
			}
		})
	})
	if v.group.Deleted {
		return
	}
	if v.scope != v.group.ScopeRevision {
		ui.Text(c, "正在查看旧成员范围；新消息仅发往当前范围。").TextColor(c.Theme().TextMuted)
		return
	}
	if v.group.ReplyMode == "mentions" {
		ui.Scroll(c.Key("group-mentions-scroll")).Label("Group mention list").FillWidth().Height(112).Shrink(0).Children(func() {
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				for _, id := range v.group.Members {
					id := id
					name := id
					for _, profile := range v.profiles {
						if profile.ID == id {
							name = profile.Name
						}
					}
					selected := v.mentions[id]
					workspaceControlCheckbox(c, "group-mention-"+id, &selected, "@"+name).Label("Mention companion " + name).Tooltip(name).FillWidth().Disabled(running || v.busy).OnChange(func() { v.mentions[id] = selected })
				}
			})
		})
	}
	ui.Scroll(c.Key("group-routes-scroll")).Label("Group billing route list").FillWidth().Height(48).Shrink(0).Children(func() {
		ui.Column(c).FillWidth().Gap(4).Children(func() {
			for _, id := range v.group.Members {
				for _, profile := range v.profiles {
					if profile.ID == id {
						ui.Text(c, profile.Name+" · "+workspaceTargetLabel(profile.Target)+" · "+profile.Target.AccountSource+" · "+profile.Target.BillingRoute).FontSize(11).TextColor(c.Theme().TextMuted).Wrap()
					}
				}
			}
		})
	})

	workspaceControlCheckbox(c, "group-route-approved", &v.approved, "确认本轮所有参与伙伴的账号与计费路由").Label("Approve group accounts and billing routes").Disabled(running || v.busy).OnChange(func() {
		v.approvedProfiles = nil
		if v.approved {
			for _, id := range v.group.Members {
				for _, profile := range v.profiles {
					if profile.ID == id {
						v.approvedProfiles = append(v.approvedProfiles, profile)
					}
				}
			}
		}
	})
	workspaceControlTextArea(c, "group-prompt", &v.prompt).Label("Companion group message").Height(80).FillWidth().Disabled(running || v.busy)
	workspaceControlButton(c, "group-send", "发送", "primary", running || v.busy || !v.group.Enabled || !v.approved || strings.TrimSpace(v.prompt) == "").Label("Send companion group round").OnClick(p.runWorkspaceCompanionGroup)
}

// pollWorkspaceCompanions uses the existing observer clock; Build remains a
// cold projection and this page does not own a timer or execution cancellation.
func (p *NativePage) pollWorkspaceCompanions() {
	if p.pane == "companions" && p.companions.kind == "group" && !p.companions.busy && p.workspaceCompanionGroupRunning() {
		p.loadWorkspaceCompanionGroup()
	}
}

func (p *NativePage) loadWorkspaceCompanionPrivate(id string) {
	v := &p.companions
	v.busy = true
	epoch, generation := v.groupEpoch, p.generation
	go func() {
		conversation, err := p.Service.Companions.Conversation(p.ctx, hub.LocalCompanionOwner, id, "", 0)
		p.dispatch(func() {
			v.busy = false
			if v.profile.ID != id || v.kind != "profile" || v.groupEpoch != epoch {
				if v.groupReloadPending {
					p.loadWorkspaceCompanionGroup()
				}
				return
			}
			if err != nil {
				if !errors.Is(err, hub.ErrNotFound) {
					v.problem = err.Error()
				}
				return
			}
			v.conversation = conversation
			if p.pane != "companions" || p.generation != generation {
				return
			}
			p.selectSession(conversation.SessionID)
			p.pane = "companions"
		})
	}()
}
func (p *NativePage) cancelWorkspaceCompanionProfile() {
	v := &p.companions
	for _, saved := range v.profiles {
		if saved.ID == v.profile.ID {
			v.profile = saved
			v.profile.Skills = append([]hub.CompanionSkill(nil), saved.Skills...)
			break
		}
	}
	v.editor = ""
	v.problem = ""
}

func companionRoundUnresolved(status string) bool {
	return status == "running" || status == "waiting" || status == "interrupted" || status == "unknown"
}
func (p *NativePage) buildWorkspaceCompanionRoundRecovery(c *ui.Context, r hub.CompanionRound) {
	if !companionRoundUnresolved(r.Status) {
		return
	}
	v := &p.companions
	review := v.roundReviews[r.ID]
	ui.Text(c, fmt.Sprintf("已投递 %d / %d 位伙伴；仅继续未完成部分，不重放已完成执行。", r.NextMember, len(r.Members))).FontSize(workspaceFontCaption).Wrap()
	for _, member := range r.Members {
		ui.Text(c, member.Profile.Name+" · 固定 "+workspaceTargetLabel(member.Profile.Target)+" · "+member.Profile.Target.AccountSource+" · "+member.Profile.Target.BillingRoute).FontSize(11).TextColor(c.Theme().TextMuted).Wrap()
	}
	if review.Wait != nil && review.Wait.Status == engine.Waiting {
		ui.Text(c, "此轮等待原伙伴检查点；先保存真实问题/审批结果，再明确继续。").Wrap()
	}
	if r.NextMember < len(r.Members) {
		member := r.Members[r.NextMember]
		workspaceControlButton(c, "group-checkpoint-"+r.ID, "审阅原伙伴的等待 / 放弃", "secondary", v.running || v.busy).Label("Review original group checkpoint " + r.ID).OnClick(func() { p.selectSession(member.SessionID); p.setPane("waits") })
	}
	if v.roundApprovals == nil {
		v.roundApprovals = make(map[string]uint64)
	}
	approved := v.roundApprovals[r.ID] == r.Revision
	workspaceControlCheckbox(c, "group-continue-approval-"+r.ID, &approved, "确认继续此轮固定伙伴、账号与计费路由").Label("Approve saved group round " + r.ID).Disabled(v.running || v.busy).OnChange(func() {
		if approved {
			v.roundApprovals[r.ID] = r.Revision
		} else {
			delete(v.roundApprovals, r.ID)
		}
	})
	ui.Row(c).Gap(8).Children(func() {
		workspaceControlButton(c, "group-continue-"+r.ID, "继续本群回合", "primary", v.running || v.busy || !approved).Label("Continue saved group round " + r.ID).OnClick(func() { p.continueWorkspaceCompanionRound(r, review) })
		workspaceControlButton(c, "group-terminate-review-"+r.ID, "终止回合…", "secondary", v.running || v.busy).Label("Review group round termination " + r.ID).OnClick(func() { v.terminationRound = r.ID })
	})
	if v.terminationRound == r.ID {
		ui.Text(c, "终止不会撤销既有副作用；未知结果保留且不重放。若仍有原检查点，先在等待面板审阅并放弃，再确认终止。").Wrap()
		workspaceControlButton(c, "group-terminate-"+r.ID, "确认终止本轮", "danger", v.running || v.busy).Label("Confirm group round termination " + r.ID).OnClick(func() { p.terminateWorkspaceCompanionRound(r) })
	}
}
func (p *NativePage) continueWorkspaceCompanionRound(r hub.CompanionRound, review product.CompanionRoundReview) {
	v := &p.companions
	if v.running || v.busy {
		return
	}
	var target *product.WaitTarget
	if review.Wait != nil && review.Wait.Status == engine.Waiting {
		target = &product.WaitTarget{BranchID: review.Wait.BranchID, TurnID: review.Wait.TurnID, Revision: review.Wait.Snapshot.Revision}
	}
	v.running = true
	go func() {
		_, err := p.Service.ContinueCompanionGroup(context.Background(), hub.LocalCompanionOwner, r.ID, r.Revision, target, true)
		p.dispatch(func() {
			v.running = false
			if err != nil {
				v.problem = err.Error()
			}
			p.loadWorkspaceCompanionGroup()
		})
	}()
}
func (p *NativePage) terminateWorkspaceCompanionRound(r hub.CompanionRound) {
	v := &p.companions
	if v.busy || v.running {
		return
	}
	v.busy = true
	go func() {
		_, err := p.Service.TerminateCompanionGroup(p.ctx, hub.LocalCompanionOwner, r.ID, r.Revision, true)
		p.dispatch(func() {
			v.busy = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.terminationRound = ""
			p.loadWorkspaceCompanionGroup()
		})
	}()
}

// Older pages are queried from storage; appending a page never claims a Turn.
func (p *NativePage) loadOlderWorkspaceCompanionHistory(rounds bool) {
	v := &p.companions
	if v.busy || v.running || v.group.ID == "" {
		return
	}
	before, more := v.messageBefore, v.moreMessages
	if rounds {
		before, more = v.roundBefore, v.moreRounds
	}
	if !more || before == "" {
		return
	}
	id, scope, epoch := v.group.ID, v.scope, v.groupEpoch
	v.busy = true
	go func() {
		var messages hub.CompanionMessagePage
		var page hub.CompanionRoundPage
		var err error
		if rounds {
			page, err = p.Service.Companions.RoundPage(p.ctx, hub.LocalCompanionOwner, id, scope, hub.CompanionHistoryQuery{Before: before, Limit: 32})
		} else {
			messages, err = p.Service.Companions.MessagePage(p.ctx, hub.LocalCompanionOwner, id, scope, hub.CompanionHistoryQuery{Before: before, Limit: 256})
		}
		p.dispatch(func() {
			v.busy = false
			if v.groupReloadPending {
				p.loadWorkspaceCompanionGroup()
				return
			}
			if v.group.ID != id || v.scope != scope || v.groupEpoch != epoch {
				return
			}
			if err != nil {
				v.problem = err.Error()
				return
			}
			if rounds {
				v.rounds = append(v.rounds, page.Rounds...)
				v.roundBefore, v.moreRounds = page.NextBefore, page.HasMore
			} else {
				v.messages = append(messages.Messages, v.messages...)
				v.messageBefore, v.moreMessages = messages.NextBefore, messages.HasMore
			}
			v.problem = ""
		})
	}()
}

func companionMemberHistoryLabel(name string, archived bool) string {
	if archived {
		return "查看 " + name + " 的历史"
	}
	return "查看 " + name + " 的执行 / 审批"
}
