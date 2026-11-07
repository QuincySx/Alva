// INPUT: Actual native artifact drafts, independent Worker queues and real local SDK execution
// OUTPUT: Reference-only attachment queueing, branch pinning, failure preservation and no cross-owner transfer regressions
// POS: [L7 Hub product] Original attachment bytes stay under the actual Worker native session, never automatically in model context.
package product

import (
	"context"
	"errors"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestWorkerCapturedDraftRejectsForeignUnsupportedChangedAndCancelled(t *testing.T) {
	s, _ := fixture(t)
	lead, err := s.Create(t.Context(), "separate Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(t.Context(), lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.AddWorker(t.Context(), lead.ID, team.Team.ID, "original owner", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	sibling, err := s.AddWorker(t.Context(), lead.ID, team.Team.ID, "different owner", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	file, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: worker.SessionID}, composerFixtureFile(t, "own.txt", []byte("original Worker bytes")))
	if err != nil {
		t.Fatal(err)
	}
	leadFile, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: lead.ID}, composerFixtureFile(t, "lead.txt", []byte("Lead private original")))
	if err != nil {
		t.Fatal(err)
	}
	home, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "unbound-home"}, composerFixtureFile(t, "home.txt", []byte("unadopted original")))
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		worker string
		file   hub.ComposerFile
	}{{sibling.ID, file}, {worker.ID, leadFile}, {worker.ID, home}} {
		if _, err = s.EnqueueComposerWorkerFileDraft(t.Context(), lead.ID, team.Team.ID, test.worker, "keep manual draft", nil, nil, nil, []ComposerFileSelection{SelectedComposerFile(test.file)}); !errors.Is(err, hub.ErrComposerFileChanged) {
			t.Fatal("typed file DTO borrowed another logical session original", test.worker, err)
		}
	}
	drafts := s.Store.(hub.ComposerDraftStore)
	draft, err := drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: file.Scope, Text: "keep manual draft", References: []hub.ComposerDraftReference{SelectedComposerFile(file).DraftSelection()}}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueComposerWorkerFileDraft(t.Context(), lead.ID, team.Team.ID, worker.ID, draft.Text, nil, nil, nil, []ComposerFileSelection{SelectedComposerFile(file)}); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("unsupported file engine admitted Worker task", err)
	}
	if err = s.Runtime.Register("worker-files", composerFileRecordingRuntime{calls: make(chan engine.Request, 1)}); err != nil {
		t.Fatal(err)
	}
	route := s.DefaultTarget
	route.EngineID = "worker-files"
	s.Targets = append(s.Targets, route)
	if _, err = s.Store.Switch(t.Context(), worker.SessionID, route, "explicit file-capable Worker route"); err != nil {
		t.Fatal(err)
	}
	cancelled, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = s.EnqueueComposerWorkerFileDraft(cancelled, lead.ID, team.Team.ID, worker.ID, draft.Text, nil, nil, nil, []ComposerFileSelection{SelectedComposerFile(file)}); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled queue admission committed or hid cancellation", err)
	}
	path, err := s.ComposerFilePrivatePath(t.Context(), SelectedComposerFile(file), false)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, []byte("changed private original"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueComposerWorkerFileDraft(t.Context(), lead.ID, team.Team.ID, worker.ID, draft.Text, nil, nil, nil, []ComposerFileSelection{SelectedComposerFile(file)}); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("changed captured file admitted Worker task", err)
	}
	retained, err := drafts.ComposerDraft(t.Context(), draft.Scope)
	if err != nil || !reflect.DeepEqual(retained, draft) {
		t.Fatal("failed queue admission consumed the durable draft", retained, err)
	}
	queued, err := s.Teams.Store.WorkerTasks(t.Context(), lead.ID, team.Team.ID, worker.ID, 0, 8)
	if err != nil || len(queued.Tasks) != 0 || s.Runtime.HasActive() {
		t.Fatal("rejected file input created execution facts", queued, err)
	}
}

func TestWorkerDraftQueuesOnlyOwnAttachmentReferencesAndUsesPinnedNativeSession(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	lead, err := s.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "attached worker", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	const raw = "PRIVATE_ORIGINAL_BODY_ONLY_NEVER_AUTOMATIC_MODEL_CONTEXT"
	attachment, err := s.UploadReader(ctx, w.SessionID, "worker-input.txt", strings.NewReader(raw))
	if err != nil {
		t.Fatal(err)
	}
	leadAttachment, err := s.UploadReader(ctx, lead.ID, "lead-input.txt", strings.NewReader("separate Lead body"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.EnqueueWorkerDraft(ctx, lead.ID, team.Team.ID, w.ID, strings.Repeat("x", 16<<10), nil, []DraftReferenceSelection{attachment.DraftSelection()}); err == nil {
		t.Fatal("oversize prompt+manifest admitted")
	}
	if len(s.DraftAttachments(w.SessionID)) != 1 {
		t.Fatal("failed queue discarded attachment draft")
	}
	foreign, err := s.Create(ctx, "foreign Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.EnqueueWorkerDraft(ctx, foreign.ID, team.Team.ID, w.ID, "wrong owner", nil, nil); err == nil {
		t.Fatal("foreign Lead consumed Worker draft")
	}
	otherObserver, err := s.UploadReader(ctx, w.SessionID, "other-observer.txt", strings.NewReader("other observer original"))
	if err != nil {
		t.Fatal(err)
	}
	task, err := s.EnqueueWorkerDraft(ctx, lead.ID, team.Team.ID, w.ID, "inspect only explicitly needed ranges", []hub.ResourceClaim{{Path: ".", Write: false}}, []DraftReferenceSelection{attachment.DraftSelection()})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(task.Scope, raw) || strings.Contains(task.Scope, leadAttachment.Reference.URI) || strings.Contains(task.Scope, otherObserver.Reference.URI) || !strings.Contains(task.Scope, attachment.Reference.URI) || task.RequiredBranchID != attachment.BranchID {
		t.Fatal("body/foreign reference leaked or branch pin missing", task)
	}
	if len(s.DraftAttachments(w.SessionID)) != 1 || s.DraftAttachments(w.SessionID)[0] != otherObserver || len(s.DraftAttachments(lead.ID)) != 1 || len(requests) != 0 {
		t.Fatal("enqueue executed or consumed another draft")
	}
	if err := s.Teams.Resume(ctx, lead.ID, team.Team.ID, w.ID); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		page, err := s.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, w.ID, 0, 32)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Tasks) == 1 && page.Tasks[0].Status == engine.Succeeded && !s.Runtime.HasActive() {
			break
		}
		time.Sleep(time.Millisecond)
	}
	select {
	case request := <-requests:
		if request.OwnerID != w.SessionID || request.ResumeSession != attachment.Info.SessionID || strings.Contains(request.Prompt, raw) || !strings.Contains(request.Prompt, attachment.Reference.URI) {
			t.Fatal("actual execution lost original native reference binding", request)
		}
	default:
		t.Fatal("actual SDK Worker did not execute")
	}
	info, data, err := s.Repository.ReadArtifactFull(ctx, w.SessionID, attachment.Info.SessionID, attachment.Reference.URI, 4096)
	if err != nil || string(data) != raw || info.SHA256 != attachment.Info.SHA256 {
		t.Fatal("original bytes changed", err)
	}
}
