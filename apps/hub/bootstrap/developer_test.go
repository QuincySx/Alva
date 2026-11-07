// INPUT: Real Hub bootstrap, explicit primary/extra model routes and a blocked local HTTP request.
// OUTPUT: Cold developer assembly, independent job admission and application Close canceling/joining actual model IO.
// POS: [L7 Hub assembly testing] Developer work neither creates chat sessions nor borrows external engine accounts.
package bootstrap

import (
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/developer"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestDeveloperAssemblyIsColdAndApplicationCloseJoinsItsActualRequest(t *testing.T) {
	var calls atomic.Int32
	entered, closed := make(chan struct{}, 1), make(chan struct{}, 1)
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		_, _ = io.Copy(io.Discard, r.Body)
		if r.Header.Get("Authorization") != "Bearer primary-key" || r.URL.Path != "/primary" {
			t.Error("developer request escaped its selected route/account")
		}
		entered <- struct{}{}
		select {
		case <-r.Context().Done():
			closed <- struct{}{}
		case <-release:
		}
	}))
	defer func() { close(release); server.Close() }()
	a, err := Open(t.Context(), Config{OpenStores: testStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/primary", Model: "primary", APIKey: "primary-key"}, DeveloperRoutes: []developer.Route{{ID: "extra", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/extra", Model: "extra", APIKey: "extra-key"}}}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	if calls.Load() != 0 || a.Service.Developer == nil || len(a.Service.Developer.Targets()) != 2 {
		t.Fatal("Open called a model or lost explicit developer routes")
	}
	sessions, err := a.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 {
		t.Fatal(sessions, err)
	}
	expected := "answer"
	request := developer.Request{Mode: developer.Evaluate, Targets: []string{"configured", "extra"}, Repeats: 1, Dataset: &developer.Dataset{ID: "shutdown", Version: "v1", Tasks: []developer.Task{{ID: "task", Prompt: "Reply answer", ExpectedText: &expected, Weight: 1}}}}
	plan, err := a.Service.Developer.Plan(request)
	if err != nil || plan.TotalPlannedCalls != 2 || calls.Load() != 0 {
		t.Fatal(plan, err)
	}
	if err = a.Service.Developer.Start(request); err != nil {
		t.Fatal(err)
	}
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("no actual developer request")
	}
	if a.Runtime.HasActive() {
		t.Fatal("developer job became a chat turn")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err = a.Close(ctx); err != nil {
		t.Fatal(err)
	}
	select {
	case <-closed:
	case <-time.After(time.Second):
		t.Fatal("Application closed without canceling actual model request")
	}
	result := a.Service.Developer.Snapshot()
	if result.State != "canceled" || result.StartedCalls != 1 || calls.Load() != 1 || result.Report.Reports[0].AttemptedCalls != 1 || result.Report.Scores[0].WeightedScore != nil {
		t.Fatal(result, calls.Load())
	}
}
