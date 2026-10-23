// INPUT: MyGo native UI, shared desktop controls, durable product.HubService and bounded product pages
// OUTPUT: Run, NativePage and formal native workspace with shared history, explicit input and advanced panes
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
	preferences                    preferencesPane
	composer                       workspaceComposerState
	pendingFirstSend               string
	pendingFirstPrompt             string
	workspaceFiles                 map[string][]workspaceSelectedFile
	workspaceReturn                workspaceReturnState
	stopRequests                   map[string]string
	stopRetry                      map[string]bool
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
	if err := registerWorkspaceAssets(); err != nil {
		page.problem = err.Error()
	}
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
	name = p.workspacePaneDestination(name)
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
	p.restoreWorkspacePane()
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
	p.pendingFirstSend = ""
	p.pendingFirstPrompt = ""
	p.composer.closePopovers()
	p.stopPlayback()
	p.drafts[p.selected] = p.model.Draft
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
	if p.selected == "" || p.polling || p.busy {
		return
	}
	id, generation := p.selected, p.generation
	p.chat.Busy = true
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
				p.pendingFirstSend = ""
				p.pendingFirstPrompt = ""
				p.problem = err.Error()
				return
			}
			p.detail = detail
			p.chat.Busy = false
			p.transcript.Replace(entries)
			p.reviewing = false
			p.applyDetail()
			p.chat.ScrollToEnd()
			if p.pendingFirstSend == id {
				prompt := p.pendingFirstPrompt
				p.pendingFirstSend = ""
				p.pendingFirstPrompt = ""
				p.sendSnapshot(prompt)
			}
		})
	}()
}
func (p *NativePage) applyDetail() {
	p.chat.Busy = p.busy || p.detail.Session.Status == engine.Waiting
	p.model.Messages = p.transcript.Rows
	p.model.Error = p.problem
	wasStopping := p.modelStopping()
	if p.stopRequests[p.selected] != "" && !wasStopping {
		delete(p.stopRequests, p.selected)
		delete(p.stopRetry, p.selected)
	}
	p.model.Status = view.Status(p.detail.Session.Status)
	if p.detail.Session.Status == hub.Running {
		p.model.ActiveRunID = p.detail.Session.ID
		p.model.Status = view.Running
		if wasStopping {
			p.model.Status = view.Stopping
		}
	} else {
		p.model.ActiveRunID = ""
		delete(p.stopRequests, p.selected)
		delete(p.stopRetry, p.selected)
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
	p.observePreferences()
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
	p.sendSnapshot(p.model.Draft)
}
func (p *NativePage) sendSnapshot(prompt string) {
	p.traceUI("send")
	frozen := *p.model
	frozen.Draft = prompt
	if p.selected == "" || p.busy || p.chat.Busy || p.detail.Session.Status == engine.Waiting || !frozen.CanSend() || p.workspacePendingFiles() {
		return
	}
	if problem := p.workspaceAttachmentProblem(); problem != "" {
		p.problem = problem
		return
	}
	// Observations captured before admission must not replace its newer state.
	p.generation++
	if worker, ok := p.selectedWorker(); ok {
		p.sendWorkerDraft(worker, prompt)
		return
	}
	id := p.selected
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
			if p.model.Draft == prompt {
				p.model.Draft = ""
				delete(p.drafts, id)
			}
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
	if p.selected == "" || p.busy || p.modelStopping() && !p.stopRetry[p.selected] {
		return
	}
	id := p.selected
	executionID, active := p.Service.Runtime.ActiveExecution(id)
	if !active || executionID == "" {
		p.poll()
		return
	}
	if p.stopRequests == nil {
		p.stopRequests = make(map[string]string)
	}
	p.stopRequests[id] = executionID
	delete(p.stopRetry, id)
	p.generation++
	worker, isWorker := p.selectedWorker()
	lead, team := p.team.Team.LeadSessionID, p.team.Team.ID
	p.model.Status = view.Stopping
	go func() {
		var err error
		if isWorker {
			err = p.Service.Teams.Stop(p.ctx, lead, team, worker.ID)
		} else {
			err = p.Service.Runtime.CancelExecution(p.ctx, id, executionID)
		}
		p.dispatch(func() {
			if p.stopRequests[id] != executionID {
				return
			}
			if err != nil {
				if p.stopRetry == nil {
					p.stopRetry = make(map[string]bool)
				}
				p.stopRetry[id] = true
			}
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
func (p *NativePage) modelStopping() bool {
	if executionID := p.stopRequests[p.selected]; executionID != "" {
		activeID, active := p.Service.Runtime.ActiveExecution(p.selected)
		return !active || activeID == executionID
	}
	return p.model.Status == view.Stopping
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
	p.composer.closePopovers()
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
	p.restoreWorkspacePane()
	p.observePreferences()
	p.applyPreferences(c)
	if p.isWorkspaceSettings() {
		p.buildWorkspaceSettings(c)
		return
	}
	ui.Row(c).Fill().Gap(0).Children(func() {
		p.buildWorkspaceSidebar(c)
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().Gap(0).Children(func() {
			p.buildWorkspaceHeader(c)
			if p.playerBusy {
				ui.Button(c, "暂停本地播放").Label("Pause local video and wait for decoder cleanup").Disabled(p.playerStopping).OnClick(p.stopPlayback)
			}
			if p.pane == "" && p.selected == "" {
				p.buildWorkspaceWelcome(c)
				return
			}
			ui.Column(c).Grow(1).MinHeight(0).FillWidth().Padding(24).Gap(16).Children(func() {
				if p.problem != "" {
					ui.Text(c, p.problem).Role(ui.RoleStatus).TextColor(c.Theme().Danger).FontSize(14)
				}
				if p.selected == "" {
					p.buildWorkspaceWelcome(c)
					return
				}
				p.buildApprovals(c)
				p.buildQuestions(c)
				if p.buildWorkspacePane(c) {
					return
				}
				ui.Column(c).Grow(1).MinHeight(0).FillWidth().AlignItems(ui.Center).Children(func() {
					ui.Column(c).FillWidth().MaxWidth(960).Grow(1).MinHeight(0).Gap(16).Children(func() {
						p.chat.BuildMessages(c)
						if len(p.model.Messages) > 0 && !p.chat.AtEnd() {
							ui.Button(c, "Jump to latest").OnClick(p.chat.ScrollToEnd)
						}
						p.buildWorkspaceComposer(c)
					})
				})
			})
		})
	})
}
