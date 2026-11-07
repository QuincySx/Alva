// INPUT: MyGo headless input Tester, actual durable product services and a controlled local execution owner
// OUTPUT: Collaboration no-start, explicit Worker creation and real Stop/join queue-retention regressions
// POS: [L7 Hub product] Native interaction contract tests; not a substitute for actual OS window acceptance
//go:build desktop

package hubdesktop

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/egoist/mygo/ui"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

func TestWorkerWaitingReopenPreservesTaskAndRequiresSameTurnContinuation(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		delta := map[string]any{"content": "explicit Worker answer"}
		reason := "stop"
		if n == 1 {
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "worker-question", "type": "function", "function": map[string]string{"name": "ask_human", "arguments": `{"header":"Worker clarification","question":"Confirm the original Worker task"}`}}}}
			reason = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": reason}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	config := bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "worker-wait-fixture"}}
	app, err := bootstrap.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = app.Close(context.Background()) }()
	lead, err := app.Service.Create(t.Context(), "waiting Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := app.Service.Teams.Store.CreateTeam(t.Context(), lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := app.Service.AddWorker(t.Context(), lead.ID, team.Team.ID, "waiting Worker", app.Service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	if err = app.Service.SetToolEnabled(t.Context(), worker.SessionID, "interaction", true, false); err != nil {
		t.Fatal(err)
	}
	if err = app.Service.EnableDurableWaits(t.Context(), worker.SessionID, true); err != nil {
		t.Fatal(err)
	}
	first, err := app.Service.Teams.Store.EnqueueWorker(t.Context(), lead.ID, team.Team.ID, worker.ID, "ask before continuing first task", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = app.Service.Teams.Store.EnqueueWorker(t.Context(), lead.ID, team.Team.ID, worker.ID, "second stays in FIFO", nil); err != nil {
		t.Fatal(err)
	}
	if err = app.Service.Teams.Resume(t.Context(), lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	var waiting product.WaitView
	for time.Now().Before(deadline) {
		waiting, err = app.Service.Waits(t.Context(), worker.SessionID)
		if err == nil && waiting.Status == engine.Waiting && !app.Service.Teams.Status(worker.ID).Active && !app.Runtime.HasActive() {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if err != nil || waiting.Status != engine.Waiting || calls.Load() != 1 {
		t.Fatal("Worker failed to suspend its original task", waiting, err)
	}
	if err = app.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	app, err = bootstrap.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	page, err := app.Service.Teams.Store.WorkerTasks(t.Context(), lead.ID, team.Team.ID, worker.ID, 0, 8)
	if err != nil || len(page.Tasks) != 2 || page.Tasks[0].ID != first.ID || page.Tasks[0].Status != hub.Running || page.Tasks[0].TurnID != waiting.TurnID || page.Tasks[1].Status != hub.TaskQueued || calls.Load() != 1 || app.Runtime.HasActive() {
		t.Fatal("restart replayed or settled waiting Worker task", page, err)
	}
	waiting, err = app.Service.Waits(t.Context(), worker.SessionID)
	if err != nil || len(waiting.Snapshot.State.Requests) != 1 {
		t.Fatal("reopen lost actual durable question", waiting, err)
	}
	question := waiting.Snapshot.State.Requests[0]
	text := "confirmed original Worker task"
	answer, _ := json.Marshal(interaction.Answer{Text: &text})
	if err = app.Service.ResolveWait(t.Context(), worker.SessionID, product.WaitResolution{Target: product.WaitTarget{BranchID: waiting.BranchID, TurnID: waiting.TurnID, Revision: waiting.Snapshot.Revision}, RequestID: question.ID, Binding: question.Binding, Result: answer}); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 {
		t.Fatal("saving Worker answer resumed execution")
	}
	waiting, err = app.Service.Waits(t.Context(), worker.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	turn, err := app.Service.ResumeWait(t.Context(), worker.SessionID, product.WaitTarget{BranchID: waiting.BranchID, TurnID: waiting.TurnID, Revision: waiting.Snapshot.Revision})
	if err != nil || turn != waiting.TurnID {
		t.Fatal("explicit Worker continuation created another Turn", turn, err)
	}
	page = waitNativeWorkerTask(t, app, lead.ID, team.Team.ID, worker.ID, engine.Succeeded)
	if calls.Load() != 2 || page.Tasks[0].TurnID != waiting.TurnID || page.Tasks[1].Status != hub.TaskQueued {
		t.Fatal("same-Turn resume automatically consumed FIFO successor", page, calls.Load())
	}
	if err = app.Service.Teams.Resume(t.Context(), lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	deadline = time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		page, err = app.Service.Teams.Store.WorkerTasks(t.Context(), lead.ID, team.Team.ID, worker.ID, 0, 8)
		if err == nil && page.Tasks[1].Status == engine.Succeeded && !app.Service.Teams.Status(worker.ID).Active && !app.Runtime.HasActive() {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if err != nil || page.Tasks[1].Status != engine.Succeeded || calls.Load() != 3 || page.Tasks[1].TurnID == waiting.TurnID {
		t.Fatal("explicit FIFO resume did not consume only next task", page, calls.Load(), err)
	}
}

func TestNativeWorkerCapturedFilesAndSkillReopenExplicitFIFO(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(io.LimitReader(r.Body, 2<<20))
		if err != nil {
			t.Error(err)
		}
		calls.Add(1)
		if !bytes.Contains(body, []byte("worker document derivative")) || !bytes.Contains(body, []byte("Worker guide metadata")) || bytes.Contains(body, []byte("LEAD_PRIVATE_ORIGINAL")) {
			t.Error("actual model input lost selected Worker sources or borrowed Lead content")
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"worker complete\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer upstream.Close()
	config := bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "worker-file-fixture"}}
	app, err := bootstrap.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = app.Close(context.Background()) }()
	workspace := t.TempDir()
	guide := filepath.Join(workspace, product.ManagedSkillDirectory, "worker-guide")
	if err = os.MkdirAll(guide, 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(guide, "SKILL.md"), []byte("---\nname: worker-guide\ndescription: Worker guide metadata\n---\nWorker instructions.\n"), 0600); err != nil {
		t.Fatal(err)
	}
	source := nativeWorkerDocument(t, workspace)
	lead, err := app.Service.Create(t.Context(), "file Lead", workspace)
	if err != nil {
		t.Fatal(err)
	}
	team, err := app.Service.Teams.Store.CreateTeam(t.Context(), lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := app.Service.AddWorker(t.Context(), lead.ID, team.Team.ID, "file worker", app.Service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	team, err = app.Service.Teams.Store.Team(t.Context(), lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(app.Service)
	pageCtx, detach := context.WithCancel(t.Context())
	defer detach()
	page.ctx = pageCtx
	tester := ui.NewTester(workspaceStaticView(page), 1240, 950)
	page.selectSession(worker.SessionID)
	waitWorkspace(t, page, tester, func() bool {
		draft := page.workspaceDraftEntry(page.workspaceComposerDraftScope())
		return !page.polling && page.detail.Session.ID == worker.SessionID && draft.known && !draft.loading
	})
	page.team = team
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("manual Worker task")
	page.captureWorkspaceFilePaths([]string{source})
	waitWorkspace(t, page, tester, func() bool { return !page.busy && len(page.composerFiles[worker.SessionID]) == 1 })
	selection := page.composerFiles[worker.SessionID][0]
	file := page.composerFileRecords[selection.ID]
	if file.Scope.Kind != "session" || file.Scope.ID != worker.SessionID || file.TextKey == "" || len(page.composerFiles[lead.ID]) != 0 || page.model.Draft != "manual Worker task" {
		t.Fatal("Worker capture borrowed Lead scope or discarded document/text", file, page.problem)
	}
	prepared, err := app.Service.PrepareComposerHomeSelection(t.Context(), workspace, product.ComposerSelectionRequest{EntryID: "skill:worker-guide"})
	if err != nil {
		t.Fatal(err)
	}
	page.composerSelections = map[string][]product.ComposerPreparation{worker.SessionID: {prepared}}
	if err = app.Service.Teams.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	page.sendSnapshot("duplicate click must not enqueue")
	waitWorkspace(t, page, tester, func() bool { return !page.busy })
	queued, err := app.Service.Teams.Store.WorkerTasks(t.Context(), lead.ID, team.Team.ID, worker.ID, 0, 8)
	if err != nil || len(queued.Tasks) != 1 || queued.Tasks[0].ComposerInput == nil || len(queued.Tasks[0].ComposerInput.Files) != 1 || len(queued.Tasks[0].ComposerInput.Skills) != 1 || queued.Tasks[0].Scope != "manual Worker task" || page.model.Draft != "" || len(page.composerFiles[worker.SessionID]) != 0 || calls.Load() != 0 {
		t.Fatal("native typed Send lost sources, double queued or executed while dispatch closed", queued, page.problem, err)
	}
	if !strings.Contains(page.problem, "task retained; do not resend") {
		t.Fatal("dispatch failure lost its committed task receipt", page.problem)
	}
	page.referenceOwner.close()
	page.closeWorkspaceResources()
	detach()
	if err = app.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	app, err = bootstrap.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 || app.Runtime.HasActive() {
		t.Fatal("reopen automatically dispatched typed Worker queue")
	}
	if err = app.Service.Teams.Resume(t.Context(), lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	waitNativeWorkerTask(t, app, lead.ID, team.Team.ID, worker.ID, engine.Succeeded)
	if calls.Load() != 1 {
		t.Fatal("typed Worker queue did not execute exactly once", calls.Load())
	}
	history, err := app.Store.History(t.Context(), worker.SessionID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range history {
		if event.Input != "" && (event.Input != "manual Worker task" || len(event.InputFiles) != 1 || len(event.InputSkills) != 1) {
			t.Fatal("augmented file manifest replaced original task history", event)
		}
	}
	settings, err := app.Store.Settings(t.Context(), worker.SessionID)
	if err != nil || settings.SkillsEnabled || settings.MediaEnabled {
		t.Fatal("explicit inputs enabled whole optional capabilities", settings, err)
	}
}

func nativeWorkerDocument(t *testing.T, workspace string) string {
	t.Helper()
	var document bytes.Buffer
	archive := zip.NewWriter(&document)
	entry, err := archive.Create("word/document.xml")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = io.WriteString(entry, `<w:document xmlns:w="urn:w"><w:body><w:p><w:r><w:t>worker document derivative</w:t></w:r></w:p></w:body></w:document>`); err != nil {
		t.Fatal(err)
	}
	if err = archive.Close(); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(workspace, "worker.docx")
	if err = os.WriteFile(path, document.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestNativeWorkerQueuedFileCorruptionFailsBeforeHTTPAndRetainsUnsentDraft(t *testing.T) {
	for _, derivative := range []bool{false, true} {
		t.Run(fmt.Sprint(derivative), func(t *testing.T) {
			var calls atomic.Int32
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				http.Error(w, "corrupted captures must not reach the model", http.StatusInternalServerError)
			}))
			defer upstream.Close()
			app, err := bootstrap.Open(t.Context(), bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "worker-corruption-fixture"}})
			if err != nil {
				t.Fatal(err)
			}
			defer app.Close(context.Background())
			workspace := t.TempDir()
			lead, err := app.Service.Create(t.Context(), "corruption Lead", workspace)
			if err != nil {
				t.Fatal(err)
			}
			team, err := app.Service.Teams.Store.CreateTeam(t.Context(), lead.ID)
			if err != nil {
				t.Fatal(err)
			}
			worker, err := app.Service.AddWorker(t.Context(), lead.ID, team.Team.ID, "capture owner", app.Service.DefaultTarget, true)
			if err != nil {
				t.Fatal(err)
			}
			file, err := app.Service.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: worker.SessionID}, nativeWorkerDocument(t, workspace))
			if err != nil {
				t.Fatal(err)
			}
			selection := product.SelectedComposerFile(file)
			queued, err := app.Service.EnqueueComposerWorkerFileDraft(t.Context(), lead.ID, team.Team.ID, worker.ID, "frozen original task", nil, nil, nil, []product.ComposerFileSelection{selection})
			if err != nil {
				t.Fatal(err)
			}
			if _, err = app.Service.Teams.Store.EnqueueWorker(t.Context(), lead.ID, team.Team.ID, worker.ID, "must remain queued after preparation failure", nil); err != nil {
				t.Fatal(err)
			}
			unsent, err := app.Service.ComposerDrafts.Save(t.Context(), hub.ComposerDraft{Scope: file.Scope, Text: "new manual draft stays unsent", References: []hub.ComposerDraftReference{selection.DraftSelection()}}, 0)
			if err != nil {
				t.Fatal(err)
			}
			path, err := app.Service.ComposerFilePrivatePath(t.Context(), selection, derivative)
			if err != nil {
				t.Fatal(err)
			}
			if err = os.WriteFile(path, []byte("changed private captured bytes"), 0600); err != nil {
				t.Fatal(err)
			}
			if err = app.Service.Teams.Resume(t.Context(), lead.ID, team.Team.ID, worker.ID); err != nil {
				t.Fatal(err)
			}
			page := waitNativeWorkerTask(t, app, lead.ID, team.Team.ID, worker.ID, engine.Failed)
			if len(page.Tasks) != 2 || page.Tasks[0].ID != queued.ID || page.Tasks[0].ComposerInput.Files[0].SHA256 != file.SHA256 || page.Tasks[1].Status != hub.TaskQueued || calls.Load() != 0 {
				t.Fatal("file failure replayed, skipped frozen facts or reached model", page, calls.Load())
			}
			current, err := app.Service.ComposerDrafts.Read(t.Context(), file.Scope)
			if err != nil || current.Text != unsent.Text || current.Revision != unsent.Revision || len(current.References) != 1 || current.References[0] != selection.DraftSelection() {
				t.Fatal("in-flight preparation failure consumed new manual draft", current, err)
			}
		})
	}
}

func waitNativeWorkerTask(t *testing.T, app *bootstrap.Application, lead, team, worker string, outcome engine.Outcome) hub.WorkerTaskPage {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	var page hub.WorkerTaskPage
	var err error
	for time.Now().Before(deadline) {
		page, err = app.Service.Teams.Store.WorkerTasks(t.Context(), lead, team, worker, 0, 8)
		if err == nil && len(page.Tasks) > 0 && page.Tasks[0].Status == outcome && !app.Service.Teams.Status(worker).Active && !app.Runtime.HasActive() {
			return page
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("Worker did not reach actual terminal outcome", page, err)
	return page
}

type nativeWorkerCleanupEngine struct {
	engine.Runtime
	started, cleanup, release chan struct{}
}

func (e nativeWorkerCleanupEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true}
}
func (e nativeWorkerCleanupEngine) Cancel(context.Context, string) error { return nil }
func (e nativeWorkerCleanupEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "native-worker-cleanup"}); err != nil {
		return err
	}
	close(e.started)
	<-ctx.Done()
	close(e.cleanup)
	<-e.release
	return ctx.Err()
}

func TestNativeWorkerStopButtonWaitsForActualOwnerAndPreservesQueue(t *testing.T) {
	service, _ := fixture(t)
	ctx := context.Background()
	adapter := nativeWorkerCleanupEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
	unblock := sync.OnceFunc(func() { close(adapter.release) })
	defer unblock()
	if err := service.Runtime.Register("worker-cleanup", adapter); err != nil {
		t.Fatal(err)
	}
	route := service.DefaultTarget
	route.EngineID = "worker-cleanup"
	service.Targets = append(service.Targets, route)
	lead, err := service.Create(ctx, "Stop Worker Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := service.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := service.AddWorker(ctx, lead.ID, team.Team.ID, "owned worker", route, true)
	if err != nil {
		t.Fatal(err)
	}
	for _, prompt := range []string{"running task", "must stay queued"} {
		if _, err = service.Teams.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, prompt, nil); err != nil {
			t.Fatal(err)
		}
	}
	_, done, err := service.Runtime.StartWorkerTask(ctx, lead.ID, team.Team.ID, worker.ID)
	if err != nil {
		t.Fatal(err)
	}
	<-adapter.started
	page := NewNativePage(service)
	page.selected, page.teamLead, page.teamWorker, page.pane = lead.ID, lead.ID, worker.ID, "team"
	page.detail, err = service.Store.Detail(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.team, err = service.Teams.Store.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 1200)
	if err = tester.Click("Stop Worker and pump"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-adapter.cleanup:
	case <-time.After(3 * time.Second):
		t.Fatal("native Stop did not reach the real owner")
	}
	tester.Frame()
	if !page.paneBusy || !service.Runtime.SessionActive(worker.SessionID) {
		t.Fatal("native Stop claimed completion while Execute still cleaned up", page.problem)
	}
	unblock()
	<-done
	settlePage(t, tester, page)
	tasks, err := service.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, worker.ID, 0, 8)
	if err != nil || len(tasks.Tasks) != 2 || tasks.Tasks[0].Status != engine.Cancelled || tasks.Tasks[1].Status != hub.TaskQueued || page.problem != "" {
		t.Fatal("native Stop lost its actual task/queue outcome", tasks, page.problem, err)
	}
}

func TestNativeWorkerSendConsumesOnlyItsReferenceDraftAfterQueueCommit(t *testing.T) {
	service, requests := fixture(t)
	ctx := context.Background()
	lead, err := service.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := service.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w, err := service.AddWorker(ctx, lead.ID, team.Team.ID, "attached worker", service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	attachment, err := service.UploadReader(ctx, w.SessionID, "actual-worker.txt", strings.NewReader("RAW_ORIGINAL_WORKER_BYTES"))
	if err != nil {
		t.Fatal(err)
	}
	team, err = service.Teams.Store.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	// This fixture owns no engine. Close dispatch admission so the native Send
	// can prove durable queue+draft handoff and truthful Start failure separately.
	if err := service.Teams.Close(ctx); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(page), 1400, 1200)
	page.selectSession(w.SessionID)
	waitWorkspace(t, page, tester, func() bool { return !page.polling && page.detail.Session.ID == w.SessionID })
	page.retainWorkspaceReferences(w.SessionID, []product.DraftReferenceSelection{attachment.DraftSelection()})
	page.team = team
	tester.Frame()
	if err := tester.Click("Add attachments"); err != nil {
		t.Fatal("Worker attach control remains disabled", err)
	}
	if _, found := tester.Find("Attach file"); !found || !tester.HasText("文件、图片或视频…") {
		t.Fatal("Worker attachment popover omitted its actual file action")
	}
	tester.Key(0, ui.KeyEscape)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("inspect the referenced Worker file")
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !page.busy {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if page.busy || page.model.Draft != "" || len(service.DraftAttachments(w.SessionID)) != 0 {
		t.Fatal("successful queue admission failed to hand off draft", page.problem)
	}
	queued, err := service.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, w.ID, 0, 8)
	if err != nil || len(queued.Tasks) != 1 || !strings.Contains(queued.Tasks[0].Scope, attachment.Reference.URI) || strings.Contains(queued.Tasks[0].Scope, "RAW_ORIGINAL_WORKER_BYTES") || queued.Tasks[0].RequiredBranchID != attachment.BranchID {
		t.Fatal("native Send lost branch-bound reference", queued, err)
	}
	if len(requests) != 0 || service.Runtime.HasActive() || !strings.Contains(page.problem, "task retained; do not resend") {
		t.Fatal("dispatch failure fabricated execution or lost durable queue", page.problem)
	}
}

func settlePage(t *testing.T, tester *ui.Tester, page *NativePage) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !page.paneBusy {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("native action did not settle")
}
func TestNativeCollaborationCreatesOnlyExplicitWorkerAndKeepsHomeClean(t *testing.T) {
	service, requests := fixture(t)
	s, err := service.Create(context.Background(), "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = s.ID
	page.detail, err = service.Store.Detail(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	if err = workspaceTestMenu(tester, "协作"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.team.Team.ID != "" || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("observing collaboration created/executed Team")
	}
	if err = tester.Click("Create Team on this Lead"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.team.Team.ID == "" || len(page.team.Workers) != 0 {
		t.Fatal("explicit Team creation missing")
	}
	if err = tester.Click("New Worker name"); err != nil {
		t.Fatal(err)
	}
	tester.Type("native-worker")
	if err = tester.Click("Approve route and add Worker"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if len(page.team.Workers) != 1 || page.team.Workers[0].SessionID == s.ID || service.Runtime.HasActive() {
		t.Fatal("Worker is not a distinct idle session")
	}
	home, err := service.Store.PageSessions(context.Background(), "", hub.SessionCursor{}, 128)
	if err != nil || len(home.Sessions) != 1 {
		t.Fatal("Worker became unrelated home session", err)
	}
}
func TestPendingPermissionVisibleInCollaborationPane(t *testing.T) {
	service, _ := fixture(t)
	s, err := service.Create(context.Background(), "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = s.ID
	page.detail, err = service.Store.Detail(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	page.pane = "team"
	page.teamLead = s.ID
	page.approvals = []security.ApprovalRequest{{ID: "visible", SessionID: "actual-child-or-root", CallID: "call", ToolName: "hub_team"}}
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	if !tester.HasText("Allow once") || !tester.HasText("Permission required: hub_team · ") {
		t.Fatal("permission hidden behind collaboration pane")
	}
}
