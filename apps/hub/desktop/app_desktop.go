// INPUT: MyGo native UI, shared desktop controls, durable product.HubService and bounded product pages
// OUTPUT: Run, NativePage and formal native workspace with shared history (approval/question cards in the stream) or execution trace, explicit input and advanced panes
// POS: [L7 Hub product] Separate native Hub product entry; no WebView or direct-harness entry rewrite
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/apps/gateway"
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
	layout                         workspaceLayoutState
	layoutPersistence              workspaceLayoutPersistence
	navigation                     workspaceNavigationState
	settingsScope                  workspaceSettingsScopeState
	workspaceAutomationIndex       workspaceAutomationIndexState
	diagnosticLog                  *slog.Logger
	diagnosticFrame                func()
	diagnosticChanged              func()
	Service                        *product.HubService
	developer                      developerPane
	preferences                    preferencesPane
	connections                    modelConnectionsPane
	goal                           workspaceGoalState
	search                         workspaceSearchState
	sessionOps                     sessionOpsState
	companions                     workspaceCompanionState
	resources                      workspaceResourcesState
	voice                          workspaceVoiceState
	composer                       workspaceComposerState
	durableDrafts                  workspaceDraftsState
	engineInteractions             workspaceEngineInteractionsState
	picker                         workspacePickerState
	composerSelections             map[string][]product.ComposerPreparation
	planModes                      map[string]bool
	planLoaded                     map[string]bool
	permissionPolicies             map[string]hub.PermissionPolicy
	homePermissionScope            string
	pendingFirstSend               string
	pendingFirstPrompt             string
	pendingFirstSelections         []product.ComposerPreparation
	pendingFirstReferences         []product.DraftReferenceSelection
	pendingFirstFiles              []product.ComposerFileSelection
	pendingFirstDraft              workspaceDraftSubmission
	composerFiles                  map[string][]product.ComposerFileSelection
	composerFileRecords            map[string]hub.ComposerFile
	composerFilePending            map[string][]workspaceComposerPendingFile
	workspaceFiles                 map[string][]workspaceSelectedFile
	workspaceReferences            map[string][]product.DraftReferenceSelection
	referenceOwner                 *workspaceReferenceOwner
	workspaceReturn                workspaceReturnState
	stopRequests                   map[string]string
	stopRetry                      map[string]bool
	model                          *view.Model
	chat                           *view.NativeView
	transcript                     *product.TranscriptWindow
	transcriptRows                 []view.Message
	transcriptBlocks               map[string]product.Block
	transcriptTools                map[string]transcriptTool
	toolBlocks                     workspaceToolBlocks
	interactionCards               workspaceInteractionCards
	charts                         *transcriptCharts
	trace                          workspaceTraceState
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
	busy, polling                  bool
	generation                     uint64
	window                         *mygo.Window
	openWindow                     func(string) error
	openResourceWindow             func(string, hub.WorkspaceTab) error
	returnResourceWindow           func()
	ctx                            context.Context
	list                           ui.ListState
	targets                        []hub.Target
	approvals                      []security.ApprovalRequest
	approvalSubmitting             map[string]bool
	approvalEpoch                  uint64
	approvalReadProblem            string
	approvalsCurrent               bool
	pane, configDraft              string
	aepDraft                       string
	commandDraft                   string
	toolsDraft                     string
	gatewayCredentials             string
	gatewayGeneration              uint64
	gatewayStatus                  gateway.LocalSnapshot
	gatewayBusy                    bool
	questions                      []interaction.Question
	questionSubmitting             map[string]bool
	questionEpoch                  uint64
	questionReadProblem            string
	questionsCurrent               bool
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
	settingsForm                   settingsFormState
	mcpNotice                      mcpApprovalNotice
	skillForm                      skillFormState
	sessionModelOptions            workspaceSessionModelOptionsState
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
	markdownOptions := modules.NewRegistry().Options()
	chat.SetMarkdownOptions(markdownOptions)
	page := &NativePage{Service: service, model: model, chat: chat, title: "New conversation", drafts: make(map[string]string), ctx: context.Background(), targets: service.AvailableTargets(), updates: make(chan func(), 8), transcript: product.NewTranscriptWindow(0)}
	page.referenceOwner = &workspaceReferenceOwner{service: service}
	page.trace.markdown = markdownOptions
	if err := registerWorkspaceAssets(); err != nil {
		page.problem = err.Error()
	}
	chat.Send = page.send
	chat.Stop = page.stop
	// Only the Message Stream draws chart data blocks; the trace keeps its own
	// options, so trace details (and exports) show the original text.
	page.charts = newTranscriptCharts()
	transcriptOptions := modules.NewRegistry().Options()
	transcriptOptions.Code["chart"] = page.charts.render
	chat.RenderMessage = newWorkspaceTranscriptRenderer(page, transcriptOptions)
	page.observeGateway()
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
	// Notifications belong to the application, not to a window: one observer
	// announces finished replies and pending approvals/questions while no
	// window is in the foreground, and a click selects that session.
	observerCtx, stopObserver := context.WithCancel(context.Background())
	defer stopObserver()
	mygo.App.OnNotificationClick(windows.openAttention)
	mygo.App.WhenReady(func() {
		go newAttentionObserver(service).run(observerCtx, 2*time.Second)
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
	previousPane := p.pane
	if name != p.pane {
		p.closeWorkspacePicker()
	}
	name = p.workspacePaneDestination(name)
	if previousPane != name && name != "" {
		p.finishWorkspaceVoiceForNavigation()
		p.observeWorkspaceResources()
		p.closeWorkspaceResources()
		p.resources.suppressed = true
	}
	if p.pane == "connections" && name != "connections" {
		p.connections.revealAPIKey = false
	}
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
	if name == "" {
		p.resources.suppressed = false
	}
	p.restoreWorkspacePane()
	if previousPane != "" && p.pane == "" && p.workspaceResourcesVisible() {
		p.loadWorkspaceResource()
	}
}
func (p *NativePage) refreshSessions() {
	p.sessionBack = nil
	p.loadSessions(hub.SessionCursor{})
	p.loadPinnedSessions()
}
func (p *NativePage) loadSessions(cursor hub.SessionCursor) {
	if p.sessionLoading {
		return
	}
	p.sessionLoading = true
	query := p.sessionQuery
	go func() {
		page, err := p.Service.Conversations.Page(p.ctx, query, cursor, 64)
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
	p.flushWorkspaceDraft()
	p.pendingFirstSend = ""
	p.pendingFirstPrompt = ""
	p.pendingFirstSelections = nil
	p.pendingFirstReferences = nil
	p.pendingFirstFiles = nil
	p.pendingFirstDraft = workspaceDraftSubmission{}
	p.composer.closePopovers()
	p.closeWorkspacePicker()
	p.finishWorkspaceVoiceForNavigation()
	p.stopPlayback()
	p.observeWorkspaceResources()
	p.drafts[p.selected] = p.model.Draft
	p.selected = id
	p.selectWorkspaceResources(id)
	p.model.Draft = p.drafts[id]
	p.loadWorkspaceDraft()
	p.generation++
	p.resetTranscript()
	p.resetTrace()
	p.pane = ""
	p.configDraft = ""
	p.aepDraft = ""
	p.commandDraft = ""
	p.toolsDraft = ""
	p.shellJobs = nil
	p.approvals = nil
	p.approvalReadProblem = ""
	p.questions = nil
	p.questionReadProblem = ""
	p.approvalsCurrent, p.questionsCurrent = false, false
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
	p.sessionModelOptions = workspaceSessionModelOptionsState{}
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
	optionsEpoch := p.sessionModelOptions.epoch
	p.chat.Busy = true
	p.polling = true
	go func() {
		detail, err := p.Service.Conversations.Detail(p.ctx, id)
		var planSettings hub.Settings
		optionsErr := err
		if err == nil {
			planSettings, err = p.Service.SessionSettings(p.ctx, id)
			optionsErr = err
		}
		var chunk product.TranscriptChunk
		if err == nil {
			chunk, err = p.Service.Transcripts.Tail(p.ctx, id)
		}
		p.dispatch(func() {
			p.polling = false
			if p.selected != id || p.generation != generation {
				p.loadTail()
				return
			}
			if err != nil {
				if optionsErr != nil {
					p.failWorkspaceSessionModelOptions(id, generation, optionsEpoch, optionsErr.Error())
				}
				p.pendingFirstSend = ""
				p.pendingFirstPrompt = ""
				p.problem = err.Error()
				return
			}
			p.detail = detail
			p.acceptWorkspaceNavigationDetail(id, generation)
			p.acceptWorkspaceSessionModelOptions(id, generation, detail.Session.ActiveBranchID, optionsEpoch, planSettings.SessionID, planSettings.SessionModelOptions)
			p.acceptWorkspacePlanSettings(id, detail.Session.ActiveBranchID, planSettings.PlanMode)
			p.acceptWorkspacePermissionSettings(id, detail.Session.ActiveBranchID, planSettings.PermissionPolicy)
			p.chat.Busy = false
			p.transcript.Reset(chunk)
			p.transcript.Trim(true)
			p.projectTranscript()
			p.applyDetail()
			p.chat.ScrollToEnd()
			if p.pendingFirstSend == id {
				prompt := p.pendingFirstPrompt
				selections := p.pendingFirstSelections
				references := p.pendingFirstReferences
				files, draft := p.pendingFirstFiles, p.pendingFirstDraft
				p.pendingFirstSelections = nil
				p.pendingFirstReferences = nil
				p.pendingFirstFiles = nil
				p.pendingFirstDraft = workspaceDraftSubmission{}
				p.pendingFirstSend = ""
				p.pendingFirstPrompt = ""
				p.busy, p.chat.Busy = false, false
				p.sendPreparedFileSnapshot(prompt, selections, references, files, draft)
			}
		})
	}()
}
func (p *NativePage) applyDetail() {
	p.chat.Busy = p.busy || p.detail.Session.Status == engine.Waiting
	p.model.Messages = p.transcriptRows
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
	} else if p.transcript.MoreAfter() {
		p.model.Notice = "Newer committed messages are outside this window. Live text updates are paused; scroll to the bottom or choose Latest to reload them."
	} else {
		p.model.Notice = "Message window: scrolling up loads earlier committed messages. This UI cache is not model context."
	}
}
func (p *NativePage) poll() {
	p.pollEngineInteractions()
	p.observeWorkspaceDraft()
	p.observeMCPApprovalNotice()
	p.observeWorkspaceVoice()
	p.observeWorkspaceResources()
	p.pollWorkspaceCompanions()
	p.observePreferences()
	p.flushWorkspaceLayoutPreferences(false)
	p.observeConnections()
	p.pollDeveloper()
	p.observeGateway()
	p.pollTrace()
	if p.selected == "" || p.polling || p.busy {
		return
	}
	id, generation, after := p.selected, p.generation, p.transcript.Last()
	optionsEpoch := p.sessionModelOptions.epoch
	approvalEpoch, questionEpoch := p.approvalEpoch, p.questionEpoch
	// A window whose newest end was trimmed stops following until reloaded.
	readText := !p.transcript.MoreAfter()
	readWait, waitEpoch := p.pane == "waits" && !p.paneBusy, p.wait.Epoch
	p.polling = true
	go func() {
		detail, err := p.Service.Conversations.Detail(p.ctx, id)
		var planSettings hub.Settings
		optionsErr := err
		if err == nil {
			planSettings, err = p.Service.SessionSettings(p.ctx, id)
			optionsErr = err
		}
		var newer product.TranscriptChunk
		var approvals []security.ApprovalRequest
		var questions []interaction.Question
		var approvalErr, questionErr error
		var waiting product.WaitView
		var waitErr error
		if err == nil && readText {
			newer, err = p.Service.Transcripts.Newer(p.ctx, id, after)
		}
		if err == nil {
			approvals, approvalErr = p.Service.PendingApprovals(p.ctx, id)
			questions, questionErr = p.Service.PendingQuestions(p.ctx, id)
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
				if optionsErr != nil {
					p.failWorkspaceSessionModelOptions(id, generation, optionsEpoch, optionsErr.Error())
				}
				p.approvals = nil
				p.questions = nil
				p.approvalsCurrent, p.questionsCurrent = false, false
				if readWait && p.pane == "waits" && p.wait.Epoch == waitEpoch {
					p.wait.Current = false
					p.wait.Problem = err.Error()
				}
				p.problem = err.Error()
				return
			}
			p.detail = detail
			p.acceptWorkspaceNavigationDetail(id, generation)
			p.acceptWorkspaceSessionModelOptions(id, generation, detail.Session.ActiveBranchID, optionsEpoch, planSettings.SessionID, planSettings.SessionModelOptions)
			p.acceptWorkspacePlanSettings(id, detail.Session.ActiveBranchID, planSettings.PlanMode)
			p.acceptWorkspacePermissionSettings(id, detail.Session.ActiveBranchID, planSettings.PermissionPolicy)
			if p.approvalEpoch == approvalEpoch {
				p.approvals = approvals
				p.approvalReadProblem = ""
				p.approvalsCurrent = approvalErr == nil
				if approvalErr != nil {
					p.approvals = nil
					p.approvalReadProblem = "Pending permissions could not be refreshed: " + approvalErr.Error()
				}
			}
			if p.questionEpoch == questionEpoch {
				p.questions = questions
				p.questionReadProblem = ""
				p.questionsCurrent = questionErr == nil
				if questionErr != nil {
					p.questions = nil
					p.questionReadProblem = "Pending questions could not be refreshed: " + questionErr.Error()
				}
			}
			if readWait && p.pane == "waits" && !p.paneBusy && p.wait.Epoch == waitEpoch {
				if waitErr != nil {
					p.wait.Current = false
					p.wait.Problem = waitErr.Error()
				} else {
					p.applyWaitView(waiting)
				}
			}
			if readText {
				p.transcript.ApplyNewer(newer)
				p.transcript.Trim(p.chat.AtEnd())
				p.projectTranscript()
			} else {
				// Pending requests may change without newer history.
				p.projectInteractionsIfChanged()
			}
			p.applyDetail()
			if p.pendingFirstSend == id {
				prompt, preparations, references := p.pendingFirstPrompt, p.pendingFirstSelections, p.pendingFirstReferences
				files, draft := p.pendingFirstFiles, p.pendingFirstDraft
				p.pendingFirstSend, p.pendingFirstPrompt = "", ""
				p.pendingFirstSelections, p.pendingFirstReferences, p.pendingFirstFiles = nil, nil, nil
				p.pendingFirstDraft = workspaceDraftSubmission{}
				p.busy, p.chat.Busy = false, false
				p.sendPreparedFileSnapshot(prompt, preparations, references, files, draft)
			}
		})
	}()
}
func (p *NativePage) send() {
	p.sendSnapshot(p.model.Draft)
}
func (p *NativePage) sendSnapshot(prompt string) {
	p.sendPreparedSnapshot(prompt, p.captureWorkspaceSelections(), p.captureWorkspaceReferences())
}
func (p *NativePage) sendPreparedSnapshot(prompt string, selections []product.ComposerPreparation, references []product.DraftReferenceSelection) {
	p.sendPreparedFileSnapshot(prompt, selections, references, p.captureWorkspaceComposerFiles(), workspaceDraftSubmission{})
}
func (p *NativePage) sendPreparedFileSnapshot(prompt string, selections []product.ComposerPreparation, references []product.DraftReferenceSelection, files []product.ComposerFileSelection, draft workspaceDraftSubmission) {
	p.traceUI("send")
	if problem := p.workspaceSessionModelOptionsProblem(); problem != "" {
		p.problem = problem
		return
	}
	if p.voice.recorder != nil {
		p.voice.problem = "请先结束录音并确认文字，再发送。"
		return
	}
	frozen := *p.model
	frozen.Draft = prompt
	if p.selected == "" || p.busy || p.chat.Busy || p.detail.Session.Status == engine.Waiting || !frozen.CanSend() || p.workspacePendingFiles() {
		return
	}
	if problem := p.workspaceAttachmentProblem(); problem != "" {
		p.problem = problem
		return
	}
	if p.Service.ComposerDrafts != nil && !draft.valid {
		p.prepareWorkspaceDraftSubmission(func(captured workspaceDraftSubmission) {
			p.sendPreparedFileSnapshot(prompt, selections, references, files, captured)
		})
		return
	}
	// Observations captured before admission must not replace its newer state.
	p.generation++
	if worker, ok := p.selectedWorker(); ok {
		p.sendPreparedWorkerFileDraftRevision(worker, prompt, selections, references, files, draft)
		return
	}
	id := p.selected
	finishReferences := p.referenceOwner.begin()
	if finishReferences == nil {
		return
	}
	p.busy = true
	p.chat.Busy = true
	go func() {
		_, err := p.Service.SubmitComposerFileDraft(p.ctx, id, prompt, selections, references, files)
		var draftErr error
		if err == nil && draft.valid {
			_, draftErr = p.Service.ComposerDrafts.Consume(p.ctx, draft.scope, draft.revision)
			if draftErr == nil && draft.homeRevision != 0 {
				_, draftErr = p.Service.ComposerDrafts.Consume(p.ctx, hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}, draft.homeRevision)
			}
		}
		finishReferences()
		p.dispatch(func() {
			p.busy = false
			p.chat.Busy = false
			clearDraft := false
			if err == nil && draft.valid {
				clearDraft = p.finishWorkspaceDraftSubmission(draft)
			}
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			if !draft.valid && p.model.Draft == prompt {
				p.model.Draft = ""
				delete(p.drafts, id)
			}
			if !draft.valid || clearDraft {
				p.consumeWorkspaceSelections(id, selections)
				p.consumeWorkspaceReferences(id, references)
				p.consumeWorkspaceComposerFiles(id, files)
			}
			p.problem = ""
			if draftErr != nil {
				p.problem = "请求已接收，草稿保全回执失败；请勿重复发送：" + draftErr.Error()
			}
			p.model.ActiveRunID = id
			p.model.Status = view.Running
			p.chat.ScrollToEnd()
			p.poll()
			p.refreshSessions()
		})
	}()
}

// stop is an immediate command; rendered controls pass the identity they observed.
func (p *NativePage) stop() {
	id := p.selected
	executionID, _ := p.Service.Turns.Active(id)
	p.stopExecution(id, executionID)
}
func (p *NativePage) stopExecution(id, executionID string) {
	p.traceUI("stop")
	if id == "" || p.selected != id || executionID == "" || p.busy || p.modelStopping() && !p.stopRetry[id] {
		return
	}
	activeID, active := p.Service.Turns.Active(id)
	if !active || activeID != executionID {
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
			err = p.Service.Teams.StopExecution(p.ctx, lead, team, worker.ID, executionID)
		} else {
			err = p.Service.Turns.Stop(p.ctx, id, executionID)
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
		activeID, active := p.Service.Turns.Active(p.selected)
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
			p.refreshWorkspaceNavigation()
			p.selectSession(session.ID)
		})
	}()
}
func (p *NativePage) upload() {
	if p.selected == "" || p.busy || p.window == nil {
		return
	}
	id := p.selected
	finishReferences := p.referenceOwner.begin()
	if finishReferences == nil {
		return
	}
	p.busy = true
	p.chat.Busy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Title: "Attach original files — 20 MiB each, no automatic model delivery", Multiple: true})
		var references []product.DraftReferenceSelection
		if err == nil {
			for _, path := range paths {
				var attachment product.DraftAttachment
				if attachment, err = p.Service.Upload(p.ctx, id, path); err != nil {
					break
				}
				references = append(references, attachment.DraftSelection())
				p.referenceOwner.retain(id, []product.DraftReferenceSelection{attachment.DraftSelection()})
			}
		}
		finishReferences()
		p.dispatch(func() {
			p.retainWorkspaceReferences(id, references)
			p.busy = false
			p.chat.Busy = false
			if err != nil {
				p.problem = err.Error()
			}
			p.poll()
		})
	}()
}

// older loads earlier committed blocks above the visible rows, which stay put.
func (p *NativePage) older() {
	if p.selected == "" || p.polling || p.transcript.First() == 0 || !p.transcript.MoreBefore() {
		return
	}
	id, generation, before := p.selected, p.generation, p.transcript.First()
	p.polling = true
	go func() {
		chunk, err := p.Service.Transcripts.Older(p.ctx, id, before)
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
			p.transcript.ApplyOlder(chunk)
			p.transcript.Trim(p.chat.AtEnd())
			p.projectTranscript()
			p.applyDetail()
		})
	}()
}
func (p *NativePage) switchBranch(target hub.Target) {
	p.changeWorkspaceBranch(func(id string) (hub.Branch, error) {
		return p.Service.SwitchTarget(p.ctx, id, target, "Explicit native UI selection; retained previous branch")
	})
}
func (p *NativePage) selectSavedBranch(branchID string) {
	p.changeWorkspaceBranch(func(id string) (hub.Branch, error) {
		return p.Service.SelectBranch(p.ctx, id, branchID, "Explicit retained history branch selection")
	})
}
func (p *NativePage) changeWorkspaceBranch(change func(string) (hub.Branch, error)) {
	if p.busy || p.selected == "" {
		return
	}
	id, generation := p.selected, p.generation
	p.composer.closePopovers()
	p.closeWorkspacePicker()
	p.busy = true
	p.problem = ""
	go func() {
		_, err := change(id)
		p.dispatch(func() {
			if p.selected != id || p.generation != generation {
				return
			}
			p.busy = false
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
	id, generation := p.selected, p.generation
	var targets []hub.Target
	if target != nil {
		approved := *target
		approved.AutomaticAuthorized = true
		targets = []hub.Target{approved}
	}
	p.busy = true
	p.problem = ""
	go func() {
		err := p.Service.SetFallback(p.ctx, id, targets)
		p.dispatch(func() {
			if p.selected != id || p.generation != generation {
				return
			}
			p.busy = false
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
	c.OnShortcut(ui.Cmd, ui.KeyK, p.openWorkspaceSearch)
	defer p.buildWorkspaceModelDefaults(c)
	defer p.buildWorkspaceSearch(c)
	defer p.buildSessionRenameDialog(c)
	defer p.buildSessionDeleteDialog(c)
	defer p.buildWorkspaceGoal(c)
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
	p.observeWorkspaceVoice()
	p.restoreWorkspacePane()
	p.observePreferences()
	p.observeConnections()
	p.applyPreferences(c)
	p.buildWorkspaceBody(c)
}

func (p *NativePage) buildWorkspaceBody(c *ui.Context) {
	if p.layout.sidebarResize.dragging || p.resources.resize.dragging {
		// A drag follows the pointer directly; only open/close uses easing.
		c.DisableAnimations()
	}
	if p.isWorkspaceSettings() {
		p.buildWorkspaceSettings(c)
		return
	}
	workspaceShell(c).Children(func() {
		width, _ := c.Size()
		focused := p.workspaceResourcesFullWidth(width)
		p.buildWorkspaceSidebar(c)
		if focused {
			p.buildWorkspaceResources(c, true)
		} else {
			ui.Column(c.Key("workspace-main-column")).Label("Workspace main column").Grow(1).MinWidth(0).FillHeight().
				Gap(workspaceRegionGap).Transition(workspacePaneTransition()).Children(func() {
				p.buildWorkspaceHeader(c)
				workspaceMainSurface(c, "workspace-main", "Workspace main surface").Children(func() {
					p.buildWorkspaceMainContent(c, false)
				})
			})
		}
		if !focused && p.workspaceResourcesVisible() {
			p.buildWorkspaceResources(c, true)
		}
		p.buildWorkspacePaneControls(c)
	})
}

func (p *NativePage) buildWorkspaceMainContent(c *ui.Context, resourceFocus bool) {
	if p.playerBusy {
		ui.Button(c, "暂停本地播放").Label("Pause local video and wait for decoder cleanup").Disabled(p.playerStopping).OnClick(p.stopPlayback)
	}
	if !resourceFocus {
		if p.pane == "companions" {
			ui.Column(c).Grow(1).MinHeight(0).FillWidth().Padding(24).Gap(16).Children(func() { p.buildWorkspaceCompanions(c) })
			return
		}
		if p.buildWorkspaceSessionScope(c) || p.buildWorkspaceAutomationIndex(c) {
			return
		}
		if p.pane == "" && p.selected == "" {
			p.buildWorkspaceWelcome(c)
			return
		}
	}
	body := ui.Column(c).Grow(1).MinHeight(0).FillWidth().Padding(24).Gap(16)
	if p.pane == "" && p.selected != "" {
		body.Padding(0, 24, 12, 24)
	}
	if resourceFocus {
		body.Padding(0, 12, 12, 24)
	}
	body.Children(func() {
		if p.problem != "" {
			ui.Text(c, p.problem).Role(ui.RoleStatus).TextColor(c.Theme().Danger).FontSize(workspaceFontBody)
		}
		// In the conversation, requests are cards in the stream. The trace
		// keeps a one-line entry back to them; panes and resource focus
		// also keep the existing panels as their entry.
		pane := p.workspacePaneBuilder()
		if p.selected != "" && !resourceFocus && (pane != nil || p.trace.active) {
			p.buildInteractionEntry(c)
		}
		if p.selected != "" && !resourceFocus && pane == nil {
			// The panels that show read failures are hidden here.
			p.buildInteractionReadProblems(c)
		}
		if p.selected != "" && (resourceFocus || pane != nil) {
			p.buildApprovals(c)
			p.buildQuestions(c)
		}
		if resourceFocus {
			p.buildWorkspaceResources(c, false)
			return
		}
		if p.selected == "" {
			p.buildWorkspaceWelcome(c)
			return
		}
		if p.buildWorkspacePane(c) {
			return
		}
		ui.Column(c).Grow(1).MinWidth(0).FillWidth().Gap(16).Children(func() {
			if p.trace.active {
				p.buildWorkspaceTrace(c)
				return
			}
			p.buildWorkspaceTranscript(c)
			p.buildInteractionJump(c)
			if len(p.model.Messages) > 0 && !p.chat.AtEnd() {
				ui.Button(c, "Jump to latest").OnClick(p.chat.ScrollToEnd)
			}
			ui.Column(c).FillWidth().MaxWidth(workspaceReadingWidth).AlignSelf(ui.Center).Children(func() {
				p.buildWorkspaceComposer(c)
			})
		})
	})
}
