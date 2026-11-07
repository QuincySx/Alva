// INPUT: Real bootstrap/private stores, deterministic local HTTP question/model and explicit group continuation controls.
// OUTPUT: Reopened same-round waiting, same-Turn resume, exact-once replies and no replay of previously completed members.
// POS: [L7 Hub UI/product acceptance] All requests stay on a local test server; no external account or OS window is used.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
)

func TestCompanionDurableGroupReopenExplicitSameRoundResumeAndPublication(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		var delta map[string]any
		reason := "stop"
		if n == 1 || n == 4 {
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": fmt.Sprintf("group-question-%d", n), "type": "function", "function": map[string]string{"name": "ask_human", "arguments": `{"header":"Group clarification","question":"Confirm this group input explicitly"}`}}}}
			reason = "tool_calls"
		} else {
			delta = map[string]any{"content": fmt.Sprintf("completed-group-reply-%d", n)}
		}
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": reason}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	config := bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}}
	var app *bootstrap.Application
	open := func() {
		t.Helper()
		var err error
		app, err = bootstrap.Open(t.Context(), config)
		if err != nil {
			t.Fatal(err)
		}
	}
	closeApp := func() {
		t.Helper()
		if app != nil {
			if err := app.Close(context.Background()); err != nil {
				t.Fatal(err)
			}
			app = nil
		}
	}
	defer closeApp()
	open()
	var profiles []hub.CompanionProfile
	for _, name := range []string{"first", "second"} {
		p, err := app.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: name, Target: app.Service.DefaultTarget, Enabled: true}, 0)
		if err != nil {
			t.Fatal(err)
		}
		profiles = append(profiles, p)
	}
	g, err := app.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: hub.LocalCompanionOwner, Name: "durable group", Members: []string{profiles[0].ID, profiles[1].ID}, ReplyMode: "all", ReplyLimit: 2, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	store := app.Store.(hub.CompanionStore)
	c, err := store.OpenCompanionConversation(t.Context(), g.OwnerID, profiles[0].ID, g.ID, g.Revision, g.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	if err = app.Service.SetToolEnabled(t.Context(), c.SessionID, "interaction", true, false); err != nil {
		t.Fatal(err)
	}
	if err = app.Service.EnableDurableWaits(t.Context(), c.SessionID, true); err != nil {
		t.Fatal(err)
	}
	round, err := app.Service.RunCompanionGroup(t.Context(), g.OwnerID, g.ID, g.Revision, "ask first, then both answer once", nil, true)
	if err != nil || round.Status != "waiting" || round.NextMember != 0 || round.ActiveTurnID == "" {
		t.Fatal("group did not retain original waiting member", round, err)
	}
	originalTurn := round.ActiveTurnID
	if calls.Load() != 1 {
		t.Fatal("second member started while first waited")
	}
	closeApp()
	open()
	round, err = app.Service.Companions.Round(t.Context(), g.OwnerID, round.ID)
	if err != nil || round.Status != "waiting" {
		t.Fatal(round, err)
	}
	page := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(page), 960, 850)
	page.openWorkspaceCompanions()
	waitWorkspace(t, page, tester, func() bool { return page.companions.loaded && !page.companions.busy })
	page.selectWorkspaceCompanionGroup(g)
	waitWorkspace(t, page, tester, func() bool { return !page.companions.busy && len(page.companions.rounds) == 1 })
	if calls.Load() != 1 {
		t.Fatal("cold group observation resumed execution")
	}
	if _, err = app.Service.RunCompanionGroup(t.Context(), g.OwnerID, g.ID, g.Revision, "must not mix another round", nil, true); err == nil {
		t.Fatal("waiting group admitted another round")
	}
	review, err := app.Service.ReviewCompanionRound(t.Context(), g.OwnerID, round.ID)
	if err != nil || review.Wait == nil {
		t.Fatal(review, err)
	}
	view := review.Wait
	if len(view.Snapshot.State.Requests) != 1 {
		t.Fatal(view)
	}
	request := view.Snapshot.State.Requests[0]
	text := "confirmed group input"
	answer, _ := json.Marshal(interaction.Answer{Text: &text})
	if err = app.Service.ResolveWait(t.Context(), c.SessionID, product.WaitResolution{Target: product.WaitTarget{BranchID: view.BranchID, TurnID: view.TurnID, Revision: view.Snapshot.Revision}, RequestID: request.ID, Binding: request.Binding, Result: answer}); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 {
		t.Fatal("saving answer executed")
	}
	review, err = app.Service.ReviewCompanionRound(t.Context(), g.OwnerID, round.ID)
	if err != nil {
		t.Fatal(err)
	}
	view = review.Wait
	page.loadWorkspaceCompanionGroup()
	waitWorkspace(t, page, tester, func() bool {
		review := page.companions.roundReviews[round.ID]
		return !page.companions.busy && review.Wait != nil && review.Wait.CanResume
	})
	companionScrollTo(t, tester, "Approve saved group round "+round.ID, "Companion group conversation")
	companionClick(t, tester, "Approve saved group round "+round.ID)
	companionScrollTo(t, tester, "Continue saved group round "+round.ID, "Companion group conversation")
	companionClick(t, tester, "Continue saved group round "+round.ID)
	waitWorkspace(t, page, tester, func() bool {
		return !page.companions.running && !page.companions.busy && len(page.companions.rounds) > 0 && page.companions.rounds[0].Status == "completed"
	})
	continued := page.companions.rounds[0]
	if continued.NextMember != 2 {
		t.Fatal("same-round continuation cursor missing", continued)
	}
	if calls.Load() != 3 {
		t.Fatal("tool/member replay or missing continuation", calls.Load())
	}
	messages, err := app.Service.Companions.Messages(t.Context(), g.OwnerID, g.ID, g.ScopeRevision, 100)
	if err != nil || len(messages) != 3 || messages[1].TurnID != originalTurn {
		t.Fatal("original member was not delivered exactly once", messages, err)
	}
	closeApp()
	open()
	messages, err = app.Service.Companions.Messages(t.Context(), g.OwnerID, g.ID, g.ScopeRevision, 100)
	if err != nil || len(messages) != 3 || calls.Load() != 3 {
		t.Fatal("reopen changed original replies", messages, err)
	}
	if _, err = app.Service.ContinueCompanionGroup(t.Context(), g.OwnerID, round.ID, continued.Revision, nil, true); err == nil {
		t.Fatal("completed round resumed")
	}
	if calls.Load() != 3 {
		t.Fatal("terminal continuation reran model")
	}
	waiting, err := app.Service.RunCompanionGroup(t.Context(), g.OwnerID, g.ID, g.Revision, "explicit abandonment branch", nil, true)
	if err != nil || waiting.Status != "waiting" {
		t.Fatal(waiting, err)
	}
	if _, err = app.Service.TerminateCompanionGroup(t.Context(), g.OwnerID, waiting.ID, waiting.Revision, true); err == nil {
		t.Fatal("group terminated without reviewing original checkpoint")
	}
	review, err = app.Service.ReviewCompanionRound(t.Context(), g.OwnerID, waiting.ID)
	if err != nil || review.Wait == nil {
		t.Fatal(review, err)
	}
	view = review.Wait
	if err = app.Service.AbandonWait(t.Context(), c.SessionID, product.WaitTarget{BranchID: view.BranchID, TurnID: view.TurnID, Revision: view.Snapshot.Revision}); err != nil {
		t.Fatal(err)
	}
	terminated, err := app.Service.TerminateCompanionGroup(t.Context(), g.OwnerID, waiting.ID, waiting.Revision, true)
	if err != nil || terminated.Status != "cancelled" || calls.Load() != 4 {
		t.Fatal("reviewed termination replayed execution or lacked a receipt", terminated, calls.Load(), err)
	}

}
