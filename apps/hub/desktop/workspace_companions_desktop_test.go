// INPUT: Real temporary Hub/native stores, deterministic local harness and actual Foundation/MyGo controls.
// OUTPUT: Headless CRUD, deleted-history reopen, 258-message/129-round keyset UI and isolated explicit execution regressions.
// POS: [L7 Hub UI tests] Does not open a window, launch a client or access external accounts/user data.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
)

func companionPage(t *testing.T) (*NativePage, *ui.Tester) {
	t.Helper()
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := app.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	s := app.Service
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return p.companions.loaded && !p.companions.busy })
	return p, tester
}
func companionClick(t *testing.T, tester *ui.Tester, label string) {
	t.Helper()
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
}
func companionType(t *testing.T, tester *ui.Tester, label, text string) {
	t.Helper()
	companionClick(t, tester, label)
	tester.Type(text)
	tester.Frame()
}
func createCompanionUI(t *testing.T, p *NativePage, tester *ui.Tester, name string) {
	t.Helper()
	companionClick(t, tester, "New companion")
	companionType(t, tester, "Companion name", name)
	companionType(t, tester, "Companion role", "Check sources before answering.")
	companionClick(t, tester, "Save companion profile")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.editor == "" })
}
func TestWorkspaceCompanionActualCRUDAndFiniteGroupUI(t *testing.T) {
	p, tester := companionPage(t)
	if p.Service.Runtime.HasActive() {
		t.Fatal("opening roster executed")
	}
	createCompanionUI(t, p, tester, "Research")
	createCompanionUI(t, p, tester, "Review")
	sessions, err := p.Service.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 {
		t.Fatal("profile creation created chat", sessions, err)
	}
	companionClick(t, tester, "New companion group")
	companionType(t, tester, "Companion group name", "Product group")
	companionClick(t, tester, "Group member Research")
	companionClick(t, tester, "Group member Review")
	companionClick(t, tester, "Save companion group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && p.companions.editor == "" })
	g := p.companions.group
	if len(g.Members) != 2 || g.Revision != 1 {
		t.Fatal("group member controls not saved", g, p.companions.problem)
	}
	sessions, err = p.Service.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("creating group executed", err)
	}
	companionClick(t, tester, "Mention companion Research")
	companionClick(t, tester, "Mention companion Review")
	companionType(t, tester, "Companion group message", "Review the group flow.")
	if err = tester.Click("Send companion group round"); err == nil && p.companions.running {
		t.Fatal("unapproved group ran")
	}
	companionClick(t, tester, "Approve group accounts and billing routes")
	companionClick(t, tester, "Send companion group round")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.running && !p.companions.busy })
	if len(p.companions.messages) != 3 {
		t.Fatalf("group round did not produce replies: problem=%s rounds=%+v messages=%+v mentions=%+v", p.companions.problem, p.companions.rounds, p.companions.messages, p.companions.mentions)
	}
	if p.companions.messages[1].CompanionID != g.Members[0] || p.companions.messages[2].CompanionID != g.Members[1] {
		t.Fatal("group replies lost saved member order", p.companions.messages)
	}
	companionClick(t, tester, "Open companion Research")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	companionClick(t, tester, "Approve companion account and billing route")
	companionClick(t, tester, "Open companion private conversation")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && !p.polling && p.companions.conversation.SessionID != "" })
	private := p.companions.conversation.SessionID
	for _, r := range p.companions.rounds {
		for _, member := range r.Members {
			if member.SessionID == private {
				t.Fatal("private conversation reused group member owner")
			}
		}
	}
	companionType(t, tester, "Companion message", "Keep this in the private conversation.")
	companionClick(t, tester, "Send companion message")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && !p.Service.Runtime.HasActive() && !p.polling })
	if p.selected != private || p.Service.Runtime.HasActive() {
		t.Fatal("private UI owner not retained")
	}
	companionClick(t, tester, "Toggle companion or group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	if p.companions.profile.Enabled {
		t.Fatal("disable failed")
	}
	if _, err = p.Service.Store.Detail(t.Context(), private); err != nil {
		t.Fatal("disable lost history", err)
	}
}

type companionWorkspaceReadBarrierStore struct {
	hub.CompanionStore
	publication                int32
	publications               atomic.Int32
	publishing, publishRelease chan struct{}
	readKind                   string
	holdRead                   atomic.Bool
	readCaptured, readRelease  chan struct{}
}

func (s *companionWorkspaceReadBarrierStore) PublishCompanionReply(ctx context.Context, owner, id string, revision uint64, message hub.CompanionMessage) (hub.CompanionRound, error) {
	if s.publications.Add(1) == s.publication {
		close(s.publishing)
		<-s.publishRelease
	}
	return s.CompanionStore.PublishCompanionReply(ctx, owner, id, revision, message)
}
func (s *companionWorkspaceReadBarrierStore) CompanionRounds(ctx context.Context, owner, id string, scope uint64, limit int) ([]hub.CompanionRound, error) {
	rounds, err := s.CompanionStore.CompanionRounds(ctx, owner, id, scope, limit)
	s.pauseRead("group")
	return rounds, err
}
func (s *companionWorkspaceReadBarrierStore) CompanionArchive(ctx context.Context, owner string) (hub.CompanionArchive, error) {
	return s.CompanionStore.(hub.CompanionHistoryStore).CompanionArchive(ctx, owner)
}
func (s *companionWorkspaceReadBarrierStore) CompanionMessagePage(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery) (hub.CompanionMessagePage, error) {
	return s.CompanionStore.(hub.CompanionHistoryStore).CompanionMessagePage(ctx, owner, id, scope, query)
}
func (s *companionWorkspaceReadBarrierStore) CompanionRoundPage(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery) (hub.CompanionRoundPage, error) {
	page, err := s.CompanionStore.(hub.CompanionHistoryStore).CompanionRoundPage(ctx, owner, id, scope, query)
	s.pauseRead("group")
	return page, err
}
func (s *companionWorkspaceReadBarrierStore) CompanionGroups(ctx context.Context, owner string) ([]hub.CompanionGroup, error) {
	groups, err := s.CompanionStore.CompanionGroups(ctx, owner)
	s.pauseRead("roster")
	return groups, err
}
func (s *companionWorkspaceReadBarrierStore) pauseRead(kind string) {
	if s.readKind == kind && s.holdRead.CompareAndSwap(true, false) {
		close(s.readCaptured)
		<-s.readRelease
	}
}
func companionWorkspaceBarrier(t *testing.T, publication int32, readKind string) (*NativePage, *ui.Tester, *companionWorkspaceReadBarrierStore, func(), func()) {
	t.Helper()
	p, tester := companionPage(t)
	var members []string
	for _, name := range []string{"Research", "Review"} {
		profile, err := p.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: name, Target: p.Service.DefaultTarget, Enabled: true}, 0)
		if err != nil {
			t.Fatal(err)
		}
		members = append(members, profile.ID)
	}
	g, err := p.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: hub.LocalCompanionOwner, Name: "controlled round", Members: members, ReplyMode: "all", ReplyLimit: 2, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	p.selectWorkspaceCompanionGroup(g)
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	if err := p.Service.Companions.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := &companionWorkspaceReadBarrierStore{CompanionStore: p.Service.Store.(hub.CompanionStore), publication: publication, publishing: make(chan struct{}), publishRelease: make(chan struct{}), readKind: readKind, readCaptured: make(chan struct{}), readRelease: make(chan struct{})}
	p.Service.Companions, err = product.NewCompanionService(store)
	if err != nil {
		t.Fatal(err)
	}
	var publicationOnce, readOnce sync.Once
	releasePublication := func() { publicationOnce.Do(func() { close(store.publishRelease) }) }
	releaseRead := func() { readOnce.Do(func() { close(store.readRelease) }) }
	t.Cleanup(releaseRead)
	t.Cleanup(releasePublication)
	p.companions.prompt = "Complete both replies exactly once."
	p.companions.approved = true
	p.runWorkspaceCompanionGroup()
	select {
	case <-store.publishing:
	case <-time.After(5 * time.Second):
		t.Fatal("group did not reach controlled publication")
	}
	return p, tester, store, releasePublication, releaseRead
}

func TestWorkspaceCompanionRunningRetainsOwnerBetweenMembers(t *testing.T) {
	p, tester, _, releasePublication, _ := companionWorkspaceBarrier(t, 1, "")
	if p.Service.Runtime.HasActive() {
		t.Fatal("physical member execution did not exit before publication")
	}
	p.loadWorkspaceCompanionGroup()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	if !p.companions.running || !p.workspaceCompanionGroupRunning() || len(p.companions.rounds) != 1 || p.companions.rounds[0].NextMember != 0 {
		t.Fatal("observer cleared the finite-round owner between member executions", p.companions)
	}
	releasePublication()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.running && !p.companions.busy })
	if len(p.companions.messages) != 3 || p.companions.rounds[0].Status != "completed" {
		t.Fatal("round did not finish after publication was released", p.companions)
	}
}

func TestWorkspaceCompanionCompletionRefreshSupersedesEarlierRead(t *testing.T) {
	for _, readKind := range []string{"group", "roster"} {
		t.Run(readKind, func(t *testing.T) {
			p, tester, store, releasePublication, releaseRead := companionWorkspaceBarrier(t, 2, readKind)
			store.holdRead.Store(true)
			if readKind == "group" {
				p.loadWorkspaceCompanionGroup()
			} else {
				p.reloadWorkspaceCompanions()
			}
			select {
			case <-store.readCaptured:
			case <-time.After(5 * time.Second):
				t.Fatal("observer read did not capture the unfinished round")
			}
			releasePublication()
			waitWorkspace(t, p, tester, func() bool { return !p.companions.running })
			if !p.companions.busy {
				t.Fatal("blocked observer read lost its busy ownership")
			}
			releaseRead()
			waitWorkspace(t, p, tester, func() bool { return !p.companions.running && !p.companions.busy })
			if len(p.companions.messages) != 3 || len(p.companions.rounds) != 1 || p.companions.rounds[0].Status != "completed" {
				t.Fatal("finished group settled on the earlier observer snapshot", p.companions)
			}
		})
	}
}

func TestWorkspaceCompanionProfileCancelIsColdAndPreservesDraft(t *testing.T) {
	p, tester := companionPage(t)
	p.model.Draft = "ordinary workspace draft"
	companionClick(t, tester, "New companion")
	companionType(t, tester, "Companion name", "Cancelled")
	companionType(t, tester, "Companion role", "Never save this draft.")
	companionClick(t, tester, "Cancel companion profile")
	profiles, err := p.Service.Companions.List(t.Context(), hub.LocalCompanionOwner)
	if err != nil || len(profiles) != 0 || p.model.Draft != "ordinary workspace draft" || p.Service.Runtime.HasActive() {
		t.Fatal("cancel saved/executed or replaced workspace draft", profiles, err)
	}
}

func companionScrollTo(t *testing.T, tester *ui.Tester, label, viewportLabel string) {
	t.Helper()
	for i := 0; i < 16; i++ {
		box, found := tester.Find(label)
		viewport, ok := tester.Find(viewportLabel)
		if !found || !ok {
			t.Fatal("companion scroll control missing", label, viewportLabel)
		}
		if box.W <= 0 || box.H <= 0 {
			dy := float32(48)
			if box.Y < viewport.Y {
				dy = -48
			}
			if i >= 8 {
				dy = -dy
			}
			tester.Scroll(viewport.X+viewport.W-12, viewport.Y+16, 0, dy)
			tester.Frame()
			continue
		}
		dy := float32(0)
		if box.Y < viewport.Y {
			dy = box.Y - viewport.Y - 8
		} else if box.Y+box.H > viewport.Y+viewport.H {
			dy = box.Y + box.H - viewport.Y - viewport.H + 8
		} else {
			return
		}
		tester.Scroll(viewport.X+viewport.W-12, viewport.Y+16, 0, dy)
		tester.Frame()
	}
	t.Fatal("companion control could not be scrolled into view", label)
}
func TestWorkspaceCompanionNarrowGroupAllLongNameMembersSelectable(t *testing.T) {
	p, tester := companionPage(t)
	var profiles []hub.CompanionProfile
	members := []string{}
	for i := 0; i < 6; i++ {
		profile, err := p.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: fmt.Sprintf("长期协作伙伴资料核对与文档整理成员%d", i+1), Target: p.Service.DefaultTarget, Enabled: true}, 0)
		if err != nil {
			t.Fatal(err)
		}
		profiles = append(profiles, profile)
		members = append(members, profile.ID)
	}
	g, err := p.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: hub.LocalCompanionOwner, Name: "six members", Members: members, ReplyMode: "mentions", ReplyLimit: 6, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	p.reloadWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.profiles) == 6 })
	p.selectWorkspaceCompanionGroup(g)
	tester = ui.NewTester(workspaceStaticView(p), 960, 850)
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	for _, profile := range profiles {
		label := "Mention companion " + profile.Name
		companionScrollTo(t, tester, label, "Group mention list")
		companionClick(t, tester, label)
		if !p.companions.mentions[profile.ID] {
			t.Fatal("member did not receive actual selection", profile.Name)
		}
	}
	for _, label := range []string{"Send companion group round", "Stop companion group round"} {
		box, ok := tester.Find(label)
		if !ok || box.W <= 0 || box.H <= 0 || box.Y < 0 || box.Y+box.H > 850 {
			t.Fatal("group send/stop was displaced from the narrow viewport", label, box)
		}
	}
	if p.Service.Runtime.HasActive() {
		t.Fatal("member selection executed")
	}
}

func TestWorkspaceDeletedCompanionHistoryReopensAndLoadsEveryPage(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"saved-local-history-%d\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n", n)
	}))
	defer upstream.Close()
	config := bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}}
	app, err := bootstrap.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if app != nil {
			_ = app.Close(context.Background())
		}
	}()
	s := app.Service
	profile, err := s.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: "History companion", Target: s.DefaultTarget, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	private, err := s.OpenCompanion(t.Context(), profile.OwnerID, profile.ID, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitCompanion(t.Context(), profile.OwnerID, profile.ID, "private saved fact", true); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	waitWorkspace(t, p, tester, func() bool { return !s.Runtime.HasActive() })
	g, err := s.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: profile.OwnerID, Name: "History group", Members: []string{profile.ID}, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.RunCompanionGroup(t.Context(), profile.OwnerID, g.ID, g.Revision, "oldest group fact", nil, true); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 2 {
		t.Fatal("actual fixture private/group send missing", calls.Load())
	}
	store := app.Store.(hub.CompanionStore)
	for i := 1; i < 129; i++ {
		r, e := store.BeginCompanionRound(t.Context(), profile.OwnerID, g.ID, g.Revision, fmt.Sprintf("saved prompt %03d", i), nil)
		if e != nil {
			t.Fatal(e)
		}
		if e = store.AppendCompanionMessage(t.Context(), profile.OwnerID, hub.CompanionMessage{ID: fmt.Sprintf("fixture-reply-%03d", i), RoundID: r.ID, GroupID: g.ID, GroupRevision: 1, Author: profile.Name, Text: fmt.Sprintf("saved reply %03d", i), CompanionID: profile.ID, SessionID: r.Members[0].SessionID, Status: "completed", CreatedAt: r.CreatedAt}); e != nil {
			t.Fatal(e)
		}
		if e = store.FinishCompanionRound(t.Context(), profile.OwnerID, r.ID, "completed", ""); e != nil {
			t.Fatal(e)
		}
	}
	if err = s.Companions.Delete(t.Context(), profile.OwnerID, profile.ID, profile.Revision); err != nil {
		t.Fatal(err)
	}
	if err = s.Companions.DeleteGroup(t.Context(), profile.OwnerID, g.ID, g.Revision); err != nil {
		t.Fatal(err)
	}
	if err = app.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	app, err = bootstrap.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	p = NewNativePage(app.Service)
	tester = ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return p.companions.loaded && !p.companions.busy })
	if len(p.companions.profiles) != 0 || len(p.companions.groups) != 0 || len(p.companions.archive.Profiles) != 1 || len(p.companions.archive.Groups) != 1 {
		t.Fatal("cold deleted-history discovery failed", p.companions)
	}
	companionClick(t, tester, "Open archived companion History companion")
	waitWorkspace(t, p, tester, func() bool {
		return !p.companions.busy && !p.polling && p.companions.conversation.SessionID == private.SessionID
	})
	if !tester.HasText("saved-local-history-1") {
		t.Fatal("saved private response not rendered")
	}
	for _, label := range []string{"Edit companion or group", "Toggle companion or group", "Approve companion account and billing route", "Send companion message", "Open companion private conversation"} {
		if _, found := tester.Find(label); found {
			t.Fatal("deleted identity exposed mutation/approval", label)
		}
	}
	companionClick(t, tester, "Open archived group History group")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.messages) == 256 })
	if len(p.companions.rounds) != 32 || !p.companions.moreMessages || !p.companions.moreRounds {
		t.Fatal("truncated window looked complete", p.companions.problem)
	}
	companionClick(t, tester, "Load older companion messages")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && !p.companions.moreMessages })
	if len(p.companions.messages) != 258 || p.companions.messages[0].Text != "oldest group fact" || p.companions.messages[257].Text != "saved reply 128" {
		t.Fatal("UI message coverage/endpoints lost", len(p.companions.messages), p.companions.problem)
	}
	for p.companions.moreRounds {
		companionClick(t, tester, "Load older companion rounds")
		waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	}
	if len(p.companions.rounds) != 129 {
		t.Fatal("UI round coverage lost", len(p.companions.rounds), p.companions.problem)
	}
	seen := map[string]bool{}
	for _, m := range p.companions.messages {
		if seen[m.ID] {
			t.Fatal("duplicated UI message", m.ID)
		}
		seen[m.ID] = true
	}
	seen = map[string]bool{}
	for _, r := range p.companions.rounds {
		if seen[r.ID] {
			t.Fatal("duplicated UI round", r.ID)
		}
		seen[r.ID] = true
	}
	for _, label := range []string{"Edit companion or group", "Toggle companion or group", "Approve group accounts and billing routes", "Send companion group round", "Stop companion group round"} {
		if _, found := tester.Find(label); found {
			t.Fatal("deleted group exposed execution", label)
		}
	}
	member := p.companions.rounds[0].Members[0]
	label := "Observe group member " + member.SessionID
	reached := false
	for i := 0; i < 128; i++ {
		box, found := tester.Find(label)
		viewport, ok := tester.Find("Companion group conversation")
		if !found || !ok {
			t.Fatal("archived member history control missing")
		}
		if box.W > 0 && box.H > 0 && box.Y >= viewport.Y && box.Y+box.H <= viewport.Y+viewport.H {
			reached = true
			break
		}
		tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, viewport.H/2)
		tester.Frame()
	}
	if !reached {
		t.Fatal("archived member history could not be reached by scrolling")
	}
	companionClick(t, tester, "Observe group member "+member.SessionID)
	waitWorkspace(t, p, tester, func() bool { return !p.polling && p.selected == member.SessionID })
	if !tester.HasText("saved-local-history-2") {
		t.Fatal("member history action did not render original response")
	}
	for _, label := range []string{"Send companion group round", "Approve group accounts and billing routes", "Approve companion account and billing route"} {
		if _, found := tester.Find(label); found {
			t.Fatal("archived member transcript exposed execution", label)
		}
	}
	companionClick(t, tester, "Back to companion group messages")
	if p.companions.memberHistory != "" {
		t.Fatal("member history did not return to group")
	}
	if calls.Load() != 2 || app.Runtime.HasActive() {
		t.Fatal("history reopening/pagination executed", calls.Load())
	}
}

type companionMutationOnlyStore struct{ hub.CompanionStore }

func TestWorkspaceOptionalArchiveDoesNotHideLiveCompanions(t *testing.T) {
	p, tester := companionPage(t)
	profile, err := p.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: "Existing provider", Target: p.Service.DefaultTarget, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err = p.Service.Companions.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	p.Service.Companions, err = product.NewCompanionService(companionMutationOnlyStore{CompanionStore: p.Service.Store.(hub.CompanionStore)})
	if err != nil {
		t.Fatal(err)
	}
	p.reloadWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.profiles) == 1 })
	if p.companions.profiles[0].ID != profile.ID || p.companions.problem != "" || p.companions.archiveProblem == "" {
		t.Fatal("optional archive failure hid live roster", p.companions)
	}
	if _, found := tester.Find("Companion archive unavailable"); !found {
		t.Fatal("unsupported archive was not explicit")
	}
	companionClick(t, tester, "Open companion Existing provider")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	companionClick(t, tester, "Approve companion account and billing route")
	companionClick(t, tester, "Open companion private conversation")
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && !p.polling && p.companions.conversation.SessionID != "" })
	if p.companions.conversation.CompanionID != profile.ID || p.Service.Runtime.HasActive() {
		t.Fatal("optional archive removed or executed private conversation", p.companions)
	}
}

type companionUIOwnedEngine struct {
	entered, cancelled, release chan struct{}
	calls                       atomic.Int32
}

func (e *companionUIOwnedEngine) Execute(ctx context.Context, r engine.Request, emit func(engine.Event) error) error {
	e.calls.Add(1)
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "fixture-" + r.OwnerID}); err != nil {
		return err
	}
	close(e.entered)
	<-ctx.Done()
	close(e.cancelled)
	<-e.release
	return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Cancelled})
}
func (e *companionUIOwnedEngine) Cancel(context.Context, string) error { return nil }
func (e *companionUIOwnedEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (e *companionUIOwnedEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Cancel: true}
}

func TestWorkspaceOptionalHistoryPreservesGroupOwnerStop(t *testing.T) {
	p, tester := companionPage(t)
	fixture := &companionUIOwnedEngine{entered: make(chan struct{}), cancelled: make(chan struct{}), release: make(chan struct{})}
	var releaseOnce sync.Once
	release := func() { releaseOnce.Do(func() { close(fixture.release) }) }
	t.Cleanup(release)
	if err := p.Service.Runtime.Register("owned-companion-fixture", fixture); err != nil {
		t.Fatal(err)
	}
	target := p.Service.DefaultTarget
	target.EngineID = "owned-companion-fixture"
	p.Service.Targets = append(p.Service.Targets, target)
	var members []string
	for _, name := range []string{"first bounded provider member", "second bounded provider member"} {
		profile, err := p.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: name, Target: target, Enabled: true}, 0)
		if err != nil {
			t.Fatal(err)
		}
		members = append(members, profile.ID)
	}
	g, err := p.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: hub.LocalCompanionOwner, Name: "bounded provider group", Members: members, ReplyMode: "all", ReplyLimit: 2, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err = p.Service.Companions.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	p.Service.Companions, err = product.NewCompanionService(companionMutationOnlyStore{CompanionStore: p.Service.Store.(hub.CompanionStore)})
	if err != nil {
		t.Fatal(err)
	}
	p.reloadWorkspaceCompanions()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy && len(p.companions.profiles) == 2 })
	p.selectWorkspaceCompanionGroup(g)
	waitWorkspace(t, p, tester, func() bool { return !p.companions.busy })
	companionClick(t, tester, "Approve group accounts and billing routes")
	companionType(t, tester, "Companion group message", "Stop the actual first member owner")
	companionClick(t, tester, "Send companion group round")
	select {
	case <-fixture.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("group fixture did not execute")
	}
	waitWorkspace(t, p, tester, func() bool {
		if !p.companions.busy {
			if len(p.companions.rounds) == 1 && p.companions.rounds[0].Stage == "executing" {
				return true
			}
			p.loadWorkspaceCompanionGroup()
		}
		return false
	})
	if p.companions.historyWarning == "" || p.companions.moreMessages || p.companions.moreRounds || !p.workspaceCompanionGroupRunning() {
		t.Fatal("bounded provider lost running owner or claimed full history", p.companions)
	}
	if _, found := tester.Find("Companion group pagination unavailable"); !found {
		t.Fatal("bounded history was not explicit")
	}
	companionClick(t, tester, "Stop companion group round")
	select {
	case <-fixture.cancelled:
	case <-time.After(5 * time.Second):
		t.Fatal("Stop did not reach the actual owner")
	}
	if !p.companions.running || !p.companions.stopping || !p.Service.Runtime.HasActive() {
		t.Fatal("Stop released the actual owner before execution returned")
	}
	release()
	waitWorkspace(t, p, tester, func() bool { return !p.companions.running && !p.companions.stopping && !p.companions.busy })
	if p.companions.rounds[0].Status != "cancelled" || p.Service.Runtime.HasActive() || fixture.calls.Load() != 1 {
		t.Fatal("optional history prevented real group Stop/join", p.companions)
	}
	second := p.companions.rounds[0].Members[1]
	detail, err := p.Service.Store.Detail(t.Context(), second.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = p.Service.Store.BranchTurn(t.Context(), second.SessionID, detail.Session.ActiveBranchID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("Stop executed the next group member", err)
	}
}
