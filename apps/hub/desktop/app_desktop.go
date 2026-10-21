// INPUT: MyGo native UI, shared desktop controls, durable product.HubService and bounded product pages
// OUTPUT: Run, NativePage and session home/history/file/branch controls
// POS: [L7 Hub product] Separate native Hub product entry; no WebView or direct-harness entry rewrite
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"github.com/QuincySx/Alva/apps/debug"
	"io"
	"log/slog"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/apps/ui/modules"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type NativePage struct {
	diagnosticLog                  *slog.Logger
	diagnosticFrame                func()
	diagnosticChanged              func()
	Service                        *product.HubService
	model                          *view.Model
	chat                           *view.NativeView
	transcript                     Transcript
	sessions                       []hub.Session
	sessionQuery                   string
	sessionLoadedQuery             string
	sessionPage                    hub.SessionPage
	sessionCursor                  hub.SessionCursor
	sessionBack                    []hub.SessionCursor
	sessionLoading                 bool
	detail                         hub.Detail
	selected                       string
	drafts                         map[string]string
	title, workspace, problem      string
	busy, polling, reviewing       bool
	generation                     uint64
	window                         *mygo.Window
	openWindow                     func(string) error
	ctx                            context.Context
	list                           ui.ListState
	targets                        []hub.Target
	approvals                      []security.ApprovalRequest
	pane, configDraft              string
	aepDraft                       string
	commandDraft                   string
	toolsDraft                     string
	gatewayCredentials             string
	gatewayBusy                    bool
	questions                      []interaction.Question
	questionID, questionText       string
	questionSelected               map[int]bool
	browserDraft                   string
	handoffText                    string
	automation                     automationPane
	media                          MediaPane
	decoderFFmpeg, decoderProbe    string
	player                         localPlayback
	audioVoice                     func([]float32) audioVoice
	audioOutputError               func() error
	playerBusy, playerStopping     bool
	playerCancel                   context.CancelFunc
	playerToken                    uint64
	playerFrames                   int
	playerPosition, playerDuration int64
	playerSeek                     string
	settings                       hub.Settings
	wait                           waitPane
	paneBusy                       bool
	inspectBranch, inspectQuery    string
	inspectLoadedQuery             string
	inspectGroup                   string
	inspectExportChildren          bool
	inspectExportBusy              bool
	inspectExportPath              string
	inspectPage                    product.InspectorPage
	inspectURI                     string
	inspectText                    agentcontext.TextPage
	inspectAfter                   []uint64
	inspectCursor                  uint64
	inspectPath                    []product.ChildHop
	inspectFrames                  []InspectorFrame
	inspectList                    ui.ListState
	updates                        chan func()
	skills                         []skill.Skill
	skillDraft                     string
	compatibilityDraft             string
	team                           hub.TeamDetail
	teamLead, teamWorker           string
	workerName, workerDraft        string
	workerPath, editTaskID         string
	workerReadOnly                 bool
	workerTarget                   int
	taskPage                       hub.WorkerTaskPage
	taskCursor                     uint64
	taskBack                       []uint64
	teamList                       ui.ListState
	workerList                     ui.ListState
	editClaims                     []hub.ResourceClaim
	worktreePatch                  product.WorktreePatchPage
	worktreeOffset                 int
	worktreeConfirm                string
	worktreePatchID                string
	worktreeList                   ui.ListState
	shellJobs                      []product.ShellJob
	shellJobList                   ui.ListState
}

func NewNativePage(service *product.HubService) *NativePage {
	model := view.NewModel(service.Demo)
	chat := view.NewNativeView(model)
	chat.LargePasteHint = "Pasted text exceeds 32 KiB. Use Attach file: raw bytes are stored and only an explicit reference is sent."
	chat.SetMarkdownOptions(modules.NewRegistry().Options())
	page := &NativePage{Service: service, model: model, chat: chat, title: "New conversation", drafts: make(map[string]string), ctx: context.Background(), targets: append([]hub.Target(nil), service.Targets...), updates: make(chan func(), 8)}
	chat.Send = page.send
	chat.Stop = page.stop
	return page
}
func Run(service *product.HubService) error {
	return runNative(service, nil, nil)
}

// RunDiagnosed is opt-in own-window QA instrumentation, not a production default
// or a substitute for observing real native controls through accessibility.
func RunDiagnosed(service *product.HubService, diagnostics *debug.Server, output io.Writer) error {
	return runNative(service, diagnostics, output)
}
func runNative(service *product.HubService, diagnostics *debug.Server, output io.Writer) error {
	// Native startup is the explicit scheduler owner. Constructing Open/Service
	// remains cold, and every stored automation is paused until separately approved.
	if service.Automations != nil {
		if err := service.Automations.Start(); err != nil {
			return err
		}
	}
	windows := &nativeWindows{service: service, output: output, pages: make(map[*NativePage]struct{}), diagnostics: diagnostics}
	if output != nil {
		fmt.Fprintln(output, "native_qa: register WhenReady; no task/session auto-start")
		time.AfterFunc(3*time.Second, func() { fmt.Fprintf(output, "native_qa: ready watchdog App.IsReady=%v\n", mygo.App.IsReady()) })
	}
	mygo.App.WhenReady(func() {
		if output != nil {
			fmt.Fprintln(output, "native_qa: App ready; creating actual self-drawn window")
		}
		if err := windows.open(""); err != nil && output != nil {
			fmt.Fprintln(output, "native_qa: window creation failed")
		}
	})
	return mygo.App.Run()
}
func (p *NativePage) dispatch(fn func()) {
	apply := func() {
		if p.diagnosticChanged != nil {
			p.diagnosticChanged()
		}
		fn()
	}
	if p.window != nil {
		p.window.Update(apply)
	} else {
		select {
		case p.updates <- apply:
		case <-p.ctx.Done():
		}
	}
}
func (p *NativePage) setPane(name string) {
	p.traceUI("open_pane")
	if p.pane == "team" && name != "team" {
		p.worktreePatch = product.WorktreePatchPage{}
		p.worktreeOffset = 0
		p.worktreeConfirm = ""
		p.worktreePatchID = ""
	}
	if p.pane == "media" && name != "media" {
		p.stopPlayback()
		p.media = MediaPane{}
	}
	p.pane = name
}
func (p *NativePage) refreshSessions() {
	p.sessionBack = nil
	p.loadSessions(hub.SessionCursor{})
}
func (p *NativePage) loadSessions(cursor hub.SessionCursor) {
	if p.sessionLoading {
		return
	}
	p.sessionLoading = true
	query := p.sessionQuery
	go func() {
		page, err := p.Service.Store.PageSessions(p.ctx, query, cursor, 64)
		p.dispatch(func() {
			p.sessionLoading = false
			if p.sessionQuery != query {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.sessions = page.Sessions
			p.sessionLoadedQuery = query
			p.sessionPage = page
			p.sessionCursor = cursor
		})
	}()
}
func (p *NativePage) selectSession(id string) {
	p.traceUI("select_session")
	if p.busy {
		return
	}
	p.stopPlayback()
	if p.selected != "" {
		p.drafts[p.selected] = p.model.Draft
	}
	p.selected = id
	p.model.Draft = p.drafts[id]
	p.generation++
	p.reviewing = false
	p.transcript = Transcript{}
	p.pane = ""
	p.configDraft = ""
	p.aepDraft = ""
	p.commandDraft = ""
	p.toolsDraft = ""
	p.shellJobs = nil
	p.approvals = nil
	p.questions = nil
	p.questionID, p.questionText = "", ""
	p.questionSelected = nil
	p.browserDraft = ""
	p.handoffText = ""
	p.automation = automationPane{}
	p.worktreePatch = product.WorktreePatchPage{}
	p.worktreeOffset = 0
	p.worktreeConfirm = ""
	p.worktreePatchID = ""
	p.media = MediaPane{}
	p.decoderFFmpeg, p.decoderProbe = "", ""
	p.settings = hub.Settings{}
	p.wait = waitPane{}
	p.inspectPage = product.InspectorPage{}
	p.inspectGroup = ""
	p.inspectExportPath = ""
	p.inspectLoadedQuery = ""
	p.inspectText = agentcontext.TextPage{}
	p.inspectBranch = ""
	p.inspectPath = nil
	p.inspectFrames = nil
	p.inspectAfter = nil
	p.model.Messages = nil
	p.model.ActiveRunID = ""
	p.model.Status = view.Idle
	p.chat.Busy = true
	p.problem = ""
	p.loadTail()
}
func (p *NativePage) loadTail() {
	if p.selected == "" || p.polling {
		return
	}
	id, generation := p.selected, p.generation
	p.polling = true
	go func() {
		detail, err := p.Service.Store.Detail(p.ctx, id)
		var entries []hub.HistoryEntry
		if err == nil {
			entries, err = p.Service.Store.HistoryBefore(p.ctx, id, 0, 128)
		}
		p.dispatch(func() {
			p.polling = false
			if p.selected != id || p.generation != generation {
				p.loadTail()
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.detail = detail
			p.chat.Busy = false
			p.transcript.Replace(entries)
			p.reviewing = false
			p.applyDetail()
			p.chat.ScrollToEnd()
		})
	}()
}
func (p *NativePage) applyDetail() {
	p.chat.Busy = p.busy || p.detail.Session.Status == engine.Waiting
	p.model.Messages = p.transcript.Rows
	p.model.Error = p.problem
	wasStopping := p.model.Status == view.Stopping
	p.model.Status = view.Status(p.detail.Session.Status)
	if p.detail.Session.Status == hub.Running {
		p.model.ActiveRunID = p.detail.Session.ID
		p.model.Status = view.Running
		if wasStopping {
			p.model.Status = view.Stopping
		}
	} else {
		p.model.ActiveRunID = ""
	}
	if p.detail.Session.Status == engine.Waiting {
		p.model.Notice = "Task waiting; execution has exited. Open Waiting tasks to review requests, save results and explicitly continue the same Turn."
	} else if p.reviewing {
		p.model.Notice = "Reading earlier committed events. Live text updates are paused; Latest reloads the newest bounded page."
	} else {
		p.model.Notice = "Bounded history window: up to 512 events; its first reply may be partial. Earlier pages remain durable; this UI cache is not model context."
	}
}
func (p *NativePage) poll() {
	if p.selected == "" || p.polling || p.busy {
		return
	}
	id, generation, after := p.selected, p.generation, p.transcript.Last
	readText := !p.reviewing && p.chat.AtEnd()
	readWait, waitEpoch := p.pane == "waits" && !p.paneBusy, p.wait.Epoch
	p.polling = true
	go func() {
		detail, err := p.Service.Store.Detail(p.ctx, id)
		var entries []hub.HistoryEntry
		var approvals []security.ApprovalRequest
		var questions []interaction.Question
		var waiting product.WaitView
		var waitErr error
		if err == nil && readText {
			entries, err = p.Service.Store.History(p.ctx, id, after, 128)
		}
		if err == nil {
			approvals, err = p.Service.PendingApprovals(p.ctx, id)
		}
		if err == nil {
			questions, err = p.Service.PendingQuestions(p.ctx, id)
		}
		if err == nil && readWait {
			waiting, waitErr = p.Service.Waits(p.ctx, id)
		}
		p.dispatch(func() {
			p.polling = false
			if p.selected != id || p.generation != generation {
				p.loadTail()
				return
			}
			if err != nil {
				if readWait && p.pane == "waits" && p.wait.Epoch == waitEpoch {
					p.wait.Current = false
					p.wait.Problem = err.Error()
				}
				p.problem = err.Error()
				return
			}
			p.detail = detail
			p.approvals = approvals
			p.questions = questions
			if readWait && p.pane == "waits" && !p.paneBusy && p.wait.Epoch == waitEpoch {
				if waitErr != nil {
					p.wait.Current = false
					p.wait.Problem = waitErr.Error()
				} else {
					p.applyWaitView(waiting)
				}
			}
			if readText {
				p.transcript.Append(entries)
			}
			p.applyDetail()
		})
	}()
}
func (p *NativePage) respondApproval(requestID string, allowed bool) {
	id := p.selected
	go func() {
		err := p.Service.RespondApproval(p.ctx, id, requestID, allowed)
		p.dispatch(func() {
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
			}
			p.poll()
		})
	}()
}
func (p *NativePage) send() {
	p.traceUI("send")
	if p.selected == "" || p.busy || p.detail.Session.Status == engine.Waiting || !p.model.CanSend() {
		return
	}
	if worker, ok := p.selectedWorker(); ok {
		p.sendWorkerDraft(worker)
		return
	}
	id, prompt := p.selected, p.model.Draft
	p.busy = true
	p.chat.Busy = true
	go func() {
		_, err := p.Service.Submit(p.ctx, id, prompt)
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.model.Draft = ""
			delete(p.drafts, id)
			p.problem = ""
			p.model.ActiveRunID = id
			p.model.Status = view.Running
			p.reviewing = false
			p.chat.ScrollToEnd()
			p.poll()
			p.refreshSessions()
		})
	}()
}
func (p *NativePage) stop() {
	p.traceUI("stop")
	if p.selected == "" || p.busy {
		return
	}
	id := p.selected
	worker, isWorker := p.selectedWorker()
	lead, team := p.team.Team.LeadSessionID, p.team.Team.ID
	p.model.Status = view.Stopping
	go func() {
		var err error
		if isWorker {
			err = p.Service.Teams.Stop(p.ctx, lead, team, worker.ID)
		} else {
			err = p.Service.Runtime.Cancel(p.ctx, id)
		}
		p.dispatch(func() {
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
			}
			p.poll()
		})
	}()
}
func (p *NativePage) create() {
	p.traceUI("create_session")
	if p.busy {
		return
	}
	title, workspace := p.title, p.workspace
	if strings.TrimSpace(title) == "" {
		p.problem = "Enter a session title"
		return
	}
	p.busy = true
	go func() {
		session, err := p.Service.Create(p.ctx, title, workspace)
		p.dispatch(func() {
			p.busy = false
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.refreshSessions()
			p.selectSession(session.ID)
		})
	}()
}
func (p *NativePage) upload() {
	if p.selected == "" || p.busy || p.window == nil {
		return
	}
	id := p.selected
	p.busy = true
	p.chat.Busy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Title: "Attach original files — 20 MiB each, no automatic model delivery", Multiple: true})
		if err == nil {
			for _, path := range paths {
				if _, err = p.Service.Upload(p.ctx, id, path); err != nil {
					break
				}
			}
		}
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			if err != nil {
				p.problem = err.Error()
			}
			p.poll()
		})
	}()
}
func (p *NativePage) older() {
	if p.selected == "" || p.polling || p.transcript.First == 0 {
		return
	}
	id, generation, before := p.selected, p.generation, p.transcript.First
	p.polling = true
	go func() {
		entries, err := p.Service.Store.HistoryBefore(p.ctx, id, before, 128)
		p.dispatch(func() {
			p.polling = false
			if p.selected != id || p.generation != generation {
				p.loadTail()
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.reviewing = true
			p.transcript.Prepend(entries)
			p.applyDetail()
		})
	}()
}
func (p *NativePage) switchBranch(target hub.Target) {
	if p.busy || p.selected == "" {
		return
	}
	id := p.selected
	p.busy = true
	go func() {
		_, err := p.Service.Store.Switch(p.ctx, id, target, "Explicit native UI selection; retained previous branch")
		p.dispatch(func() {
			p.busy = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.loadTail()
		})
	}()
}
func (p *NativePage) setFallback(target *hub.Target) {
	if p.busy || p.selected == "" || p.inputBlocked() {
		return
	}
	id := p.selected
	var targets []hub.Target
	if target != nil {
		approved := *target
		approved.AutomaticAuthorized = true
		targets = []hub.Target{approved}
	}
	p.busy = true
	go func() {
		err := p.Service.Store.SetFallback(p.ctx, id, targets)
		p.dispatch(func() {
			p.busy = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.loadTail()
		})
	}()
}
func (p *NativePage) sessionLabel(session hub.Session) string {
	status := session.Status
	// The independently polled selected detail may be newer than this frozen
	// session page, including a late list query captured while a run was active.
	// Project only its status; keep the page's ordering, membership and cursors.
	if session.ID == p.selected && p.detail.Session.ID == session.ID && p.detail.Session.UpdatedAt >= session.UpdatedAt {
		status = p.detail.Session.Status
	}
	return session.Title + " · " + string(status)
}

func (p *NativePage) Build(c *ui.Context) {
	if p.diagnosticFrame != nil {
		defer p.diagnosticFrame()
	}
	for i := 0; i < 8; i++ {
		select {
		case update := <-p.updates:
			update()
		default:
			i = 8
		}
	}
	ui.Row(c).Fill().Gap(12).Padding(12).Children(func() {
		ui.Column(c).Label("Session navigation").Width(240).FillHeight().Gap(8).Children(func() {
			ui.Text(c, "ALVA Hub · Go").Role(ui.RoleHeading).Level(1).Bold().FontSize(20)
			ui.Button(c, "New window").Disabled(p.openWindow == nil).OnClick(func() {
				if err := p.openWindow(p.selected); err != nil {
					p.problem = err.Error()
				}
			})
			ui.TextInput(c.Key("title"), &p.title).Label("Session title").Placeholder("Session title")
			ui.TextInput(c.Key("workspace"), &p.workspace).Label("Workspace directory").Placeholder("Optional absolute workspace; blank = private session")
			ui.Button(c, "New session").Disabled(p.busy).OnClick(p.create)
			ui.TextInput(c.Key("session-search"), &p.sessionQuery).Label("Find session title/workspace")
			ui.Button(c, "Search / refresh sessions").Disabled(p.busy || p.sessionLoading).OnClick(p.refreshSessions)
			p.list.Key = func(i int) any { return p.sessions[i].ID }
			p.list.Label = func(i int) string { return p.sessionLabel(p.sessions[i]) }
			ui.List(c, &p.list, len(p.sessions), func(i int) {
				session := p.sessions[i]
				label := p.sessionLabel(session)
				ui.Button(c, label).Disabled(p.busy).OnClick(func() { p.selectSession(session.ID) })
			}).Grow(1)
			ui.Row(c).Gap(4).Children(func() {
				ui.Button(c, "Previous sessions").Disabled(p.sessionLoading || p.sessionLoadedQuery != p.sessionQuery || len(p.sessionBack) == 0).OnClick(func() {
					last := len(p.sessionBack) - 1
					cursor := p.sessionBack[last]
					p.sessionBack = p.sessionBack[:last]
					p.loadSessions(cursor)
				})
				ui.Button(c, "More sessions").Disabled(p.sessionLoading || p.sessionLoadedQuery != p.sessionQuery || !p.sessionPage.HasMore).OnClick(func() { p.sessionBack = append(p.sessionBack, p.sessionCursor); p.loadSessions(p.sessionPage.Next) })
			})
			ui.Text(c, "Product sessions and native execution records are separate durable authorities. No old Rust data is imported.").FontSize(12)
		})
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().Gap(8).Children(func() {
			mode := "LIVE · explicit provider"
			if p.Service.Demo {
				mode = "DEMO · local model/tool fixture, no service billing"
			}
			if branch, err := product.ActiveBranch(p.detail); err == nil && branch.Target.EngineID != "alva" {
				mode = "EXTERNAL · " + branch.Target.EngineID + " · explicit executable with real host/account access"
			}
			ui.Text(c, mode).FontSize(12)
			if p.problem != "" {
				ui.Text(c, "Error: "+p.problem).FontSize(13)
			}
			if p.selected == "" {
				ui.Text(c, "Create or select a durable session. Nothing executes on startup.").FontSize(20)
				ui.Text(c, "Native self-drawn UI: no WebView, React, npm or frontend server.")
				return
			}
			ui.Text(c, p.detail.Session.Title).Bold().FontSize(20)
			ui.Text(c, p.detail.Session.Workspace).FontSize(12)
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c, "Chat").OnClick(func() { p.setPane("") })
				ui.Button(c, "Session settings").Disabled(p.paneBusy).OnClick(p.openSettings)
				ui.Button(c, "Inspector").Disabled(p.paneBusy).OnClick(p.openInspector)
				ui.Button(c, "Waiting tasks").Disabled(p.paneBusy).OnClick(p.openWaits)
				ui.Button(c, "Handoff evidence").Disabled(p.paneBusy).OnClick(func() { p.setPane("handoffs"); p.handoffText = "" })
			})
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c, "Media evidence").Disabled(p.paneBusy).OnClick(p.openMedia)
				ui.Button(c, "Project skills").Disabled(p.paneBusy).OnClick(p.openSkills)
				ui.Button(c, "Collaboration").Disabled(p.paneBusy || p.Service.Teams == nil).OnClick(p.openTeam)
				ui.Button(c, "Automations").Disabled(p.paneBusy || p.Service.Automations == nil).OnClick(p.openAutomations)
			})
			p.buildApprovals(c)
			p.buildQuestions(c)
			if p.playerBusy {
				ui.Button(c, "Pause local video and wait for decoder cleanup").Disabled(p.playerStopping).OnClick(p.stopPlayback)
			}
			if p.pane == "waits" {
				p.buildWaits(c)
				return
			}
			if p.pane == "settings" {
				p.buildSettings(c)
				return
			}
			if p.pane == "plugins" {
				p.buildAEP(c)
				return
			}
			if p.pane == "commands" {
				p.buildCommands(c)
				return
			}
			if p.pane == "tools" {
				p.buildTools(c)
				return
			}
			if p.pane == "gateway" {
				p.buildGateway(c)
				return
			}
			if p.pane == "media-settings" {
				p.buildMediaSettings(c)
				return
			}
			if p.pane == "browser-settings" {
				p.buildBrowserSettings(c)
				return
			}
			if p.pane == "media" {
				p.buildMedia(c)
				return
			}
			if p.pane == "handoffs" {
				p.buildHandoffs(c)
				return
			}
			if p.pane == "automations" {
				p.buildAutomations(c)
				return
			}
			if p.pane == "inspector" {
				p.buildInspector(c)
				return
			}
			if p.pane == "skills" {
				p.buildSkills(c)
				return
			}
			if p.pane == "team" {
				p.buildTeam(c)
				return
			}
			_, isWorker := p.selectedWorker()
			if isWorker {
				ui.Text(c, "Worker: independent durable session. Send enqueues and explicitly resumes this Worker's FIFO; Stop pauses its pump. Attachments remain references in this Worker's exact saved branch; a parked-branch task waits for an explicit route return.").FontSize(11)
			}
			ui.Row(c).Gap(8).Children(func() {
				ui.Button(c, "Earlier events").Disabled(p.busy || p.polling || p.transcript.First == 0).OnClick(p.older)
				ui.Button(c, "Latest events").Disabled(p.busy || p.polling).OnClick(p.loadTail)
				ui.Button(c, "Attach files").Disabled(p.busy || p.inputBlocked()).OnClick(p.upload)
			})
			ui.Row(c).Gap(6).Children(func() {
				for _, branch := range p.detail.Branches {
					branch := branch
					label := "Use " + branch.Target.EngineID + " · " + branch.Target.ModelID
					if branch.ID == p.detail.Session.ActiveBranchID {
						label = "Active: " + branch.Target.EngineID
					}
					ui.Button(c, label).Disabled(p.busy || p.inputBlocked() || branch.ID == p.detail.Session.ActiveBranchID).OnClick(func() { p.switchBranch(branch.Target) })
				}
			})
			ui.Row(c).Gap(6).Children(func() {
				for _, target := range p.targets {
					target := target
					active := false
					for _, branch := range p.detail.Branches {
						if branch.ID == p.detail.Session.ActiveBranchID && hub.RouteKey(branch.Target) == hub.RouteKey(target) {
							active = true
						}
					}
					if !active {
						ui.Button(c, "Switch to "+target.EngineID).Disabled(p.busy || p.inputBlocked()).OnClick(func() { p.switchBranch(target) })
					}
				}
			})
			if len(p.targets) > 1 {
				ui.Text(c, "Automatic fallback is disabled unless explicitly approved below. It is never used after possible side effects. Account/billing route is part of the approval.").FontSize(11)
				ui.Row(c).Gap(6).Children(func() {
					for _, target := range p.targets {
						target := target
						ui.Button(c, fmt.Sprintf("Approve fallback: %s · %s · %s", target.EngineID, target.AccountSource, target.BillingRoute)).Disabled(p.busy || p.inputBlocked()).OnClick(func() { p.setFallback(&target) })
					}
					ui.Button(c, "Disable automatic fallback").Disabled(p.busy || p.inputBlocked()).OnClick(func() { p.setFallback(nil) })
				})
				for _, target := range p.detail.Fallback {
					ui.Text(c, "Approved fallback: "+target.EngineID+" · "+target.AccountSource+" · "+target.BillingRoute).FontSize(11)
				}
			}
			for _, attachment := range p.Service.DraftAttachments(p.selected) {
				attachment := attachment
				ui.Row(c).Gap(8).Children(func() {
					ui.Text(c, fmt.Sprintf("File: %s · %d bytes · original saved, not model-delivered", attachment.Info.Name, attachment.Info.Bytes)).Grow(1)
					ui.Button(c.Key("remove-"+attachment.Info.ID), "Remove draft reference").Disabled(p.busy).OnClick(func() { p.Service.RemoveDraftAttachment(p.selected, attachment.Info.ID) })
				})
			}

			ui.Text(c, "Uploads retain original bytes and only send references. Optional media tools process explicit sources after consent; inspect actual derivatives/losses/one-request receipts under Media evidence. No automatic media input, OCR or semantic understanding. Removing draft references does not delete originals.").FontSize(11)
			ui.Text(c, p.model.StatusText()).FontSize(12)
			if p.model.Notice != "" {
				ui.Text(c, p.model.Notice).FontSize(11)
			}
			p.chat.BuildChat(c)
		})
	})
}
