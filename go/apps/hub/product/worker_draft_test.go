// INPUT: Actual native artifact drafts, independent Worker queues and real local SDK execution
// OUTPUT: Reference-only attachment queueing, branch pinning, failure preservation and no cross-owner transfer regressions
// POS: [L7 Hub product] Original attachment bytes stay under the actual Worker native session, never automatically in model context.
package product

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/integration/engine"
)

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
	if _, err := s.EnqueueWorkerDraft(ctx, lead.ID, team.Team.ID, w.ID, strings.Repeat("x", 16<<10), nil); err == nil {
		t.Fatal("oversize prompt+manifest admitted")
	}
	if len(s.DraftAttachments(w.SessionID)) != 1 {
		t.Fatal("failed queue discarded attachment draft")
	}
	foreign, err := s.Create(ctx, "foreign Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.EnqueueWorkerDraft(ctx, foreign.ID, team.Team.ID, w.ID, "wrong owner", nil); err == nil {
		t.Fatal("foreign Lead consumed Worker draft")
	}
	task, err := s.EnqueueWorkerDraft(ctx, lead.ID, team.Team.ID, w.ID, "inspect only explicitly needed ranges", []hub.ResourceClaim{{Path: ".", Write: false}})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(task.Scope, raw) || strings.Contains(task.Scope, leadAttachment.Reference.URI) || !strings.Contains(task.Scope, attachment.Reference.URI) || task.RequiredBranchID != attachment.BranchID {
		t.Fatal("body/foreign reference leaked or branch pin missing", task)
	}
	if len(s.DraftAttachments(w.SessionID)) != 0 || len(s.DraftAttachments(lead.ID)) != 1 || len(requests) != 0 {
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
