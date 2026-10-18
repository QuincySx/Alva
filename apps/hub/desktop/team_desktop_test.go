// INPUT: MyGo headless input Tester and actual durable product service without any engine
// OUTPUT: Collaboration home no-start and explicit route-approved Worker creation regressions
// POS: [L7 Hub product] Native interaction contract tests; not a substitute for actual OS window acceptance
//go:build desktop

package hubdesktop

import (
	"context"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/egoist/mygo/ui"
	"strings"
	"testing"
	"time"
)

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
	page.selected = w.SessionID
	page.detail, err = service.Store.Detail(ctx, w.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	page.team = team
	tester := ui.NewTester(page.Build, 1400, 1200)
	if err := tester.Click("Attach files"); err != nil {
		t.Fatal("Worker attach control remains disabled", err)
	}
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
	tester := ui.NewTester(page.Build, 1240, 850)
	if err = tester.Click("Collaboration"); err != nil {
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
	tester := ui.NewTester(page.Build, 1240, 850)
	if !tester.HasText("Allow once") || !tester.HasText("Permission required: hub_team · ") {
		t.Fatal("permission hidden behind collaboration pane")
	}
}
