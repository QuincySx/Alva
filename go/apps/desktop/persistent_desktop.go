// INPUT: Direct application Service, native Controller observation and MyGo window dispatcher
// OUTPUT: PersistentPage and RunPersistent with cold selection/history and actual Send/Stop/approval/file actions
// POS: [L6 application] Durable direct UI observes the existing harness owner without EngineRuntime or Hub.
//go:build desktop

package desktop

import (
	"context"
	"time"

	"github.com/QuincySx/Alva/go/apps/debug"
	"github.com/QuincySx/Alva/go/apps/desktop/modules"
	"github.com/QuincySx/Alva/go/apps/direct"
	"github.com/QuincySx/Alva/go/platform/host"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type PersistentPage struct {
	inspectExportBusy                           bool
	Service                                     *direct.Service
	model                                       *Model
	chat                                        *NativeView
	ctx                                         context.Context
	window                                      *mygo.Window
	updates                                     chan func()
	selected, title, problem, pane              string
	meta                                        direct.Metadata
	rows                                        direct.SessionPage
	sessionAfter                                string
	sessionBack                                 []string
	busy, listBusy, approvalBusy, inspectorBusy bool
	generation                                  uint64
	drafts                                      map[string]string
	history                                     direct.HistoryPage
	reviewing                                   bool
	run                                         direct.Run
	detachRun                                   context.CancelFunc
	approvals                                   []security.ApprovalRequest
	attachments                                 []session.ArtifactInfo
	inspector                                   session.RecordPage
	inspectAfter                                []uint64
	inspectCursor                               uint64
	inspectQuery                                string
	inspectLoadedQuery                          string
	inspectGroup, inspectExportPath             string
	inspectURI                                  string
	inspectText                                 agentcontext.TextPage
	diagnosticFrame, diagnosticChanged          func()
}

func NewPersistentPage(service *direct.Service) *PersistentPage {
	m := NewModel(service.Demo())
	m.RouteLabel = service.RouteLabel()
	m.maxRows = 128
	p := &PersistentPage{Service: service, model: m, ctx: context.Background(), updates: make(chan func(), 8), title: "New conversation", drafts: make(map[string]string)}
	p.chat = NewNativeView(m)
	p.chat.SetMarkdownOptions(modules.NewRegistry().Options())
	p.chat.Send = p.send
	p.chat.Stop = p.stop
	p.chat.LargePasteHint = "Pasted text exceeds32KiB. Attach an explicitly selected file as a durable reference; nothing was uploaded or sent."
	return p
}
func (p *PersistentPage) changed() {
	if p.diagnosticChanged != nil {
		p.diagnosticChanged()
	}
}
func (p *PersistentPage) dispatch(fn func()) {
	apply := func() {
		if p.ctx.Err() != nil {
			return
		}
		p.changed()
		fn()
	}
	if p.window != nil {
		p.window.Update(apply)
		return
	}
	select {
	case p.updates <- apply:
	case <-p.ctx.Done():
	}
}
func (p *PersistentPage) refreshSessions(after string) {
	if p.listBusy {
		return
	}
	p.listBusy = true
	go func() {
		page, err := p.Service.List(p.ctx, after)
		p.dispatch(func() {
			p.listBusy = false
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.rows = page
			p.sessionAfter = after
		})
	}()
}
func (p *PersistentPage) create() {
	if p.busy {
		return
	}
	p.changed()
	p.busy = true
	p.chat.Busy = true
	title := p.title
	go func() {
		id, err := p.Service.Create(p.ctx, title)
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if err != nil {
				p.problem = "Session creation did not complete: " + err.Error()
				if id != "" {
					p.problem += "; allocated native ID: " + id
				}
				return
			}
			p.refreshSessions("")
			p.selectSession(id)
		})
	}()
}
func (p *PersistentPage) selectSession(id string) {
	if p.busy || id == p.selected {
		return
	}
	if p.selected != "" && p.model.Draft != "" {
		if _, exists := p.drafts[p.selected]; !exists && len(p.drafts) >= 16 {
			p.problem = "16 session drafts retained; clear an existing draft before selecting another session."
			return
		}
		p.drafts[p.selected] = p.model.Draft
	} else {
		delete(p.drafts, p.selected)
	}
	p.changed()
	if p.detachRun != nil {
		p.detachRun()
		p.detachRun = nil
	}
	p.selected = id
	p.generation++
	p.pane = ""
	p.meta = direct.Metadata{}
	p.history = direct.HistoryPage{}
	p.run = direct.Run{}
	p.approvals = nil
	p.attachments = nil
	p.inspectURI = ""
	p.inspectText = agentcontext.TextPage{}
	p.inspector = session.RecordPage{}
	p.inspectGroup = ""
	p.inspectExportPath = ""
	p.inspectAfter = nil
	p.model.Messages = nil
	p.model.ActiveRunID = ""
	p.model.Status = Idle
	p.model.Error = ""
	p.model.Draft = p.drafts[id]
	p.problem = ""
	p.loadHistory(0)
}
func (p *PersistentPage) loadHistory(before uint64) {
	if p.selected == "" || p.busy {
		return
	}
	p.changed()
	p.busy = true
	p.chat.Busy = true
	id, generation := p.selected, p.generation
	go func() {
		meta, err := p.Service.Metadata(p.ctx, id)
		var page direct.HistoryPage
		var attachments []session.ArtifactInfo
		if err == nil {
			page, err = p.Service.Conversation(p.ctx, id, before)
			attachments = p.Service.DraftAttachments(id)
		}
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.meta = meta
			p.attachments = attachments
			p.history = page
			p.reviewing = before != 0
			p.model.Messages = directRows(page.Rows)
			p.model.Notice = "Only this bounded native record window is displayed; Inspector retains original typed records and references. Cold viewing never resumes an Agent."
			if p.reviewing {
				p.model.Notice = "Reading an earlier fixed record window. Latest reloads current committed facts; execution is not replayed."
			}
			if run, ok := p.Service.CurrentRun(id); ok {
				p.attachRun(run)
			}
			if !p.reviewing {
				p.chat.ScrollToEnd()
			}
		})
	}()
}
func directRows(rows []direct.ConversationRow) []Message {
	result := make([]Message, 0, len(rows))
	for _, row := range rows {
		if row.Message == nil {
			result = append(result, Message{ID: row.ID, Role: "Stored message", Text: row.Processing + "\n" + row.Reference.URI, Status: Completed})
			continue
		}
		message := row.Message
		role := string(message.Role)
		switch message.Role {
		case wire.RoleUser:
			role = "You"
		case wire.RoleAssistant:
			role = "Alva"
		case wire.RoleTool:
			role = "Tool result"
		}
		text := message.Text()
		for _, block := range message.Content {
			if block.Kind == wire.ContentReference && block.Reference != nil {
				text += "\n[Saved reference: " + block.Reference.URI + "]"
			} else if block.Kind != wire.ContentText {
				text += "\n[Typed " + string(block.Kind) + " content retained; inspect original record: " + row.Reference.URI + "]"
			}
		}
		result = append(result, Message{ID: row.ID, Role: role, Text: text, Status: Completed})
	}
	return result
}
func (p *PersistentPage) attachRun(run direct.Run) {
	wasStopping := p.run.ID == run.ID && p.model.Status == Stopping
	if p.detachRun != nil {
		p.detachRun()
		p.detachRun = nil
	}
	p.run = run
	snapshot, err := run.Controller.Snapshot(run.ID)
	if err != nil {
		p.problem = err.Error()
		return
	}
	p.model.Status = Status(snapshot.Status)
	p.model.lastSeq = 0
	if snapshot.Status != host.RunRunning {
		p.model.ActiveRunID = ""
		return
	}
	p.model.ActiveRunID = run.ID
	if wasStopping {
		p.model.Status = Stopping
	}
	if !p.reviewing {
		found := false
		for _, row := range p.model.Messages {
			if row.RunID == run.ID && row.Role == "Alva" {
				found = true
			}
		}
		if !found && len(p.model.Messages) > 0 && snapshot.Text != "" {
			last := &p.model.Messages[len(p.model.Messages)-1]
			if last.Role == "Alva" && last.Text == snapshot.Text {
				last.RunID = run.ID
				last.Status = Running
				found = true
			}
		}
		if !found {
			p.model.Messages = append(p.model.Messages, Message{ID: run.ID + "/observed", RunID: run.ID, Role: "Alva", Status: Running, Text: snapshot.Text})
		}
	}
	life, detach := context.WithCancel(p.ctx)
	p.detachRun = detach
	id, generation := p.selected, p.generation
	go observe(life, run.Controller, run.ID, func(snapshot host.RunSnapshot) {
		applied := make(chan struct{})
		p.dispatch(func() {
			defer close(applied)
			if p.selected != id || p.generation != generation || p.run.ID != run.ID {
				return
			}
			p.model.Observe(run.ID, snapshot.HighWater, snapshot.Text, Status(snapshot.Status), snapshot.Error)
			if snapshot.Status != host.RunRunning {
				p.approvals = nil
				p.refreshSessions(p.sessionAfter)
				if !p.reviewing {
					p.loadHistory(0)
				}
			}
		})
		select {
		case <-applied:
		case <-life.Done():
		}
	}, func(err error) {
		p.dispatch(func() {
			if p.selected == id && p.generation == generation && p.run.ID == run.ID {
				p.problem = "Observation failed; execution was not retried: " + err.Error()
			}
		})
	})
}
func (p *PersistentPage) send() {
	if p.selected == "" || p.busy || !p.model.CanSend() {
		return
	}
	p.changed()
	id, generation, prompt := p.selected, p.generation, p.model.Draft
	p.busy = true
	p.chat.Busy = true
	go func() {
		run, err := p.Service.Start(p.ctx, id, prompt)
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.model.Admitted(run.ID, prompt)
			p.attachments = nil
			delete(p.drafts, id)
			p.reviewing = false
			p.chat.ScrollToEnd()
			p.attachRun(run)
			p.refreshSessions(p.sessionAfter)
			if snapshot, err := run.Controller.Snapshot(run.ID); err == nil && snapshot.Status != host.RunRunning {
				p.model.ActiveRunID = ""
				p.model.Status = Status(snapshot.Status)
				p.loadHistory(0)
			}
		})
	}()
}
func (p *PersistentPage) stop() {
	if p.model.ActiveRunID == "" || p.busy || p.model.Status == Stopping {
		return
	}
	p.changed()
	id := p.model.RequestStop()
	if err := p.Service.Stop(p.selected, id); err != nil {
		p.model.Status = Running
		p.problem = err.Error()
	}
}
func (p *PersistentPage) respond(requestID string, allowed bool) {
	if p.approvalBusy {
		return
	}
	p.approvalBusy = true
	id, generation := p.selected, p.generation
	go func() {
		err := p.Service.RespondApproval(p.ctx, id, requestID, allowed)
		p.dispatch(func() {
			p.approvalBusy = false
			if p.selected != id || p.generation != generation {
				return
			}
			if err != nil {
				p.problem = err.Error()
			}
			p.approvals = p.Service.PendingApprovals(id)
		})
	}()
}
func (p *PersistentPage) upload() {
	if p.window == nil || p.selected == "" || p.busy || p.model.ActiveRunID != "" {
		return
	}
	p.changed()
	id, generation := p.selected, p.generation
	p.busy = true
	p.chat.Busy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Title: "Save original files as references — no automatic model delivery", Multiple: true})
		if err == nil {
			for _, path := range paths {
				if _, err = p.Service.Upload(p.ctx, id, path); err != nil {
					break
				}
			}
		}
		attachments := p.Service.DraftAttachments(id)
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if p.selected == id && p.generation == generation {
				p.attachments = attachments
			}
			if p.selected == id && p.generation == generation && err != nil {
				p.problem = err.Error()
			}
		})
	}()
}
func (p *PersistentPage) loadInspector(after, highWater uint64) {
	if p.selected == "" || p.inspectorBusy {
		return
	}
	p.changed()
	p.inspectorBusy = true
	p.pane = "inspector"
	query := p.inspectLoadedQuery
	if highWater == 0 {
		query = p.inspectQuery
	}
	id, generation := p.selected, p.generation
	go func() {
		page, err := p.Service.History(p.ctx, id, query, after, highWater)
		p.dispatch(func() {
			p.inspectorBusy = false
			if p.selected != id || p.generation != generation || p.pane != "inspector" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.inspector = page
			p.inspectCursor = after
			p.inspectLoadedQuery = query
			p.inspectURI = ""
			p.inspectText = agentcontext.TextPage{}
		})
	}()
}

func (p *PersistentPage) removeAttachment(artifactID string) {
	if p.busy || p.model.ActiveRunID != "" || p.selected == "" {
		return
	}
	p.busy = true
	p.chat.Busy = true
	id, generation := p.selected, p.generation
	go func() {
		p.Service.RemoveDraftAttachment(id, artifactID)
		attachments := p.Service.DraftAttachments(id)
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if p.selected == id && p.generation == generation {
				p.attachments = attachments
			}
		})
	}()
}
func (p *PersistentPage) refreshPermissions() {
	if p.selected != "" {
		p.changed()
		p.approvals = p.Service.PendingApprovals(p.selected)
	}
}
func (p *PersistentPage) readInspector(uri string, offset int64) {
	if p.inspectorBusy || p.selected == "" {
		return
	}
	p.changed()
	p.inspectorBusy = true
	id, generation := p.selected, p.generation
	go func() {
		text, err := p.Service.InspectorRead(p.ctx, id, uri, offset)
		p.dispatch(func() {
			p.inspectorBusy = false
			if p.selected != id || p.generation != generation || p.pane != "inspector" {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.inspectURI = uri
			p.inspectText = text
		})
	}()
}

func RunPersistent(service *direct.Service, diagnostics *debug.Server, initialID string) error {
	p := NewPersistentPage(service)
	mygo.App.WhenReady(func() {
		ctx, detach := context.WithCancel(context.Background())
		p.ctx = ctx
		p.window = mygo.NewWindow(mygo.WindowOptions{Title: "ALVA · Direct native", Width: 1160, Height: 850, MinWidth: 920, MinHeight: 680, Content: ui.View(p.Build)})
		p.window.OnClosed(func() {
			detach()
			if p.detachRun != nil {
				p.detachRun()
			}
		})
		p.window.OnClose(func(event *mygo.CloseEvent) {
			if p.busy || p.inspectExportBusy || service.HasActive() {
				event.PreventDefault()
				p.changed()
				p.problem = "An actual direct run is active. Select it, Stop and wait for acknowledgement before closing."
				p.window.Invalidate()
			}
		})
		if diagnostics != nil && !diagnostics.Closed() {
			view, methods, err := p.Diagnostic(ctx, p.window.Update, p.window.CapturePage)
			if err == nil {
				p.diagnosticFrame, p.diagnosticChanged = view.TrackRendering(p.window.Invalidate)
				var unregister func()
				unregister, err = diagnostics.Register("direct-window", "AlvaPersistentDirectWindow", methods, view)
				if err == nil {
					p.window.OnClosed(unregister)
				}
			}
			if err != nil {
				p.problem = "Direct diagnostic registration failed"
			}
		}
		p.refreshSessions("")
		if initialID != "" {
			p.selectSession(initialID)
		}
		go func() {
			ticker := time.NewTicker(250 * time.Millisecond)
			defer ticker.Stop()
			for {
				select {
				case <-ctx.Done():
					return
				case <-ticker.C:
				}
				applied := make(chan struct{})
				p.window.Update(func() {
					defer close(applied)
					if ctx.Err() == nil {
						p.refreshPermissions()
					}
				})
				select {
				case <-ctx.Done():
					return
				case <-applied:
				}
			}
		}()
	})
	return mygo.App.Run()
}
