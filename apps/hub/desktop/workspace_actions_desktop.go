// INPUT: Page-local task/file drafts, actual configured target and existing HubService lifecycle.
// OUTPUT: Cold navigation and explicit Create/Switch/Upload/Detail/Submit with retryable partial imports.
// POS: [L7 Hub UI] Retains committed identity, prompt snapshot and saved references across admission failures.
//go:build desktop

package hubdesktop

import (
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
)

func (p *NativePage) startWorkspaceTask() {
	if p.busy {
		return
	}
	p.selectSession("")
	p.detail = hub.Detail{}
	p.chat.Busy = false
	p.model.ActiveRunID = ""
	p.model.Status = view.Idle
	p.model.Notice = ""
	p.composer.addOpen, p.composer.permissionOpen, p.composer.targetOpen = false, false, false
}

func (p *NativePage) sendWorkspaceDraft() {
	if p.selected == "" && p.Service.ComposerDrafts != nil {
		if p.busy || !p.model.CanSend() || p.workspaceAttachmentProblem() != "" || p.workspaceTargetProblem() != "" {
			return
		}
		p.prepareWorkspaceDraftSubmission(func(draft workspaceDraftSubmission) { p.sendWorkspaceDraftReady(draft) })
		return
	}
	p.sendWorkspaceDraftReady(workspaceDraftSubmission{})
}

func (p *NativePage) sendWorkspaceDraftReady(firstDraft workspaceDraftSubmission) {
	if p.voice.recorder != nil {
		p.voice.problem = "请先结束录音并确认文字，再发送。"
		return
	}
	if problem := p.workspaceTargetProblem(); problem != "" {
		p.problem = problem
		return
	}
	if problem := p.workspaceAttachmentProblem(); problem != "" {
		p.problem = problem
		return
	}
	if p.selected != "" {
		if p.workspacePendingFiles() {
			p.sendWorkspacePendingFiles()
			return
		}
		p.send()
		return
	}
	if p.busy || !p.model.CanSend() {
		return
	}
	prompt, err := p.model.Prompt()
	if err != nil {
		p.problem = err.Error()
		return
	}
	target := p.workspaceComposerTarget()
	title := strings.SplitN(strings.TrimSpace(prompt), "\n", 2)[0]
	if utf8.RuneCountInString(title) > 32 {
		title = string([]rune(title)[:32]) + "…"
	}
	selections := p.captureWorkspaceSelections()
	references := p.captureWorkspaceReferences()
	homePlan := p.workspacePlanEnabled()
	homePolicy := p.workspacePermissionPolicy()
	workspace := p.workspace
	files := append([]workspaceSelectedFile(nil), p.workspaceFiles[""]...)
	selectedFiles := p.captureWorkspaceComposerFiles()
	var capturedDraft hub.ComposerDraft
	if firstDraft.valid {
		capturedDraft = hub.CloneComposerDraft(p.workspaceDraftEntry(firstDraft.scope).committed)
	}
	finishReferences := p.referenceOwner.begin()
	if finishReferences == nil {
		return
	}
	p.busy, p.chat.Busy = true, true
	go func() {
		// Home preparations are cold-read again before the first durable Create.
		// In particular, a project change must not leave a session that can never
		// accept its Goal, and an external route cannot accept a Plan intent.
		err := validateWorkspaceHomeSelections(p.ctx, p.Service, workspace, target, selections)
		var session hub.Session
		var boundDraft hub.ComposerDraft
		if err == nil {
			if firstDraft.valid {
				session, boundDraft, err = p.Service.CreateComposerSession(p.ctx, capturedDraft, title, workspace)
			} else {
				session, err = p.Service.Create(p.ctx, title, workspace)
			}
		}
		if err == nil && hub.RouteKey(target) != hub.RouteKey(p.Service.DefaultTarget) {
			_, err = p.Service.SwitchTarget(p.ctx, session.ID, target, "Explicit new-task model selection")
		}
		if err == nil && homePlan {
			err = p.Service.SetPlanMode(p.ctx, session.ID, true)
		}
		if err == nil && !homePlan && homePolicy != hub.PermissionAsk {
			err = p.Service.SetPermissionPolicy(p.ctx, session.ID, homePolicy, homePolicy == hub.PermissionFull)
		}
		var imported []product.DraftReferenceSelection
		if err == nil {
			imported, err = p.importWorkspaceFiles(session.ID, files)
		}
		finishReferences()
		p.dispatch(func() {
			p.busy, p.chat.Busy = false, false
			if session.ID != "" {
				if firstDraft.valid {
					p.bindWorkspaceCreatedDraft(session.ID, boundDraft, imported)
				}
				p.retainWorkspaceReferences(session.ID, imported)
				p.bindWorkspaceVoiceHome(session.ID)
				// Create is durable even if the subsequent explicit route fails.
				// A new edit made while Create/import was in flight belongs to
				// this session too; never replace it with the captured prompt.
				currentDraft := p.model.Draft
				p.drafts[session.ID] = currentDraft
				// selectSession flushes the source home scope. Keep its selected
				// files/commands intact until that flush captures the same version;
				// moving them first would look like a new home edit and prevent
				// the durable PendingSessionID from being cleared after the copy.
				p.selectSession(session.ID)
				if p.composerSelections == nil {
					p.composerSelections = make(map[string][]product.ComposerPreparation)
				}
				p.composerSelections[session.ID] = p.composerSelections[""]
				delete(p.composerSelections, "")
				if p.composerFiles == nil {
					p.composerFiles = make(map[string][]product.ComposerFileSelection)
				}
				p.composerFiles[session.ID] = p.composerFiles[""]
				delete(p.composerFiles, "")
				if len(files) != 0 {
					p.workspaceFiles[session.ID] = files[len(imported):]
					delete(p.workspaceFiles, "")
				}
				delete(p.permissionPolicies, "")
				p.homePermissionScope = ""
				delete(p.drafts, "")
				p.refreshWorkspaceNavigation()
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			if firstDraft.valid {
				scope := hub.ComposerDraftScope{Kind: "session", ID: session.ID}
				entry := p.workspaceDraftEntry(scope)
				if entry.conflict {
					p.problem = entry.problem
					return
				}
				p.busy, p.chat.Busy = true, true
				copyVersion := entry.version
				p.queueWorkspaceDraftSave(scope, entry.local, copyVersion, func(saved hub.ComposerDraft, saveErr error) {
					if saveErr != nil {
						p.busy, p.chat.Busy = false, false
						p.problem = "会话已创建，未发送输入已保留：" + saveErr.Error()
						return
					}
					token := workspaceDraftSubmission{valid: true, scope: scope, version: firstDraft.version}
					if firstDraft.version == copyVersion {
						token.revision = saved.Revision
						token.homeRevision = p.workspaceLinkedHomeRevision(session.ID, saved)
					}
					p.completeWorkspaceDraftTransfer(saved, copyVersion, func(moveErr error) {
						if moveErr != nil {
							p.busy, p.chat.Busy = false, false
							p.problem = "会话草稿已保存，首页迁移尚未完成；未自动发送：" + moveErr.Error()
							return
						}
						p.scheduleWorkspaceFirstSend(session.ID, prompt, selections, append(references, imported...), selectedFiles, token)
					})
				})
				return
			}
			// Initial Detail must commit to the UI before admission; an older
			// load must never replace the just-admitted Running observation.
			p.pendingFirstSend = session.ID
			p.pendingFirstPrompt = prompt
			p.pendingFirstSelections = selections
			p.pendingFirstReferences = append(references, imported...)
			p.pendingFirstFiles = selectedFiles
		})
	}()
}

func (p *NativePage) workspaceTargetOptions(target hub.Target) hub.ModelOptions {
	var options hub.ModelOptions
	if p.Service.Connections != nil {
		options = p.Service.Connections.ModelPreference(target).Options
	}
	if p.selected != "" && p.workspaceSessionModelOptionsProblem() == "" {
		if override, ok := p.sessionModelOptions.value.Options(target); ok {
			options = override
		}
	}
	return options
}

func (p *NativePage) workspaceTargetProblem() string {
	if problem := p.workspaceSessionModelOptionsProblem(); problem != "" {
		return problem
	}
	target := p.workspaceComposerTarget()
	if problem := p.workspaceGoalTargetProblem(target); problem != "" {
		return problem
	}
	if p.workspaceTargetOptions(target).ValidateTarget(target) != nil {
		return "这个 Agent 不支持已保存的模型参数，请在参数设置中调整后再发送。草稿已保留。"
	}
	if p.Service.ValidateTarget(target) == nil {
		return ""
	}
	if product.IsConnectionTarget(target) {
		return "模型连接已更改或停用，请重新选择模型。草稿已保留。"
	}
	return "模型路由已更改或停用，请重新选择模型。草稿已保留。"
}

func (p *NativePage) workspaceGoalTargetProblem(target hub.Target) string {
	if target.EngineID != "alva" && p.workspacePlanEnabled() {
		return "计划模式需要 Alva Agent。请切回 Alva，或关闭计划模式后再发送。草稿已保留。"
	}
	for _, selection := range p.composerSelections[p.selected] {
		if target.EngineID != "alva" && selection.Intent == product.ComposerIntentPlan {
			return "计划命令需要 Alva Agent。请切回 Alva，或移除计划引用后再发送。草稿已保留。"
		}
	}
	if target.EngineID != "alva" {
		for _, selection := range p.composerSelections[p.selected] {
			if selection.Kind == product.ComposerGoalEntry {
				return "目标引用需要 Alva Agent。请切回 Alva，或移除目标引用后再选择其他 Agent。"
			}
		}
	}
	return ""
}

func (p *NativePage) sendWorkspacePendingFiles() {
	if p.busy || p.chat.Busy || p.detail.Session.ID != p.selected || p.inputBlocked() || !p.model.CanSend() {
		return
	}
	prompt, err := p.model.Prompt()
	if err != nil {
		p.problem = err.Error()
		return
	}
	selections := p.captureWorkspaceSelections()
	references := p.captureWorkspaceReferences()
	id := p.selected
	files := append([]workspaceSelectedFile(nil), p.workspaceFiles[id]...)
	finishReferences := p.referenceOwner.begin()
	if finishReferences == nil {
		return
	}
	// An earlier observation must not overwrite the native binding established
	// by Upload. Reload the actual branch before submitting the captured prompt.
	p.generation++
	p.busy, p.chat.Busy = true, true
	go func() {
		imported, err := p.importWorkspaceFiles(id, files)
		finishReferences()
		p.dispatch(func() {
			p.retainWorkspaceReferences(id, imported)
			p.busy, p.chat.Busy = false, false
			p.workspaceFiles[id] = files[len(imported):]
			if err != nil {
				p.problem = err.Error()
			} else {
				p.pendingFirstSend, p.pendingFirstPrompt = id, prompt
				p.pendingFirstSelections = selections
				p.pendingFirstReferences = append(references, imported...)
			}
			// A partial import may have committed the branch's first native ID.
			// Keep its successful references and refresh even when a later file fails.
			p.loadTail()
		})
	}()
}
