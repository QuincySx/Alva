// INPUT: testing, reflect, MyGo headless Tester and durable Hub service fixtures
// OUTPUT: Native Hub startup, terminal ownership and sidebar freshness without changing page/cursor regressions
// POS: [L7 Hub product] No window launch, external model or subprocess execution
//go:build desktop

package hubdesktop

import (
	"context"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/egoist/mygo/ui"
)

func fixture(t *testing.T) (*product.HubService, chan engine.Request) {
	t.Helper()
	ctx := context.Background()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := productstore.Open(ctx, filepath.Join(directory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	repo, err := session.Open(ctx, filepath.Join(directory, "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	runtime := hub.New(store)
	t.Cleanup(func() { _ = runtime.Close(context.Background()) })
	target := hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "no-billing"}
	service, err := product.NewHubService(store, runtime, repo, target, directory, true)
	if err != nil {
		t.Fatal(err)
	}
	// No adapter is needed: these tests must never execute a model.
	return service, make(chan engine.Request, 1)
}

func TestNativeHubHomeDoesNotCreateOrExecute(t *testing.T) {
	service, requests := fixture(t)
	page := NewNativePage(service)
	tester := ui.NewTester(page.Build, 1240, 850)
	if !tester.HasText("Create or select a durable session. Nothing executes on startup.") || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("startup executed or hid actual mode")
	}
	sessions, err := service.Store.ListSessions(context.Background(), 128)
	if err != nil || len(sessions) != 0 {
		t.Fatal("UI build created a session")
	}
}
func TestNativeHubUsesSharedComposerAndTruthfulStop(t *testing.T) {
	service, _ := fixture(t)
	session, err := service.Create(context.Background(), "native", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(context.Background(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1240, 850)
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("draft only")
	tester.Key(0, ui.KeyEnter)
	if service.Runtime.HasActive() || page.model.Draft != "draft only\n" {
		t.Fatal("typing started execution")
	}
	page.detail.Session.Status = hub.Running
	page.model.Status = view.Stopping
	page.applyDetail()
	if page.model.Status != view.Stopping || page.model.ActiveRunID == "" {
		t.Fatal("poll falsely acknowledged cancellation")
	}
}

func TestSidebarUsesFreshSelectedStatusWithoutRepaging(t *testing.T) {
	service, requests := fixture(t)
	for _, test := range []struct {
		name          string
		status        engine.Outcome
		detailID      string
		listUpdated   int64
		detailUpdated int64
		want          engine.Outcome
	}{
		{"completed", engine.Succeeded, "selected", 10, 20, engine.Succeeded},
		{"failed", engine.Failed, "selected", 10, 20, engine.Failed},
		{"cancelled", engine.Cancelled, "selected", 10, 20, engine.Cancelled},
		{"interrupted", engine.Interrupted, "selected", 10, 20, engine.Interrupted},
		{"newer-list-stays-running", engine.Succeeded, "selected", 30, 20, hub.Running},
		{"foreign-detail-cannot-change-row", engine.Succeeded, "other", 10, 20, hub.Running},
	} {
		t.Run(test.name, func(t *testing.T) {
			page := NewNativePage(service)
			page.selected = "selected"
			page.sessions = []hub.Session{{ID: "selected", Title: "Selected conversation", Status: hub.Running, UpdatedAt: test.listUpdated}, {ID: "other", Title: "Other conversation", Status: engine.Failed, UpdatedAt: 9}}
			page.detail.Session = hub.Session{ID: test.detailID, Status: test.status, UpdatedAt: test.detailUpdated}
			page.sessionCursor = hub.SessionCursor{Updated: 42, ID: "retained-cursor"}
			page.sessionPage = hub.SessionPage{Sessions: page.sessions, Next: hub.SessionCursor{Updated: 9, ID: "other"}, HasMore: true}
			page.sessionBack = []hub.SessionCursor{{Updated: 60, ID: "previous-page"}}
			rows := append([]hub.Session(nil), page.sessions...)
			cursor, next := page.sessionCursor, page.sessionPage.Next
			back := append([]hub.SessionCursor(nil), page.sessionBack...)
			page.applyDetail()
			tester := ui.NewTester(page.Build, 1240, 850)
			if !tester.HasText("Selected conversation · "+string(test.want)) || !tester.HasText("Other conversation · failed") {
				t.Fatal("sidebar contradicted the freshest selected session detail or changed another row")
			}
			if !reflect.DeepEqual(page.sessions, rows) || page.sessionCursor != cursor || page.sessionPage.Next != next || !reflect.DeepEqual(page.sessionBack, back) {
				t.Fatal("status rendering mutated page ordering/cursors or required a fresh query")
			}
			if service.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("status observation executed an engine")
			}
		})
	}
}
