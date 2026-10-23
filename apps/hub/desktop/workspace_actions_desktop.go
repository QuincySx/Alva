// INPUT: Page-local task draft, actual configured target and existing HubService lifecycle.
// OUTPUT: Cold new-task navigation and explicit first-send creation followed by normal shared submission.
// POS: [L7 Hub UI] Retains committed session identity and unsent input on partial admission failure.
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
	if p.selected != "" {
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
	p.busy, p.chat.Busy = true, true
	go func() {
		session, err := p.Service.Create(p.ctx, title, workspace)
		if err == nil && hub.RouteKey(target) != hub.RouteKey(p.Service.DefaultTarget) {
			_, err = p.Service.Store.Switch(p.ctx, session.ID, target, "Explicit new-task model selection")
		}
		p.dispatch(func() {
			p.busy, p.chat.Busy = false, false
			if session.ID != "" {
				// Create is durable even if the subsequent explicit route fails.
				p.drafts[session.ID] = prompt
				p.selectSession(session.ID)
				p.model.Draft = prompt
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
		})
	}()
}
