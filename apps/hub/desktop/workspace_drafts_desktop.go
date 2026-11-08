// INPUT: Shared durable draft owner, UI edit events and logical session/home navigation.
// OUTPUT: Debounced CAS saves, cross-window restore, version-bound submission and close flush guards.
// POS: [L7 Hub UI] Keeps manual input until its captured revision is actually admitted; Build never performs storage I/O.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"errors"
	"reflect"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type workspaceDraftSubmission struct {
	valid        bool
	scope        hub.ComposerDraftScope
	revision     uint64
	version      uint64
	homeRevision uint64
}

type workspaceDraftSave struct {
	value   hub.ComposerDraft
	version uint64
	after   func(hub.ComposerDraft, error)
}

type workspaceDraftEntry struct {
	committed, local hub.ComposerDraft
	version          uint64
	known, loading   bool
	dirty, saving    bool
	conflict         bool
	problem          string
	edited           time.Time
	queue            []workspaceDraftSave
}

type workspaceDraftsState struct {
	entries map[hub.ComposerDraftScope]*workspaceDraftEntry
}

func (p *NativePage) workspaceComposerDraftScope() hub.ComposerDraftScope {
	if p.selected != "" {
		return hub.ComposerDraftScope{Kind: "session", ID: p.selected}
	}
	return hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}
}

func (p *NativePage) workspaceDraftEntry(scope hub.ComposerDraftScope) *workspaceDraftEntry {
	if p.durableDrafts.entries == nil {
		p.durableDrafts.entries = make(map[hub.ComposerDraftScope]*workspaceDraftEntry)
	}
	e := p.durableDrafts.entries[scope]
	if e == nil {
		e = &workspaceDraftEntry{committed: hub.ComposerDraft{Scope: scope}, local: hub.ComposerDraft{Scope: scope}}
		p.durableDrafts.entries[scope] = e
	}
	return e
}

func sameWorkspaceDraftContent(a, b hub.ComposerDraft) bool {
	a.Revision, b.Revision = 0, 0
	return reflect.DeepEqual(hub.CloneComposerDraft(a), hub.CloneComposerDraft(b))
}

func (p *NativePage) captureWorkspaceDurableDraft() (hub.ComposerDraft, error) {
	scope := p.workspaceComposerDraftScope()
	commands, err := product.EncodeComposerDraftPreparations(p.captureWorkspaceSelections())
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	value := hub.ComposerDraft{Scope: scope, Text: p.model.Draft, PreparedCommands: commands}
	if e := p.durableDrafts.entries[scope]; e != nil {
		value.PendingSessionID = e.committed.PendingSessionID
	}
	for _, ref := range p.captureWorkspaceReferences() {
		value.References = append(value.References, hub.ComposerDraftReference{Kind: ref.Kind, ID: ref.ID, SHA256: ref.SHA256})
	}
	for _, file := range p.captureWorkspaceComposerFiles() {
		value.References = append(value.References, file.DraftSelection())
	}
	return value, value.Validate()
}

func (p *NativePage) markWorkspaceDraftEdited() {
	if p.Service.ComposerDrafts == nil {
		return
	}
	e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
	value, err := p.captureWorkspaceDurableDraft()
	if err != nil {
		e.problem = err.Error()
		return
	}
	e.version++ // Every edit event matters, even if text later returns to its old value.
	e.local, e.dirty, e.edited = value, true, time.Now()
}

func (p *NativePage) loadWorkspaceDraft() {
	if p.Service.ComposerDrafts == nil {
		return
	}
	scope := p.workspaceComposerDraftScope()
	e := p.workspaceDraftEntry(scope)
	if e.known || e.loading {
		if e.known {
			p.applyWorkspaceLocalDraft(scope, e.local, nil, nil)
		}
		return
	}
	if local, err := p.captureWorkspaceDurableDraft(); err == nil && !sameWorkspaceDraftContent(local, hub.ComposerDraft{Scope: scope}) {
		e.local, e.dirty, e.version = local, true, e.version+1
	}
	p.readWorkspaceDraft(scope, false)
}

func (p *NativePage) readWorkspaceDraft(scope hub.ComposerDraftScope, replace bool) {
	e := p.workspaceDraftEntry(scope)
	if e.loading || e.saving {
		return
	}
	e.loading = true
	version := e.version
	ctx := p.ctx
	go func() {
		value, err := p.Service.ComposerDrafts.Read(ctx, scope)
		var files []product.ComposerFileSelection
		var records map[string]hub.ComposerFile
		if err == nil {
			files, records, err = p.Service.RestoreComposerFileSelections(ctx, value)
		}
		if err == nil && value.PendingSessionID != "" {
			_, err = p.Service.Conversations.Detail(ctx, value.PendingSessionID)
		}
		p.dispatch(func() {
			e.loading = false
			if err != nil {
				e.problem = "草稿读取失败，当前输入已保留：" + err.Error()
				queued := e.queue
				e.queue = nil
				for _, request := range queued {
					if request.after != nil {
						request.after(hub.ComposerDraft{}, err)
					}
				}
				return
			}
			if scope == p.workspaceComposerDraftScope() {
				if current, captureErr := p.captureWorkspaceDurableDraft(); captureErr == nil && !sameWorkspaceDraftContent(current, e.local) {
					p.markWorkspaceDraftEdited()
				}
			}
			e.known, e.committed = true, hub.CloneComposerDraft(value)
			if e.version == version && (replace || !e.dirty) {
				e.local, e.dirty, e.conflict, e.problem = hub.CloneComposerDraft(value), false, false, ""
				p.applyWorkspaceLocalDraft(scope, value, files, records)
			} else if value.Revision != 0 && !sameWorkspaceDraftContent(e.local, value) {
				e.conflict = true
				e.problem = "共享草稿已变化，当前输入已保留；请选择保留哪一份。"
				queued := e.queue
				e.queue = nil
				for _, request := range queued {
					if request.after != nil {
						request.after(hub.ComposerDraft{}, hub.ErrComposerDraftConflict)
					}
				}
			}
			if scope == p.workspaceComposerDraftScope() && value.PendingSessionID != "" && !e.dirty && !p.busy {
				p.restoreWorkspacePendingSession(value, files, records)
				return
			}
			p.startWorkspaceDraftSave(scope)
		})
	}()
}

func (p *NativePage) applyWorkspaceLocalDraft(scope hub.ComposerDraftScope, value hub.ComposerDraft, files []product.ComposerFileSelection, records map[string]hub.ComposerFile) {
	id := scope.ID
	if scope.Kind == "home" {
		id = ""
	}
	if p.drafts == nil {
		p.drafts = make(map[string]string)
	}
	p.drafts[id] = value.Text
	commands, err := product.DecodeComposerDraftPreparations(value)
	if err != nil {
		p.workspaceDraftEntry(scope).problem = err.Error()
		return
	}
	if p.composerSelections == nil {
		p.composerSelections = make(map[string][]product.ComposerPreparation)
	}
	p.composerSelections[id] = commands
	if p.workspaceReferences == nil {
		p.workspaceReferences = make(map[string][]product.DraftReferenceSelection)
	}
	p.workspaceReferences[id] = nil
	for _, ref := range value.References {
		if ref.Kind != "file" {
			p.workspaceReferences[id] = append(p.workspaceReferences[id], product.DraftReferenceSelection{Kind: ref.Kind, ID: ref.ID, SHA256: ref.SHA256})
		}
	}
	if files != nil {
		if p.composerFiles == nil {
			p.composerFiles = make(map[string][]product.ComposerFileSelection)
		}
		p.composerFiles[id] = append([]product.ComposerFileSelection(nil), files...)
	}
	if p.composerFileRecords == nil {
		p.composerFileRecords = make(map[string]hub.ComposerFile)
	}
	for id, record := range records {
		p.composerFileRecords[id] = record
	}
	if p.workspaceComposerDraftScope() == scope {
		p.model.Draft = value.Text
	}
}

func (p *NativePage) observeWorkspaceDraft() {
	if p.Service.ComposerDrafts == nil {
		return
	}
	scope := p.workspaceComposerDraftScope()
	e := p.workspaceDraftEntry(scope)
	if !e.known && !e.loading {
		p.loadWorkspaceDraft()
	}
	if value, err := p.captureWorkspaceDurableDraft(); err != nil {
		e.problem = err.Error()
	} else if !sameWorkspaceDraftContent(value, e.local) {
		p.markWorkspaceDraftEdited()
	}
	if !e.saving && !e.loading {
		if current, ok := p.Service.ComposerDrafts.Snapshot(scope); ok && current.Revision > e.committed.Revision {
			if e.dirty {
				e.conflict, e.problem = true, "另一窗口已保存新草稿；当前输入已保留。"
			} else {
				p.readWorkspaceDraft(scope, true)
			}
		}
		if e.known && e.dirty && !e.conflict && e.problem == "" && time.Since(e.edited) >= 250*time.Millisecond {
			p.queueWorkspaceDraftSave(scope, e.local, e.version, nil)
		}
	}
}

func (p *NativePage) queueWorkspaceDraftSave(scope hub.ComposerDraftScope, value hub.ComposerDraft, version uint64, after func(hub.ComposerDraft, error)) {
	e := p.workspaceDraftEntry(scope)
	e.queue = append(e.queue, workspaceDraftSave{hub.CloneComposerDraft(value), version, after})
	p.startWorkspaceDraftSave(scope)
}

func (p *NativePage) startWorkspaceDraftSave(scope hub.ComposerDraftScope) {
	e := p.workspaceDraftEntry(scope)
	if !e.known || e.loading || e.saving || e.conflict || len(e.queue) == 0 {
		return
	}
	request := e.queue[0]
	e.queue = e.queue[1:]
	e.saving = true
	expected := e.committed.Revision
	ctx := p.ctx
	go func() {
		value, err := p.Service.ComposerDrafts.Save(ctx, request.value, expected)
		p.dispatch(func() {
			e.saving = false
			if err != nil {
				e.problem = "草稿保存失败，输入已保留：" + err.Error()
				e.conflict = errors.Is(err, hub.ErrComposerDraftConflict)
				if request.after != nil {
					request.after(hub.ComposerDraft{}, err)
				}
				queued := e.queue
				e.queue = nil
				for _, next := range queued {
					if next.after != nil {
						next.after(hub.ComposerDraft{}, err)
					}
				}
				return
			}
			e.committed, e.problem, e.conflict = hub.CloneComposerDraft(value), "", false
			e.dirty = e.version != request.version
			if !e.dirty {
				e.local = hub.CloneComposerDraft(value)
			}
			if request.after != nil {
				request.after(value, nil)
			}
			p.startWorkspaceDraftSave(scope)
		})
	}()
}

func (p *NativePage) flushWorkspaceDraft() {
	if p.Service.ComposerDrafts == nil {
		return
	}
	scope := p.workspaceComposerDraftScope()
	e := p.workspaceDraftEntry(scope)
	if value, err := p.captureWorkspaceDurableDraft(); err == nil && !sameWorkspaceDraftContent(value, e.local) {
		p.markWorkspaceDraftEdited()
	}
	if e.dirty && !e.conflict && len(e.queue) == 0 {
		p.queueWorkspaceDraftSave(scope, e.local, e.version, nil)
	}
}

func (p *NativePage) prepareWorkspaceDraftSubmission(after func(workspaceDraftSubmission)) {
	if p.Service.ComposerDrafts == nil {
		after(workspaceDraftSubmission{})
		return
	}
	p.observeWorkspaceDraft()
	scope := p.workspaceComposerDraftScope()
	e := p.workspaceDraftEntry(scope)
	if e.conflict || e.problem != "" {
		p.problem = e.problem
		return
	}
	p.busy, p.chat.Busy = true, true
	version := e.version
	p.queueWorkspaceDraftSave(scope, e.local, version, func(value hub.ComposerDraft, err error) {
		p.busy, p.chat.Busy = false, false
		if err != nil {
			p.problem = err.Error()
			return
		}
		token := workspaceDraftSubmission{valid: true, scope: scope, revision: value.Revision, version: version}
		if scope.Kind == "session" {
			token.homeRevision = p.workspaceLinkedHomeRevision(scope.ID, value)
		}
		after(token)
	})
}

func (p *NativePage) finishWorkspaceDraftSubmission(token workspaceDraftSubmission) bool {
	if !token.valid || p.Service.ComposerDrafts == nil {
		return false
	}
	e := p.workspaceDraftEntry(token.scope)
	if token.scope == p.workspaceComposerDraftScope() {
		if current, err := p.captureWorkspaceDurableDraft(); err == nil && !sameWorkspaceDraftContent(current, e.local) {
			p.markWorkspaceDraftEdited()
		}
	}
	if value, ok := p.Service.ComposerDrafts.Snapshot(token.scope); ok && value.Revision >= e.committed.Revision {
		e.committed = value
	}
	if token.homeRevision != 0 {
		homeScope := hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}
		if home := p.durableDrafts.entries[homeScope]; home != nil {
			if current, ok := p.Service.ComposerDrafts.Snapshot(homeScope); ok && current.Revision > home.committed.Revision && current.PendingSessionID == "" && !home.dirty {
				home.committed, home.local = current, hub.CloneComposerDraft(current)
				p.applyWorkspaceLocalDraft(homeScope, current, []product.ComposerFileSelection{}, nil)
			}
		}
	}
	if e.version == token.version && e.committed.Text == "" && len(e.committed.References) == 0 && len(e.committed.PreparedCommands) == 0 {
		e.local, e.dirty, e.conflict, e.problem = hub.CloneComposerDraft(e.committed), false, false, ""
		p.applyWorkspaceLocalDraft(token.scope, e.local, []product.ComposerFileSelection{}, nil)
		return true
	} else {
		e.dirty = true
		if e.committed.Revision == token.revision+1 {
			e.conflict, e.problem = false, ""
		}
	}
	return false
}

func (p *NativePage) guardWorkspaceDraftClose(event *mygo.CloseEvent) bool {
	if p.Service.ComposerDrafts == nil {
		return false
	}
	p.flushWorkspaceDraft()
	for _, e := range p.durableDrafts.entries {
		if e.loading || e.saving || e.dirty || len(e.queue) != 0 {
			event.PreventDefault()
			p.problem = "正在保全未发送草稿；保存失败时请先处理草稿提示。"
			return true
		}
	}
	return false
}

func (p *NativePage) buildWorkspaceDraftNotice(c *ui.Context) {
	if p.Service.ComposerDrafts == nil {
		return
	}
	scope := p.workspaceComposerDraftScope()
	e := p.durableDrafts.entries[scope]
	if e == nil || e.problem == "" {
		return
	}
	ui.Column(c.Key("durable-draft-notice")).FillWidth().Gap(4).Children(func() {
		ui.Text(c, e.problem).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		ui.Row(c).Gap(8).Children(func() {
			ui.ButtonBase(c.Key("reload-shared-draft")).Label("Reload shared draft").Disabled(e.saving || e.loading).OnClick(func() {
				e.dirty = false
				p.readWorkspaceDraft(scope, true)
			}).Children(func() { ui.Text(c, "载入共享草稿（放弃本窗修改）").FontSize(workspaceFontCaption) })
			ui.ButtonBase(c.Key("retry-shared-draft")).Label("Save this draft").Disabled(e.saving || e.loading).OnClick(func() {
				local, version := hub.CloneComposerDraft(e.local), e.version
				e.loading = true
				ctx := p.ctx
				go func() {
					current, err := p.Service.ComposerDrafts.Read(ctx, scope)
					p.dispatch(func() {
						e.loading = false
						if err != nil {
							e.problem = err.Error()
							return
						}
						e.committed, e.known, e.conflict, e.problem = current, true, false, ""
						local.PendingSessionID = current.PendingSessionID
						p.queueWorkspaceDraftSave(scope, local, version, nil)
					})
				}()
			}).Children(func() { ui.Text(c, "保存本窗草稿").FontSize(workspaceFontCaption) })
		})
	})
}

func (p *NativePage) restoreWorkspacePendingSession(home hub.ComposerDraft, files []product.ComposerFileSelection, records map[string]hub.ComposerFile) {
	id := home.PendingSessionID
	scope := hub.ComposerDraftScope{Kind: "session", ID: id}
	e := p.workspaceDraftEntry(scope)
	if e.loading {
		return
	}
	e.loading = true
	version := e.version
	ctx := p.ctx
	go func() {
		value, err := p.Service.ComposerDrafts.Read(ctx, scope)
		restoredFiles, restoredRecords := files, records
		if err == nil && value.Revision != 0 {
			restoredFiles, restoredRecords, err = p.Service.RestoreComposerFileSelections(ctx, value)
		}
		p.dispatch(func() {
			e.loading = false
			if err != nil {
				p.workspaceDraftEntry(home.Scope).problem = "已创建会话的草稿读取失败，首页输入已保留：" + err.Error()
				return
			}
			e.committed, e.known = value, true
			if e.version != version || e.dirty {
				if !sameWorkspaceDraftContent(e.local, value) {
					e.conflict, e.problem = true, "已创建会话的共享草稿已变化，当前输入已保留。"
				}
				if p.workspaceComposerDraftScope() == home.Scope {
					p.selectSession(id)
				}
				return
			}
			if value.Revision == 0 {
				value = hub.CloneComposerDraft(home)
				value.Scope, value.Revision, value.PendingSessionID = scope, 0, ""
				e.dirty, e.version = true, e.version+1
			}
			e.local = hub.CloneComposerDraft(value)
			p.applyWorkspaceLocalDraft(scope, value, restoredFiles, restoredRecords)
			if p.workspaceComposerDraftScope() == home.Scope {
				p.selectSession(id)
			}
			if e.dirty {
				p.queueWorkspaceDraftSave(scope, value, e.version, nil)
			}
		})
	}()
}

func (p *NativePage) bindWorkspaceCreatedDraft(id string, bound hub.ComposerDraft, imported []product.DraftReferenceSelection) {
	home := p.workspaceDraftEntry(hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"})
	if current, err := p.captureWorkspaceDurableDraft(); err == nil && !sameWorkspaceDraftContent(current, home.local) {
		p.markWorkspaceDraftEdited()
	}
	home.committed = hub.CloneComposerDraft(bound)
	home.local.PendingSessionID = id
	home.problem, home.conflict = "", false
	value := hub.CloneComposerDraft(home.local)
	value.Scope, value.Revision, value.PendingSessionID = hub.ComposerDraftScope{Kind: "session", ID: id}, 0, ""
	for _, ref := range imported {
		value.References = append(value.References, hub.ComposerDraftReference{Kind: ref.Kind, ID: ref.ID, SHA256: ref.SHA256})
	}
	destination := p.workspaceDraftEntry(value.Scope)
	if destination.known && (destination.dirty || destination.committed.Revision != 0) {
		destination.conflict, destination.problem = true, "已创建会话已有草稿；原会话输入已保留，请在该会话确认后再发送。"
		return
	}
	destination.local, destination.version, destination.dirty, destination.known = value, home.version, true, true
	// If a later route/load step fails, navigation still flushes this copied
	// draft. The atomic home pointer remains available across any restart.
	p.queueWorkspaceDraftSave(value.Scope, value, destination.version, nil)
}

func (p *NativePage) workspaceLinkedHomeRevision(id string, session hub.ComposerDraft) uint64 {
	home, ok := p.Service.ComposerDrafts.Snapshot(hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"})
	if !ok || home.PendingSessionID != id {
		return 0
	}
	home.Scope, home.PendingSessionID = session.Scope, ""
	if !sameWorkspaceDraftContent(home, session) {
		return 0
	}
	return home.Revision
}

// The home scope is relocated only after its complete input has a committed
// session copy. A crash before this CAS retains the atomic pending identity; a
// crash after it leaves the input in the actual session, never an empty gap.
func (p *NativePage) completeWorkspaceDraftTransfer(saved hub.ComposerDraft, version uint64, after func(error)) {
	scope := hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}
	home := p.workspaceDraftEntry(scope)
	if home.version != version || home.local.Text != saved.Text || !bytes.Equal(home.local.PreparedCommands, saved.PreparedCommands) || home.conflict {
		after(nil)
		return
	}
	selected := make(map[hub.ComposerDraftReference]bool, len(saved.References))
	for _, ref := range saved.References {
		selected[ref] = true
	}
	for _, ref := range home.local.References {
		if !selected[ref] {
			after(nil)
			return
		}
	}
	p.queueWorkspaceDraftSave(scope, hub.ComposerDraft{Scope: scope}, version, func(value hub.ComposerDraft, err error) {
		if err == nil && home.version == version {
			p.applyWorkspaceLocalDraft(scope, value, []product.ComposerFileSelection{}, nil)
		}
		after(err)
	})
}

func (p *NativePage) scheduleWorkspaceFirstSend(id, prompt string, preparations []product.ComposerPreparation, references []product.DraftReferenceSelection, files []product.ComposerFileSelection, draft workspaceDraftSubmission) {
	if p.selected != id {
		return
	}
	if p.detail.Session.ID == id && !p.polling {
		p.busy, p.chat.Busy = false, false
		p.sendPreparedFileSnapshot(prompt, preparations, references, files, draft)
		return
	}
	p.pendingFirstSend, p.pendingFirstPrompt = id, prompt
	p.pendingFirstSelections, p.pendingFirstReferences = preparations, references
	p.pendingFirstFiles, p.pendingFirstDraft = files, draft
	if !p.polling {
		p.busy = false
	}
	p.loadTail()
}
