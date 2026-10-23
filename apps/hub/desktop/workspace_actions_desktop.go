// INPUT: Page-local task/file drafts, actual configured target and existing HubService lifecycle.
// OUTPUT: Cold navigation and explicit Create/Switch/Upload/Detail/Submit with retryable partial imports.
// POS: [L7 Hub UI] Retains committed identity, prompt snapshot and saved references across admission failures.
//go:build desktop

package hubdesktop

import (
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
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
	workspace := p.workspace
	files := append([]workspaceSelectedFile(nil), p.workspaceFiles[""]...)
	p.busy, p.chat.Busy = true, true
	go func() {
		session, err := p.Service.Create(p.ctx, title, workspace)
		if err == nil && hub.RouteKey(target) != hub.RouteKey(p.Service.DefaultTarget) {
			_, err = p.Service.Store.Switch(p.ctx, session.ID, target, "Explicit new-task model selection")
		}
		imported := 0
		if err == nil {
			imported, err = p.importWorkspaceFiles(session.ID, files)
		}
		p.dispatch(func() {
			p.busy, p.chat.Busy = false, false
			if session.ID != "" {
				// Create is durable even if the subsequent explicit route fails.
				// A new edit made while Create/import was in flight belongs to
				// this session too; never replace it with the captured prompt.
				currentDraft := p.model.Draft
				p.drafts[session.ID] = currentDraft
				if len(files) != 0 {
					p.workspaceFiles[session.ID] = files[imported:]
					delete(p.workspaceFiles, "")
				}
				p.selectSession(session.ID)
				delete(p.drafts, "")
				p.refreshSessions()
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			// Initial Detail must commit to the UI before admission; an older
			// load must never replace the just-admitted Running observation.
			p.pendingFirstSend = session.ID
			p.pendingFirstPrompt = prompt
		})
	}()
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
	id := p.selected
	files := append([]workspaceSelectedFile(nil), p.workspaceFiles[id]...)
	// An earlier observation must not overwrite the native binding established
	// by Upload. Reload the actual branch before submitting the captured prompt.
	p.generation++
	p.busy, p.chat.Busy = true, true
	go func() {
		imported, err := p.importWorkspaceFiles(id, files)
		p.dispatch(func() {
			p.busy, p.chat.Busy = false, false
			p.workspaceFiles[id] = files[imported:]
			if err != nil {
				p.problem = err.Error()
			} else {
				p.pendingFirstSend, p.pendingFirstPrompt = id, prompt
			}
			// A partial import may have committed the branch's first native ID.
			// Keep its successful references and refresh even when a later file fails.
			p.loadTail()
		})
	}()
}
