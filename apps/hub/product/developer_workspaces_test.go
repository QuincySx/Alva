// INPUT: Real Hub product/native fixtures, explicit repository source fake and live target services.
// OUTPUT: Repository/worktree facade plus first-Send/archive/close behavior that creates no runtime request.
// POS: [L7 product tests] Conversation-visible metadata stays dormant until the existing explicit Send path.
package product

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

type developerSourceFixture struct {
	identity  hub.DeveloperRepositoryIdentity
	worktrees []hub.DeveloperWorktree
}

func (s developerSourceFixture) InspectDeveloperRepository(context.Context, string) (hub.DeveloperRepositoryIdentity, error) {
	return s.identity, nil
}

func (s developerSourceFixture) ListDeveloperWorktrees(_ context.Context, repositoryID, _ string) ([]hub.DeveloperWorktree, error) {
	result := append([]hub.DeveloperWorktree(nil), s.worktrees...)
	for index := range result {
		result[index].RepositoryID = repositoryID
	}
	return result, nil
}

func TestDeveloperServicesPersistNavigationAndArchiveWithoutExecution(t *testing.T) {
	service, requests := fixture(t)
	repositoryStore, ok := service.Store.(hub.DeveloperRepositoryStore)
	if !ok {
		t.Fatal("real product store lacks explicit repository capability")
	}
	tabStore, ok := service.Store.(hub.ConversationTabStore)
	if !ok {
		t.Fatal("real product store lacks conversation tab capability")
	}
	root := canonicalTempDir(t)
	common := filepath.Join(root, ".git")
	linked := filepath.Join(t.TempDir(), "linked")
	source := developerSourceFixture{
		identity: hub.DeveloperRepositoryIdentity{Path: root, CommonDir: common},
		worktrees: []hub.DeveloperWorktree{
			{Path: root, Name: filepath.Base(root), Branch: "main", Head: fixtureOID("a"), Main: true},
			{Path: linked, Name: filepath.Base(linked), Branch: "feature/tabs", Head: fixtureOID("b")},
		},
	}
	workspaces, err := NewDeveloperWorkspacesService(repositoryStore, source)
	if err != nil {
		t.Fatal(err)
	}
	repository, err := workspaces.Import(t.Context(), root)
	if err != nil {
		t.Fatal(err)
	}
	repositories, err := workspaces.Repositories(t.Context())
	if err != nil || len(repositories) != 1 || repositories[0] != repository {
		t.Fatal("explicit repository import was lost", repositories, err)
	}
	worktrees, err := workspaces.Worktrees(t.Context(), repository.ID)
	if err != nil || len(worktrees) != 2 || !worktrees[0].Main || worktrees[1].Branch != "feature/tabs" {
		t.Fatal("live worktree facade was not exposed", worktrees, err)
	}

	drafts, err := NewComposerDraftsService(service.Store.(hub.ComposerDraftStore))
	if err != nil {
		t.Fatal(err)
	}
	definitions, err := NewConversationTabsService(tabStore, service.DefaultTarget, drafts, workspaces)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = definitions.Close(context.Background())
		_ = drafts.Close(context.Background())
		_ = workspaces.Close(context.Background())
	})
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	tab := hub.ConversationTab{ID: "conversation-one", DraftID: hub.ConversationTabDraftID("conversation-one"), Title: "New conversation", CreatedAt: time.Now().UTC().UnixMilli(), SelectedAt: time.Now().UTC().UnixMilli()}
	tabs, err := definitions.Save(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
	if err != nil {
		t.Fatal(err)
	}
	draft, err := drafts.Save(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}, Text: "first real prompt"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	admitted, err := definitions.CreateFirstSession(t.Context(), draft, "First tab session", scope, tab.ID, tabs.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if admitted.Session.Workspace != scope.Path || admitted.Tabs.Tabs[0].SessionID != admitted.Session.ID || admitted.Draft.PendingSessionID != admitted.Session.ID {
		t.Fatal("first Send facade returned an invalid atomic binding", admitted)
	}
	cached, ok := drafts.Snapshot(draft.Scope)
	if !ok || cached.Revision != admitted.Draft.Revision || cached.PendingSessionID != admitted.Session.ID {
		t.Fatal("successful first Send did not publish its committed draft", cached, ok)
	}
	archived := admitted.Tabs
	archived.Tabs[0].Archived = true
	archived.Tabs[0].SelectedAt = 0
	archived.ActiveTabID = ""
	archived, err = definitions.Save(t.Context(), archived, admitted.Tabs.Revision)
	if err != nil || !archived.Tabs[0].Archived {
		t.Fatal("archive metadata save failed", archived, err)
	}
	detail, err := service.Store.Detail(t.Context(), admitted.Session.ID)
	if err != nil || detail.Session.Status != engine.Succeeded || service.Runtime.HasActive() {
		t.Fatal("archive or first-Send metadata changed execution", detail.Session, err)
	}
	select {
	case request := <-requests:
		t.Fatal("tab metadata unexpectedly started an Agent", request)
	default:
	}
	if err = definitions.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if _, err = definitions.Save(t.Context(), archived, archived.Revision); err == nil {
		t.Fatal("closed conversation-tab service still admitted a write")
	}
	if err = workspaces.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if _, err = workspaces.Repositories(t.Context()); err == nil {
		t.Fatal("closed developer-workspace service still admitted a read")
	}
}

// canonicalTempDir gives tests a real directory whose stored path already is
// its own symlink-free identity, matching what the product workspace gate accepts.
func canonicalTempDir(t *testing.T) string {
	t.Helper()
	directory, err := filepath.EvalSymlinks(filepath.Clean(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	return directory
}

func fixtureOID(value string) string {
	result := ""
	for len(result) < 40 {
		result += value
	}
	return result[:40]
}

func TestConversationTabFirstSendRequiresAtomicBackend(t *testing.T) {
	target := hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "none"}
	store := &readOnlyConversationTabStore{}
	service, err := NewConversationTabsService(store, target, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	scope := hub.WorktreeScope{RepositoryID: "repository-test", Path: filepath.Join(t.TempDir(), "worktree")}
	draft := hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: hub.ConversationTabDraftID("tab")}, Revision: 1, Text: "prompt"}
	if _, err = service.CreateFirstSession(t.Context(), draft, "title", scope, "tab", 1); err == nil {
		t.Fatal("backend without atomic first-Send admission silently fell back")
	}
}

type readOnlyConversationTabStore struct{}

func (*readOnlyConversationTabStore) ConversationTabs(_ context.Context, scope hub.WorktreeScope) (hub.ConversationTabSet, error) {
	return hub.ConversationTabSet{Scope: scope}, nil
}

func (*readOnlyConversationTabStore) SaveConversationTabs(_ context.Context, value hub.ConversationTabSet, expected uint64) (hub.ConversationTabSet, error) {
	if expected != value.Revision {
		return hub.ConversationTabSet{}, hub.ErrConversationTabsConflict
	}
	value.Revision++
	return value, nil
}

type developerRepositoryListFixture []hub.DeveloperRepository

func (f developerRepositoryListFixture) ListDeveloperRepositories(context.Context) ([]hub.DeveloperRepository, error) {
	return append([]hub.DeveloperRepository(nil), f...), nil
}

func (f developerRepositoryListFixture) ImportDeveloperRepository(_ context.Context, value hub.DeveloperRepository) (hub.DeveloperRepository, error) {
	return value, nil
}

type blockingWorktreeSource struct {
	identity hub.DeveloperRepositoryIdentity
	started  chan struct{}
	release  chan struct{}
}

func (s *blockingWorktreeSource) InspectDeveloperRepository(_ context.Context, path string) (hub.DeveloperRepositoryIdentity, error) {
	if path != s.identity.Path {
		return hub.DeveloperRepositoryIdentity{}, errors.New("unexpected inspection path")
	}
	return s.identity, nil
}

func (s *blockingWorktreeSource) ListDeveloperWorktrees(context.Context, string, string) ([]hub.DeveloperWorktree, error) {
	close(s.started)
	<-s.release
	return nil, nil
}

func TestDeveloperWorkspaceCloseCancelsAndJoinsInFlightSource(t *testing.T) {
	root := filepath.Clean(t.TempDir())
	repository := hub.DeveloperRepository{
		ID: "repository-close", Name: filepath.Base(root), Path: root,
		CommonDir: filepath.Join(root, ".git"), CreatedAt: 1,
	}
	source := &blockingWorktreeSource{
		identity: hub.DeveloperRepositoryIdentity{Path: repository.Path, CommonDir: repository.CommonDir},
		started:  make(chan struct{}), release: make(chan struct{}),
	}
	service, err := NewDeveloperWorkspacesService(developerRepositoryListFixture{repository}, source)
	if err != nil {
		t.Fatal(err)
	}
	result := make(chan error, 1)
	go func() {
		_, callErr := service.Worktrees(context.Background(), repository.ID)
		result <- callErr
	}()
	<-source.started
	closed := make(chan error, 1)
	go func() { closed <- service.Close(context.Background()) }()
	select {
	case err = <-closed:
		t.Fatal("Close returned before its in-flight Git read", err)
	case <-time.After(100 * time.Millisecond):
	}
	close(source.release)
	if err = <-result; !errors.Is(err, context.Canceled) {
		t.Fatal("canceled worktree read returned stale facts", err)
	}
	if err = <-closed; err != nil {
		t.Fatal(err)
	}
	if _, err = service.Repositories(t.Context()); err == nil {
		t.Fatal("closed developer-workspace service admitted a late read")
	}
}

type blockingConversationTabStore struct {
	started chan struct{}
	release chan struct{}
}

func (s *blockingConversationTabStore) ConversationTabs(_ context.Context, scope hub.WorktreeScope) (hub.ConversationTabSet, error) {
	close(s.started)
	<-s.release
	return hub.ConversationTabSet{Scope: scope}, nil
}

func (*blockingConversationTabStore) SaveConversationTabs(context.Context, hub.ConversationTabSet, uint64) (hub.ConversationTabSet, error) {
	return hub.ConversationTabSet{}, hub.ErrConversationTabsConflict
}

func TestConversationTabsCloseCancelsAndDropsLateRead(t *testing.T) {
	store := &blockingConversationTabStore{started: make(chan struct{}), release: make(chan struct{})}
	service, err := NewConversationTabsService(store, hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "none"}, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	scope := hub.WorktreeScope{RepositoryID: "repository-close", Path: filepath.Clean(t.TempDir())}
	result := make(chan error, 1)
	go func() {
		_, callErr := service.Read(context.Background(), scope)
		result <- callErr
	}()
	<-store.started
	closed := make(chan error, 1)
	go func() { closed <- service.Close(context.Background()) }()
	select {
	case err = <-closed:
		t.Fatal("Close returned before its in-flight tab read", err)
	case <-time.After(100 * time.Millisecond):
	}
	close(store.release)
	if err = <-result; !errors.Is(err, context.Canceled) {
		t.Fatal("canceled tab read returned a stale set", err)
	}
	if err = <-closed; err != nil {
		t.Fatal(err)
	}
	if _, err = service.Read(t.Context(), scope); err == nil {
		t.Fatal("closed conversation-tab service admitted a late read")
	}
}

type emptyComposerDraftStore struct{}

func (*emptyComposerDraftStore) ComposerDraft(_ context.Context, scope hub.ComposerDraftScope) (hub.ComposerDraft, error) {
	return hub.ComposerDraft{Scope: scope}, nil
}

func (*emptyComposerDraftStore) SaveComposerDraft(context.Context, hub.ComposerDraft, uint64) (hub.ComposerDraft, error) {
	return hub.ComposerDraft{}, hub.ErrComposerDraftConflict
}

type blockingAdmissionTabStore struct {
	started   chan struct{}
	release   chan struct{}
	admission hub.ConversationTabAdmission
}

func (*blockingAdmissionTabStore) ConversationTabs(_ context.Context, scope hub.WorktreeScope) (hub.ConversationTabSet, error) {
	return hub.ConversationTabSet{Scope: scope}, nil
}

func (*blockingAdmissionTabStore) SaveConversationTabs(context.Context, hub.ConversationTabSet, uint64) (hub.ConversationTabSet, error) {
	return hub.ConversationTabSet{}, hub.ErrConversationTabsConflict
}

func (s *blockingAdmissionTabStore) AdmitConversationTab(context.Context, hub.ComposerDraft, string, hub.WorktreeScope, string, uint64, hub.Target) (hub.ConversationTabAdmission, error) {
	close(s.started)
	<-s.release
	return s.admission, nil
}

func TestConversationTabsCanceledAdmissionDoesNotPublishDraft(t *testing.T) {
	drafts, err := NewComposerDraftsService(&emptyComposerDraftStore{})
	if err != nil {
		t.Fatal(err)
	}
	defer drafts.Close(context.Background())
	scope := hub.WorktreeScope{RepositoryID: "repository-late", Path: canonicalTempDir(t)}
	workspaces, err := NewDeveloperWorkspacesService(developerRepositoryListFixture{hub.DeveloperRepository{
		ID: scope.RepositoryID, Name: filepath.Base(scope.Path), Path: scope.Path,
		CommonDir: filepath.Join(scope.Path, ".git"), CreatedAt: 1,
	}}, developerSourceFixture{
		identity:  hub.DeveloperRepositoryIdentity{Path: scope.Path, CommonDir: filepath.Join(scope.Path, ".git")},
		worktrees: []hub.DeveloperWorktree{{Path: scope.Path, Name: filepath.Base(scope.Path), Branch: "main", Head: fixtureOID("a"), Main: true}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer workspaces.Close(context.Background())
	captured := hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: hub.ConversationTabDraftID("late-tab")}, Revision: 1, Text: "captured"}
	sessionID := "hub-late-admission"
	tab := hub.ConversationTab{
		ID: "late-tab", SessionID: sessionID, DraftID: captured.Scope.ID, Title: "Late admission",
		CreatedAt: 1, SelectedAt: 1,
	}
	store := &blockingAdmissionTabStore{
		started: make(chan struct{}), release: make(chan struct{}),
		admission: hub.ConversationTabAdmission{
			Session: hub.Session{ID: sessionID, Workspace: scope.Path},
			Draft:   hub.ComposerDraft{Scope: captured.Scope, Revision: 2, Text: captured.Text, PendingSessionID: sessionID},
			Tabs:    hub.ConversationTabSet{Scope: scope, Revision: 1, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID},
		},
	}
	service, err := NewConversationTabsService(store, hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "none"}, drafts, workspaces)
	if err != nil {
		t.Fatal(err)
	}
	result := make(chan error, 1)
	go func() {
		_, callErr := service.CreateFirstSession(context.Background(), captured, "Late admission", scope, tab.ID, 0)
		result <- callErr
	}()
	<-store.started
	closed := make(chan error, 1)
	go func() { closed <- service.Close(context.Background()) }()
	select {
	case err = <-closed:
		t.Fatal("Close returned before its in-flight admission", err)
	case <-time.After(100 * time.Millisecond):
	}
	close(store.release)
	if err = <-result; !errors.Is(err, context.Canceled) {
		t.Fatal("canceled admission reported success", err)
	}
	if err = <-closed; err != nil {
		t.Fatal(err)
	}
	if value, published := drafts.Snapshot(captured.Scope); published {
		t.Fatal("late admission published a draft after its owner was canceled", value)
	}
}

type staleIdentitySource struct {
	identity  hub.DeveloperRepositoryIdentity
	worktrees []hub.DeveloperWorktree
	listings  int
}

func (s *staleIdentitySource) InspectDeveloperRepository(_ context.Context, path string) (hub.DeveloperRepositoryIdentity, error) {
	if path != s.identity.Path {
		return hub.DeveloperRepositoryIdentity{}, errors.New("unexpected inspection path")
	}
	return s.identity, nil
}

func (s *staleIdentitySource) ListDeveloperWorktrees(_ context.Context, repositoryID, repositoryPath string) ([]hub.DeveloperWorktree, error) {
	s.listings++
	if repositoryPath != s.worktrees[0].Path && repositoryPath != s.identity.Path {
		return nil, errors.New("unexpected listing root")
	}
	result := append([]hub.DeveloperWorktree(nil), s.worktrees...)
	for index := range result {
		result[index].RepositoryID = repositoryID
	}
	return result, nil
}

// TestDeveloperWorkspacesNavigationRejectsSavedIdentityWithChangedCommonDir
// proves the saved repository row is re-verified read-only against the approved
// Git owner: a same-directory .git now pointing at another common root must not
// lend its live members to the older repository identity.
func TestDeveloperWorkspacesNavigationRejectsSavedIdentityWithChangedCommonDir(t *testing.T) {
	root := canonicalTempDir(t)
	source := &staleIdentitySource{
		identity:  hub.DeveloperRepositoryIdentity{Path: root, CommonDir: filepath.Join(root, ".git")},
		worktrees: []hub.DeveloperWorktree{{Path: root, Name: filepath.Base(root), Branch: "main", Head: fixtureOID("a"), Main: true}},
	}
	service, err := NewDeveloperWorkspacesService(developerRepositoryListFixture{hub.DeveloperRepository{
		ID: "repository-stale", Name: filepath.Base(root), Path: root,
		CommonDir: filepath.Join(root, ".git"), CreatedAt: 1,
	}}, source)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close(context.Background())
	if _, err = service.Worktrees(t.Context(), "repository-stale"); err != nil {
		t.Fatal("first live listing of a matching saved identity failed", err)
	}
	source.identity.CommonDir = filepath.Join(canonicalTempDir(t), "common-elsewhere")
	if _, err = service.Worktrees(t.Context(), "repository-stale"); !errors.Is(err, hub.ErrDeveloperRepositoryConflict) {
		t.Fatal("changed Git common dir reused the saved repository identity", err)
	}
	if source.listings != 1 {
		t.Fatalf("changed identity still enumerated another repository's worktree members: %d listings", source.listings)
	}
}

func productDraftTab(id string, selected int64) hub.ConversationTab {
	return hub.ConversationTab{
		ID: id, DraftID: hub.ConversationTabDraftID(id), Title: "Live worktree chat",
		CreatedAt: time.Now().UTC().UnixMilli(), SelectedAt: selected,
	}
}

func productSessionCount(t *testing.T, service *HubService) int {
	t.Helper()
	sessions, err := service.Store.ListSessions(t.Context(), 256)
	if err != nil {
		t.Fatal(err)
	}
	return len(sessions)
}

// TestConversationTabFirstSendBindsOnlyLiveWorktreeMember covers the C1 gate:
// the tab scope must still be an existing canonical directory and a live member
// of the same repository identity before anything is written, and a retry of a
// committed admission returns that same session from the real store.
func TestConversationTabFirstSendBindsOnlyLiveWorktreeMember(t *testing.T) {
	service, requests := fixture(t)
	root := canonicalTempDir(t)
	source := developerSourceFixture{
		identity:  hub.DeveloperRepositoryIdentity{Path: root, CommonDir: filepath.Join(root, ".git")},
		worktrees: []hub.DeveloperWorktree{{Path: root, Name: filepath.Base(root), Branch: "main", Head: fixtureOID("a"), Main: true}},
	}
	workspaces, err := NewDeveloperWorkspacesService(service.Store.(hub.DeveloperRepositoryStore), source)
	if err != nil {
		t.Fatal(err)
	}
	drafts, err := NewComposerDraftsService(service.Store.(hub.ComposerDraftStore))
	if err != nil {
		t.Fatal(err)
	}
	definitions, err := NewConversationTabsService(service.Store.(hub.ConversationTabStore), service.DefaultTarget, drafts, workspaces)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = definitions.Close(context.Background())
		_ = drafts.Close(context.Background())
		_ = workspaces.Close(context.Background())
	})
	if productSessionCount(t, service) != 0 {
		t.Fatal("fixture started with an existing session")
	}
	repository, err := workspaces.Import(t.Context(), root)
	if err != nil {
		t.Fatal(err)
	}
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: root}
	tab := productDraftTab("live-member", time.Now().UTC().UnixMilli())
	tabs, err := definitions.Save(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
	if err != nil {
		t.Fatal(err)
	}
	captured, err := drafts.Save(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}, Text: "live worktree prompt"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	admitted, err := definitions.CreateFirstSession(t.Context(), captured, "Live member", scope, tab.ID, tabs.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if admitted.Session.Workspace != root || admitted.Tabs.Revision != tabs.Revision+1 || admitted.Draft.Revision != captured.Revision+1 {
		t.Fatal("live-member first Send returned an unexpected receipt", admitted)
	}
	if count := productSessionCount(t, service); count != 1 {
		t.Fatalf("live-member first Send created %d sessions", count)
	}
	// Same captured draft and same expected revision: the retry returns that one
	// admitted session instead of creating a second.
	retry, err := definitions.CreateFirstSession(t.Context(), captured, "Live member", scope, tab.ID, tabs.Revision)
	if err != nil {
		t.Fatal("retry of the first Send failed", err)
	}
	if retry.Session.ID != admitted.Session.ID || retry.Draft.Revision != admitted.Draft.Revision || retry.Tabs.Revision != admitted.Tabs.Revision {
		t.Fatal("retry reported a different receipt", retry)
	}
	// A caller that already holds the committed receipt retries with it and still
	// converges on the real stored facts without replaying the write.
	committed, err := definitions.CreateFirstSession(t.Context(), admitted.Draft, "Live member", scope, tab.ID, admitted.Tabs.Revision)
	if err != nil {
		t.Fatal("committed first-Send retry failed", err)
	}
	if committed.Session.ID != admitted.Session.ID || committed.Draft.Revision != admitted.Draft.Revision || committed.Tabs.Revision != admitted.Tabs.Revision {
		t.Fatal("committed first-Send retry changed durable facts", committed)
	}
	if count := productSessionCount(t, service); count != 1 {
		t.Fatalf("first-Send retries created %d sessions", count)
	}
	cached, ok := drafts.Snapshot(captured.Scope)
	if !ok || cached.Revision != admitted.Draft.Revision || cached.PendingSessionID != admitted.Session.ID {
		t.Fatal("live-member first Send did not publish its committed draft", cached, ok)
	}
	archived := admitted.Tabs
	archived.Tabs[0].Archived, archived.Tabs[0].SelectedAt, archived.ActiveTabID = true, 0, ""
	archived, err = definitions.Save(t.Context(), archived, admitted.Tabs.Revision)
	if err != nil || !archived.Tabs[0].Archived {
		t.Fatal("archive metadata save failed", archived, err)
	}
	if _, err = definitions.CreateFirstSession(t.Context(), admitted.Draft, "Live member", scope, tab.ID, archived.Revision); !errors.Is(err, hub.ErrConversationTabsConflict) {
		t.Fatal("archived tab first Send was not rejected as a conflict", err)
	}
	if count := productSessionCount(t, service); count != 1 {
		t.Fatalf("archived tab first Send created %d sessions", count)
	}
	if detail, detailErr := service.Store.Detail(t.Context(), admitted.Session.ID); detailErr != nil || detail.Session.Workspace != root {
		t.Fatal("first Send did not create its single live-worktree session", detail, detailErr)
	}
	if len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("conversation tab first Send started execution")
	}
}

// TestConversationTabFirstSendRejectsStaleWorktreeWithoutWrites is the C1
// regression: a removed directory, a scope that is no longer a live member, a
// path whose symlink identity differs from its canonical directory, missing live
// navigation, and a saved repository whose Git common dir changed must all
// leave zero sessions, zero tab bindings and the captured draft untouched.
func TestConversationTabFirstSendRejectsStaleWorktreeWithoutWrites(t *testing.T) {
	for _, variant := range []struct {
		name       string
		source     func(t *testing.T, root string) (hub.DeveloperWorktreeSource, string)
		invalidate func(t *testing.T, root string, worktree string)
	}{
		{
			name: "directory-removed",
			source: func(_ *testing.T, root string) (hub.DeveloperWorktreeSource, string) {
				return developerSourceFixture{
					identity:  hub.DeveloperRepositoryIdentity{Path: root, CommonDir: filepath.Join(root, ".git")},
					worktrees: []hub.DeveloperWorktree{{Path: root, Name: filepath.Base(root), Branch: "main", Head: fixtureOID("a"), Main: true}},
				}, root
			},
			invalidate: func(t *testing.T, root string, _ string) {
				if err := os.RemoveAll(root); err != nil {
					t.Fatal(err)
				}
			},
		},
		{
			name: "scope-no-longer-a-live-member",
			source: func(_ *testing.T, root string) (hub.DeveloperWorktreeSource, string) {
				return &staleIdentitySource{
					identity:  hub.DeveloperRepositoryIdentity{Path: root, CommonDir: filepath.Join(root, ".git")},
					worktrees: []hub.DeveloperWorktree{{Path: root, Name: filepath.Base(root), Branch: "main", Head: fixtureOID("a"), Main: true}},
				}, root
			},
			// The saved repository identity still resolves; only the live member
			// list no longer contains the tab scope path.
		},
		{
			name: "symlink-identity-changed",
			source: func(t *testing.T, root string) (hub.DeveloperWorktreeSource, string) {
				target := filepath.Join(root, "canonical-target")
				if err := os.Mkdir(target, 0o700); err != nil {
					t.Fatal(err)
				}
				link := filepath.Join(root, "worktree-link")
				if err := os.Symlink(target, link); err != nil {
					t.Fatal(err)
				}
				return developerSourceFixture{
					identity:  hub.DeveloperRepositoryIdentity{Path: link, CommonDir: filepath.Join(link, ".git")},
					worktrees: []hub.DeveloperWorktree{{Path: link, Name: filepath.Base(link), Branch: "main", Head: fixtureOID("c"), Main: true}},
				}, link
			},
		},
	} {
		t.Run(variant.name, func(t *testing.T) {
			service, requests := fixture(t)
			root := canonicalTempDir(t)
			source, worktree := variant.source(t, root)
			workspaces, err := NewDeveloperWorkspacesService(service.Store.(hub.DeveloperRepositoryStore), source)
			if err != nil {
				t.Fatal(err)
			}
			definitions, err := NewConversationTabsService(service.Store.(hub.ConversationTabStore), service.DefaultTarget, nil, workspaces)
			if err != nil {
				t.Fatal(err)
			}
			drafts, err := NewComposerDraftsService(service.Store.(hub.ComposerDraftStore))
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				_ = definitions.Close(context.Background())
				_ = drafts.Close(context.Background())
				_ = workspaces.Close(context.Background())
			})
			repository, err := workspaces.Import(t.Context(), worktree)
			if err != nil {
				t.Fatal(err)
			}
			scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: worktree}
			tab := productDraftTab("stale-"+variant.name, time.Now().UTC().UnixMilli())
			tabs, err := definitions.Save(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
			if err != nil {
				t.Fatal(err)
			}
			captured, err := drafts.Save(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}, Text: "stale worktree prompt"}, 0)
			if err != nil {
				t.Fatal(err)
			}
			if mutable, ok := source.(*staleIdentitySource); ok {
				member := canonicalTempDir(t)
				mutable.worktrees = []hub.DeveloperWorktree{{Path: member, Name: filepath.Base(member), Branch: "main", Head: fixtureOID("b"), Main: true}}
			}
			if variant.invalidate != nil {
				variant.invalidate(t, root, worktree)
			}
			if _, err = definitions.CreateFirstSession(t.Context(), captured, "Stale worktree", scope, tab.ID, tabs.Revision); err == nil {
				t.Fatal("first Send bound a stale worktree scope")
			}
			if count := productSessionCount(t, service); count != 0 {
				t.Fatalf("rejected first Send created %d sessions", count)
			}
			current, readErr := drafts.Read(t.Context(), captured.Scope)
			if readErr != nil || current.Revision != captured.Revision || current.PendingSessionID != "" {
				t.Fatal("rejected first Send changed or pinned the draft", current, readErr)
			}
			if cached, ok := drafts.Snapshot(captured.Scope); ok && cached.PendingSessionID != "" {
				t.Fatal("rejected first Send published a pinned draft", cached)
			}
			stored, readErr := definitions.Read(t.Context(), scope)
			if readErr != nil || stored.Revision != tabs.Revision || stored.Tabs[0].SessionID != "" {
				t.Fatal("rejected first Send bound the tab", stored, readErr)
			}
			if len(requests) != 0 || service.Runtime.HasActive() {
				t.Fatal("rejected first Send started execution")
			}
		})
	}
	t.Run("navigation-capability-missing", func(t *testing.T) {
		service, _ := fixture(t)
		root := canonicalTempDir(t)
		definitions, err := NewConversationTabsService(service.Store.(hub.ConversationTabStore), service.DefaultTarget, nil, nil)
		if err != nil {
			t.Fatal(err)
		}
		drafts, err := NewComposerDraftsService(service.Store.(hub.ComposerDraftStore))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			_ = definitions.Close(context.Background())
			_ = drafts.Close(context.Background())
		})
		repository, err := service.Store.(hub.DeveloperRepositoryStore).ImportDeveloperRepository(t.Context(), hub.DeveloperRepository{
			ID: "repository-no-git", Name: filepath.Base(root), Path: root, CommonDir: filepath.Join(root, ".git"), CreatedAt: 1,
		})
		if err != nil {
			t.Fatal(err)
		}
		scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: root}
		tab := productDraftTab("no-navigation", time.Now().UTC().UnixMilli())
		tabs, err := definitions.Save(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
		if err != nil {
			t.Fatal("conversation tab metadata became unavailable without live navigation", err)
		}
		captured, err := drafts.Save(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}, Text: "no navigation prompt"}, 0)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = definitions.CreateFirstSession(t.Context(), captured, "No navigation", scope, tab.ID, tabs.Revision); err == nil {
			t.Fatal("first Send succeeded without live worktree navigation")
		}
		if count := productSessionCount(t, service); count != 0 {
			t.Fatalf("fail-closed first Send created %d sessions", count)
		}
		current, readErr := drafts.Read(t.Context(), captured.Scope)
		if readErr != nil || current.Revision != captured.Revision || current.PendingSessionID != "" {
			t.Fatal("fail-closed first Send changed the draft", current, readErr)
		}
		stored, readErr := definitions.Read(t.Context(), scope)
		if readErr != nil || stored.Revision != tabs.Revision || stored.Tabs[0].SessionID != "" {
			t.Fatal("fail-closed first Send bound the tab", stored, readErr)
		}
	})
	t.Run("repository-common-dir-changed", func(t *testing.T) {
		service, _ := fixture(t)
		root := canonicalTempDir(t)
		source := &staleIdentitySource{
			identity:  hub.DeveloperRepositoryIdentity{Path: root, CommonDir: filepath.Join(root, ".git")},
			worktrees: []hub.DeveloperWorktree{{Path: root, Name: filepath.Base(root), Branch: "main", Head: fixtureOID("a"), Main: true}},
		}
		workspaces, err := NewDeveloperWorkspacesService(service.Store.(hub.DeveloperRepositoryStore), source)
		if err != nil {
			t.Fatal(err)
		}
		definitions, err := NewConversationTabsService(service.Store.(hub.ConversationTabStore), service.DefaultTarget, nil, workspaces)
		if err != nil {
			t.Fatal(err)
		}
		drafts, err := NewComposerDraftsService(service.Store.(hub.ComposerDraftStore))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			_ = definitions.Close(context.Background())
			_ = drafts.Close(context.Background())
			_ = workspaces.Close(context.Background())
		})
		repository, err := workspaces.Import(t.Context(), root)
		if err != nil {
			t.Fatal(err)
		}
		scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: root}
		tab := productDraftTab("changed-common-dir", time.Now().UTC().UnixMilli())
		tabs, err := definitions.Save(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
		if err != nil {
			t.Fatal(err)
		}
		captured, err := drafts.Save(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}, Text: "changed identity prompt"}, 0)
		if err != nil {
			t.Fatal(err)
		}
		source.identity.CommonDir = filepath.Join(canonicalTempDir(t), "common-elsewhere")
		if _, err = definitions.CreateFirstSession(t.Context(), captured, "Changed identity", scope, tab.ID, tabs.Revision); !errors.Is(err, hub.ErrDeveloperRepositoryConflict) {
			t.Fatal("first Send trusted a repository whose Git common dir changed", err)
		}
		if source.listings != 0 {
			t.Fatalf("changed repository identity still enumerated members: %d listings", source.listings)
		}
		if count := productSessionCount(t, service); count != 0 {
			t.Fatalf("changed repository identity created %d sessions", count)
		}
		current, readErr := drafts.Read(t.Context(), captured.Scope)
		if readErr != nil || current.Revision != captured.Revision || current.PendingSessionID != "" {
			t.Fatal("changed repository identity changed the draft", current, readErr)
		}
		stored, readErr := definitions.Read(t.Context(), scope)
		if readErr != nil || stored.Revision != tabs.Revision || stored.Tabs[0].SessionID != "" {
			t.Fatal("changed repository identity bound the tab", stored, readErr)
		}
	})
}
