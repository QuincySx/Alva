// INPUT: Real product/native fixtures, actual Alva execution and delayed storage/engine owners.
// OUTPUT: Explicit admission, private/group isolation, frozen Skills, finite round Stop and real joined Close regressions.
// POS: [L7 Hub service tests] Uses only local temporary stores and deterministic engines without external accounts.
package product

import (
	"context"
	"errors"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
)

func companionServiceFixture(t *testing.T) (*HubService, chan engine.Request) {
	t.Helper()
	s, requests := fixture(t)
	store, ok := s.Store.(hub.CompanionStore)
	if !ok {
		t.Fatal("missing companion storage")
	}
	var err error
	s.Companions, err = NewCompanionService(store)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Companions.Close(context.Background()) })
	return s, requests
}
func saveCompanion(t *testing.T, s *HubService, name string) hub.CompanionProfile {
	t.Helper()
	p, err := s.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: name, Role: "source-first " + name, Target: s.DefaultTarget, Enabled: true, PermissionPolicy: hub.PermissionAsk}, 0)
	if err != nil {
		t.Fatal(err)
	}
	return p
}
func TestCompanionActualPrivateAndGroupExecutionStayIsolated(t *testing.T) {
	s, requests := companionServiceFixture(t)
	p := saveCompanion(t, s, "research")
	q := saveCompanion(t, s, "review")
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("configuration executed")
	}
	if _, err := s.OpenCompanion(t.Context(), p.OwnerID, p.ID, false); err == nil {
		t.Fatal("account route self-approved")
	}
	private, err := s.OpenCompanion(t.Context(), p.OwnerID, p.ID, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitCompanion(t.Context(), p.OwnerID, p.ID, "PRIVATE-FACT-DO-NOT-SHARE", true); err != nil {
		t.Fatal(err)
	}
	waitIdle(t, s, private.SessionID)
	privateRequest := <-requests
	if !strings.Contains(privateRequest.Prompt, p.Role) {
		t.Fatal("role not consumed by execution")
	}
	g, err := s.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: p.OwnerID, Name: "group", Members: []string{p.ID, q.ID}, ReplyMode: "mentions", ReplyLimit: 2, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.RunCompanionGroup(t.Context(), p.OwnerID, g.ID, g.Revision, "GROUP-FACT", []string{p.ID, q.ID}, false); err == nil {
		t.Fatal("unapproved group admitted")
	}
	round, err := s.RunCompanionGroup(t.Context(), p.OwnerID, g.ID, g.Revision, "GROUP-FACT", []string{q.ID, p.ID}, true)
	if err != nil {
		t.Fatal(err)
	}
	if round.Status != "completed" || len(round.Members) != 2 {
		t.Fatal(round)
	}
	first, second := <-requests, <-requests
	if first.OwnerID == private.SessionID || first.OwnerID == second.OwnerID || strings.Contains(first.Prompt, "PRIVATE-FACT") || strings.Contains(second.Prompt, "PRIVATE-FACT") {
		t.Fatal("private scope entered group", first, second)
	}
	if round.Members[0].CompanionID != p.ID {
		t.Fatal("user mention ordering overrode saved roster")
	}
	messages, err := s.Companions.Messages(t.Context(), p.OwnerID, g.ID, g.ScopeRevision, 256)
	if err != nil || len(messages) != 3 || messages[0].Author != "你" || messages[1].CompanionID != p.ID || messages[2].CompanionID != q.ID {
		t.Fatal("not durable multi-author history", messages, err)
	}
	if messages[1].Text == "" || messages[2].Text == "" {
		t.Fatal("actual engine replies missing", messages)
	}
	settings, err := s.Store.Settings(t.Context(), round.Members[0].SessionID)
	if err != nil || settings.PersistentMemory {
		t.Fatal("private memory enabled for group", err)
	}
	memberSession := round.Members[0].SessionID
	detail, err := s.Store.Detail(t.Context(), memberSession)
	if err != nil {
		t.Fatal(err)
	}
	beforeTurn, err := s.Store.BranchTurn(t.Context(), memberSession, detail.Session.ActiveBranchID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(t.Context(), memberSession, "bypass group orchestration", nil); err == nil {
		t.Fatal("ordinary Submit admitted a group member Turn")
	}
	if _, err = s.SubmitComposerDraft(t.Context(), memberSession, "bypass through composer", nil, nil); err == nil {
		t.Fatal("composer Submit admitted a group member Turn")
	}
	afterTurn, err := s.Store.BranchTurn(t.Context(), memberSession, detail.Session.ActiveBranchID)
	if err != nil || afterTurn.ID != beforeTurn.ID || afterTurn.Sequence != beforeTurn.Sequence || len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("rejected ordinary group Sends changed a Turn or executed", afterTurn, err)
	}
	p.Enabled = false
	if _, err = s.SaveCompanionProfile(t.Context(), p, p.Revision); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitCompanion(t.Context(), p.OwnerID, p.ID, "disabled", true); !errors.Is(err, hub.ErrCompanionDisabled) {
		t.Fatal("disabled profile admitted", err)
	}
}
func TestCompanionPinnedProjectSkillRevalidatedAndNeverImplicitlyShared(t *testing.T) {
	s, _ := companionServiceFixture(t)
	source, err := s.Create(t.Context(), "source project", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err = s.InitializeSkills(t.Context(), source.ID); err != nil {
		t.Fatal(err)
	}
	raw := "---\nname: source-check\ndescription: Verify source material\n---\nCheck exact sources before describing results.\n"
	if err = s.InstallSkill(t.Context(), source.ID, raw); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.SetSkillsEnabled(t.Context(), source.ID, true); err != nil {
		t.Fatal(err)
	}
	ref, err := s.PinCompanionSkill(t.Context(), source.ID, "source-check", false)
	if err != nil {
		t.Fatal(err)
	}
	p := hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: "skilled", Role: "verify", Target: s.DefaultTarget, Workspace: source.Workspace, Skills: []hub.CompanionSkill{ref}, Enabled: true}
	p, err = s.SaveCompanionProfile(t.Context(), p, 0)
	if err != nil {
		t.Fatal(err)
	}
	c, err := s.OpenCompanion(t.Context(), p.OwnerID, p.ID, true)
	if err != nil {
		t.Fatal(err)
	}
	s.mu.Lock()
	prepared, err := s.companionPromptLocked(t.Context(), c, "check request")
	s.mu.Unlock()
	if err != nil || !strings.Contains(prepared, "source-check") {
		t.Fatal("selected Skill not attached", prepared, err)
	}
	settings, err := s.Store.Settings(t.Context(), c.SessionID)
	if err != nil || !settings.SkillsEnabled {
		t.Fatal("existing Skill capability not enabled", err)
	}
	g, err := s.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: p.OwnerID, Name: "group", Members: []string{p.ID}, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	store := s.Companions.store
	r, err := store.BeginCompanionRound(t.Context(), p.OwnerID, g.ID, g.Revision, "group without private Skill", nil)
	if err != nil {
		t.Fatal(err)
	}
	s.mu.Lock()
	prepared, err = s.companionPromptLocked(t.Context(), r.Members[0], "group request")
	s.mu.Unlock()
	if err != nil || strings.Contains(prepared, "source-check") {
		t.Fatal("private Skill implicitly shared", prepared, err)
	}
	if err = store.FinishCompanionRound(t.Context(), p.OwnerID, r.ID, "completed", ""); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ArchiveSkill(t.Context(), source.ID, "source-check"); err != nil {
		t.Fatal(err)
	}
	if err = s.InstallSkill(t.Context(), source.ID, strings.ReplaceAll(raw, "Check exact", "Changed exact")); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SaveCompanionProfile(t.Context(), p, p.Revision); err == nil {
		t.Fatal("changed pinned Skill source accepted")
	}
}

type companionDelayedStore struct {
	hub.CompanionStore
	entered chan struct{}
	release chan struct{}
}

func (s *companionDelayedStore) SaveCompanion(ctx context.Context, p hub.CompanionProfile, revision uint64) (hub.CompanionProfile, error) {
	close(s.entered)
	<-s.release
	return s.CompanionStore.SaveCompanion(context.WithoutCancel(ctx), p, revision)
}
func TestCompanionCloseRetainsUnfinishedStorageOwner(t *testing.T) {
	s, _ := companionServiceFixture(t)
	delayed := &companionDelayedStore{CompanionStore: s.Companions.store, entered: make(chan struct{}), release: make(chan struct{})}
	owner, err := NewCompanionService(delayed)
	if err != nil {
		t.Fatal(err)
	}
	result := make(chan error, 1)
	go func() {
		_, err := owner.Save(context.Background(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: "late committed", Target: s.DefaultTarget, Enabled: true}, 0)
		result <- err
	}()
	<-delayed.entered
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err = owner.Close(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("Close fabricated completion", err)
	}
	if _, err = owner.List(t.Context(), hub.LocalCompanionOwner); !errors.Is(err, storage.ErrClosed) {
		t.Fatal("closed owner admitted storage", err)
	}
	close(delayed.release)
	if err = <-result; err != nil {
		t.Fatal(err)
	}
	if err = owner.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	profiles, err := delayed.CompanionStore.Companions(t.Context(), hub.LocalCompanionOwner)
	if err != nil || len(profiles) != 1 {
		t.Fatal("late commit lost", profiles, err)
	}
}

type companionSlowEngine struct {
	entered   chan struct{}
	cancelled chan struct{}
	release   chan struct{}
	calls     atomic.Int32
}

func (e *companionSlowEngine) Execute(ctx context.Context, r engine.Request, emit func(engine.Event) error) error {
	e.calls.Add(1)
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "native-" + r.OwnerID}); err != nil {
		return err
	}
	close(e.entered)
	<-ctx.Done()
	close(e.cancelled)
	<-e.release
	return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Cancelled})
}
func (e *companionSlowEngine) Cancel(context.Context, string) error { return nil }
func (e *companionSlowEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return nil
}
func (e *companionSlowEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Cancel: true}
}
func TestCompanionGroupStopJoinsRealOwnerAndNeverStartsNextMember(t *testing.T) {
	s, _ := companionServiceFixture(t)
	e := &companionSlowEngine{entered: make(chan struct{}), cancelled: make(chan struct{}), release: make(chan struct{})}
	if err := s.Runtime.Register("companion-slow", e); err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "companion-slow"
	s.Targets = append(s.Targets, target)
	p := saveCompanion(t, s, "first")
	q := saveCompanion(t, s, "second")
	p.Target = target
	q.Target = target
	var err error
	p, err = s.SaveCompanionProfile(t.Context(), p, p.Revision)
	if err != nil {
		t.Fatal(err)
	}
	q, err = s.SaveCompanionProfile(t.Context(), q, q.Revision)
	if err != nil {
		t.Fatal(err)
	}
	g, err := s.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: p.OwnerID, Name: "finite", Members: []string{p.ID, q.ID}, ReplyMode: "all", ReplyLimit: 2, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	run := make(chan error, 1)
	go func() {
		_, err := s.RunCompanionGroup(context.Background(), p.OwnerID, g.ID, g.Revision, "wait for stop", nil, true)
		run <- err
	}()
	<-e.entered
	p.Enabled = false
	if _, err = s.SaveCompanionProfile(t.Context(), p, p.Revision); err != nil {
		t.Fatal(err)
	}
	var activeRound hub.CompanionRound
	for i := 0; i < 100; i++ {
		rounds, e := s.Companions.Rounds(t.Context(), p.OwnerID, g.ID, g.ScopeRevision, 32)
		if e != nil {
			t.Fatal(e)
		}
		if len(rounds) > 0 && rounds[0].Stage == "executing" {
			activeRound = rounds[0]
			break
		}
		time.Sleep(time.Millisecond)
	}
	if activeRound.ID == "" {
		t.Fatal("no saved active round")
	}
	stopped := make(chan error, 1)
	go func() {
		stopped <- s.StopCompanionGroup(context.Background(), p.OwnerID, g.ID, activeRound.ID, activeRound.Revision, activeRound.OperationID)
	}()
	<-e.cancelled
	select {
	case err = <-stopped:
		t.Fatal("Stop returned before physical cleanup", err)
	case <-time.After(20 * time.Millisecond):
	}
	close(e.release)
	if err = <-stopped; err != nil {
		t.Fatal(err)
	}
	if err = <-run; !errors.Is(err, context.Canceled) {
		t.Fatal("group cancellation lost", err)
	}
	if e.calls.Load() != 1 || s.Runtime.HasActive() {
		t.Fatal("Stop started another member or left an owner", e.calls.Load())
	}
	rounds, err := s.Companions.Rounds(t.Context(), p.OwnerID, g.ID, g.ScopeRevision, 32)
	if err != nil || rounds[0].Status != "cancelled" {
		t.Fatal("durable stop receipt missing", rounds, err)
	}
}

type companionBadReceiptStore struct {
	hub.CompanionStore
	mode    string
	calls   atomic.Int32
	failure error
	profile hub.CompanionProfile
	group   hub.CompanionGroup
}

func (s *companionBadReceiptStore) SaveCompanion(ctx context.Context, p hub.CompanionProfile, expected uint64) (hub.CompanionProfile, error) {
	s.calls.Add(1)
	if s.failure != nil {
		return hub.CompanionProfile{}, s.failure
	}
	out := s.profile
	if s.mode == "foreign" {
		out.OwnerID = "foreign-owner"
		out.Revision = expected + 1
	}
	if s.mode == "different" {
		out.Revision = expected + 1
		out.Role = "silently different"
	}
	return out, nil
}
func (s *companionBadReceiptStore) Companions(context.Context, string) ([]hub.CompanionProfile, error) {
	out := s.profile
	out.OwnerID = "foreign-owner"
	return []hub.CompanionProfile{out}, nil
}
func (s *companionBadReceiptStore) Companion(context.Context, string, string) (hub.CompanionProfile, error) {
	out := s.profile
	out.OwnerID = "foreign-owner"
	return out, nil
}
func (s *companionBadReceiptStore) SaveCompanionGroup(ctx context.Context, g hub.CompanionGroup, expected uint64) (hub.CompanionGroup, error) {
	s.calls.Add(1)
	return s.group, nil
}
func TestCompanionBadAndUncertainReceiptsNeverPublishOrReplay(t *testing.T) {
	s, _ := companionServiceFixture(t)
	p := saveCompanion(t, s, "saved")
	base := s.Companions.store
	for _, mode := range []string{"noop", "foreign", "different"} {
		t.Run(mode, func(t *testing.T) {
			bad := &companionBadReceiptStore{CompanionStore: base, mode: mode, profile: p}
			owner, err := NewCompanionService(bad)
			if err != nil {
				t.Fatal(err)
			}
			defer owner.Close(context.Background())
			request := p
			request.Role = "requested edit"
			if out, err := owner.Save(t.Context(), request, p.Revision); err == nil || out.ID != "" {
				t.Fatal("invalid save receipt published", out, err)
			}
			if bad.calls.Load() != 1 {
				t.Fatal("invalid mutation was retried")
			}
			if out, err := owner.List(t.Context(), p.OwnerID); err == nil || len(out) != 0 {
				t.Fatal("foreign roster published", out, err)
			}
			if out, err := owner.Read(t.Context(), p.OwnerID, p.ID); err == nil || out.ID != "" {
				t.Fatal("foreign read published", out, err)
			}
		})
	}
	secret := errors.New("backend private://secret-database-path")
	bad := &companionBadReceiptStore{CompanionStore: base, failure: secret, profile: p}
	owner, err := NewCompanionService(bad)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close(context.Background())
	if _, err = owner.Save(t.Context(), p, p.Revision); err == nil || strings.Contains(err.Error(), "secret-database") || !errors.Is(err, secret) {
		t.Fatal("opaque failure lost cause or leaked backend detail", err)
	}
	if bad.calls.Load() != 1 {
		t.Fatal("uncertain mutation automatically replayed")
	}
	actual, err := base.Companion(t.Context(), p.OwnerID, p.ID)
	if err != nil || actual.Revision != p.Revision {
		t.Fatal("fake receipt changed committed profile", actual, err)
	}
}

func TestCompanionProfileChangeRevokesReviewedRouteSnapshot(t *testing.T) {
	s, requests := companionServiceFixture(t)
	p := saveCompanion(t, s, "reviewed")
	reviewed := p
	p.Role = "changed after review"
	var err error
	p, err = s.SaveCompanionProfile(t.Context(), p, p.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitCompanion(t.Context(), p.OwnerID, p.ID, "must not execute", true, reviewed); !errors.Is(err, hub.ErrCompanionConflict) {
		t.Fatal("stale private review admitted", err)
	}
	g, err := s.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: p.OwnerID, Name: "reviewed group", Members: []string{p.ID}, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.RunCompanionGroup(t.Context(), p.OwnerID, g.ID, g.Revision, "must not execute", nil, true, reviewed); !errors.Is(err, hub.ErrCompanionConflict) {
		t.Fatal("stale group review admitted", err)
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("stale review executed")
	}
}

type companionProgressFailureStore struct {
	hub.CompanionStore
	publishMode   string
	finishFailure bool
	failed        atomic.Bool
}

func (s *companionProgressFailureStore) PublishCompanionReply(ctx context.Context, owner, id string, revision uint64, m hub.CompanionMessage) (hub.CompanionRound, error) {
	if s.publishMode != "" && !s.failed.Swap(true) {
		if s.publishMode == "after" {
			out, err := s.CompanionStore.PublishCompanionReply(ctx, owner, id, revision, m)
			if err != nil {
				return out, err
			}
			return out, errors.New("uncertain publication receipt")
		}
		return hub.CompanionRound{}, errors.New("publication unavailable")
	}
	return s.CompanionStore.PublishCompanionReply(ctx, owner, id, revision, m)
}
func (s *companionProgressFailureStore) FinishCompanionRound(ctx context.Context, owner, id, status, problem string) error {
	if s.finishFailure && !s.failed.Swap(true) {
		return errors.New("terminal commit unavailable")
	}
	return s.CompanionStore.FinishCompanionRound(ctx, owner, id, status, problem)
}
func TestCompanionPublicationAndTerminalFailuresReconcileWithoutExecutionReplay(t *testing.T) {
	for _, mode := range []string{"before", "after", "finish"} {
		t.Run(mode, func(t *testing.T) {
			s, requests := companionServiceFixture(t)
			p := saveCompanion(t, s, "first")
			q := saveCompanion(t, s, "second")
			g, err := s.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: p.OwnerID, Name: "receipt", Members: []string{p.ID, q.ID}, ReplyMode: "all", ReplyLimit: 2, Enabled: true}, 0)
			if err != nil {
				t.Fatal(err)
			}
			wrapped := &companionProgressFailureStore{CompanionStore: s.Companions.store, publishMode: mode}
			if mode == "finish" {
				wrapped.publishMode = ""
				wrapped.finishFailure = true
			}
			s.Companions, err = NewCompanionService(wrapped)
			if err != nil {
				t.Fatal(err)
			}
			r, err := s.RunCompanionGroup(t.Context(), p.OwnerID, g.ID, g.Revision, "execute each once", nil, true)
			if err == nil {
				t.Fatal("injected failure was hidden")
			}
			if _, err = s.RunCompanionGroup(t.Context(), p.OwnerID, g.ID, g.Revision, "must remain blocked", nil, true); !errors.Is(err, hub.ErrSessionBusy) {
				t.Fatal("unresolved round admitted another", err)
			}
			actual, err := s.Companions.Round(t.Context(), p.OwnerID, r.ID)
			if err != nil {
				t.Fatal(err)
			}
			if mode == "finish" {
				if err = s.StopCompanionGroup(t.Context(), p.OwnerID, g.ID, actual.ID, actual.Revision, actual.OperationID); err != nil {
					t.Fatal("explicit terminal retry failed", err)
				}
			} else {
				if _, err = s.ContinueCompanionGroup(t.Context(), p.OwnerID, r.ID, actual.Revision, nil, true); err != nil {
					t.Fatal("publication reconciliation failed", err)
				}
			}
			if len(requests) != 2 {
				t.Fatal("reconciliation replayed a member", len(requests))
			}
			messages, err := s.Companions.Messages(t.Context(), p.OwnerID, g.ID, g.ScopeRevision, 100)
			if err != nil || len(messages) != 3 || messages[1].ID == messages[2].ID {
				t.Fatal("duplicate/missing publication", messages, err)
			}
			actual, err = s.Companions.Round(t.Context(), p.OwnerID, r.ID)
			if err != nil || actual.Status != "completed" || actual.NextMember != 2 {
				t.Fatal("round terminalization missing", actual, err)
			}
		})
	}
}
