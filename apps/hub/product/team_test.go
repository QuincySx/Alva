// INPUT: Actual durable product/native store fixture and local Alva harness
// OUTPUT: Explicit Team dispatch, observer detach independence and real FIFO terminal regressions
// POS: [L7 Hub product] Product Worker pumps are not native SDK subagents or automatic startup replay
package product

import (
	"context"
	"github.com/QuincySx/Alva/integration/engine"
	"strings"
	"testing"
	"time"
)

func TestTeamActualWorkerQueueAndAdmissionContextDetach(t *testing.T) {
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
	if _, err = s.AddWorker(ctx, lead.ID, team.Team.ID, "unapproved", s.DefaultTarget, false); err == nil {
		t.Fatal("Worker billing route silently approved")
	}
	worker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "worker", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	for _, prompt := range []string{"first queued", "second queued"} {
		if _, err = s.Teams.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, prompt, nil); err != nil {
			t.Fatal(err)
		}
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("queueing started execution")
	}
	admission, cancel := context.WithCancel(ctx)
	if err = s.Teams.Resume(admission, lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	cancel()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		page, err := s.Teams.Store.WorkerTasks(ctx, lead.ID, team.Team.ID, worker.ID, 0, 32)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Tasks) == 2 && page.Tasks[1].Status == engine.Succeeded && !s.Runtime.HasActive() {
			if page.Tasks[0].Outcome == nil || page.Tasks[0].Outcome.TurnID == page.Tasks[1].Outcome.TurnID {
				t.Fatal("not separate actual Turns")
			}
			first := <-requests
			second := <-requests
			if first.OwnerID != worker.SessionID || second.OwnerID != worker.SessionID || second.ResumeSession == "" {
				t.Fatal("Worker owner/native continuity lost")
			}
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("actual Worker FIFO did not terminate after observer detached")
}
func TestUploadReaderIsByteOnlyBoundedAndDoesNotAssertProcessing(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "bytes", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"../private", "a/b", "a\\b", "nul\x00name"} {
		if _, err = s.UploadReader(ctx, p.ID, name, strings.NewReader("bytes")); err == nil {
			t.Fatalf("unsafe multipart name accepted: %q", name)
		}
	}
	a, err := s.UploadReader(ctx, p.ID, "xfile0.txt", strings.NewReader("original input"))
	if err != nil || a.Info.Bytes != 14 || !strings.Contains(a.Info.Processing, "No compression") {
		t.Fatal("byte uploader changed original or processing", err)
	}
	if _, err = s.UploadReader(ctx, p.ID, "too-large.txt", strings.NewReader(strings.Repeat("x", (20<<20)+1))); err == nil {
		t.Fatal("reader exceeded 20 MiB")
	}
	if len(s.DraftAttachments(p.ID)) != 1 || s.Runtime.HasActive() {
		t.Fatal("failed upload altered draft or ran model")
	}
}
