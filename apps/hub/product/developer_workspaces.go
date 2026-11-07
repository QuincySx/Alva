// INPUT: Explicit repository/tab stores, approved Git source, captured home draft and caller context.
// OUTPUT: DeveloperWorkspaces and ConversationTabs services with CAS, atomic first-Send receipts and joined shutdown.
// POS: [L7 product service] Read-only live Git navigation and conversation-view metadata never own execution.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
)

const maxDeveloperWorktrees = 1024

type serviceOwner struct {
	mu     sync.Mutex
	closed bool
	next   uint64
	active map[uint64]context.CancelFunc
	joined chan struct{}
}

func newServiceOwner() serviceOwner {
	return serviceOwner{active: make(map[uint64]context.CancelFunc), joined: make(chan struct{})}
}

func (o *serviceOwner) begin(ctx context.Context) (context.Context, func(), error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	if o.closed {
		return nil, nil, errors.New("developer workspace owner is closed")
	}
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	o.next++
	id := o.next
	o.active[id] = cancel
	return ctx, func() {
		cancel()
		o.mu.Lock()
		defer o.mu.Unlock()
		delete(o.active, id)
		o.finishLocked()
	}, nil
}

func (o *serviceOwner) finishLocked() {
	if o.closed && len(o.active) == 0 {
		select {
		case <-o.joined:
		default:
			close(o.joined)
		}
	}
}

func (o *serviceOwner) close(ctx context.Context) error {
	o.mu.Lock()
	o.closed = true
	for _, cancel := range o.active {
		cancel()
	}
	o.finishLocked()
	joined := o.joined
	o.mu.Unlock()
	select {
	case <-joined:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// DeveloperWorkspacesService exposes only explicit imports and live approved-Git
// worktree facts. It neither scans directories nor writes execution/product sessions.
type DeveloperWorkspacesService struct {
	store  hub.DeveloperRepositoryStore
	source hub.DeveloperWorktreeSource
	owner  serviceOwner
}

func NewDeveloperWorkspacesService(store hub.DeveloperRepositoryStore, source hub.DeveloperWorktreeSource) (*DeveloperWorkspacesService, error) {
	if storage.Missing(store) || storage.Missing(source) {
		return nil, errors.New("developer repository store and approved Git worktree source are required")
	}
	return &DeveloperWorkspacesService{store: store, source: source, owner: newServiceOwner()}, nil
}

func (s *DeveloperWorkspacesService) Repositories(ctx context.Context) ([]hub.DeveloperRepository, error) {
	ctx, end, err := s.owner.begin(ctx)
	if err != nil {
		return nil, err
	}
	defer end()
	repositories, err := s.store.ListDeveloperRepositories(ctx)
	if err != nil {
		return nil, err
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	ids, paths, common := make(map[string]bool, len(repositories)), make(map[string]bool, len(repositories)), make(map[string]bool, len(repositories))
	for _, repository := range repositories {
		if err = repository.Validate(); err != nil || ids[repository.ID] || paths[repository.Path] || common[repository.CommonDir] {
			return nil, errors.New("developer repository store returned an invalid or duplicate identity")
		}
		ids[repository.ID], paths[repository.Path], common[repository.CommonDir] = true, true, true
	}
	return append([]hub.DeveloperRepository(nil), repositories...), nil
}

func (s *DeveloperWorkspacesService) Import(ctx context.Context, path string) (hub.DeveloperRepository, error) {
	ctx, end, err := s.owner.begin(ctx)
	if err != nil {
		return hub.DeveloperRepository{}, err
	}
	defer end()
	identity, err := s.source.InspectDeveloperRepository(ctx, path)
	if err != nil {
		return hub.DeveloperRepository{}, err
	}
	if err = identity.Validate(); err != nil {
		return hub.DeveloperRepository{}, errors.New("approved Git source returned an invalid repository identity")
	}
	id, err := hub.NewID("repository-")
	if err != nil {
		return hub.DeveloperRepository{}, err
	}
	value, err := s.store.ImportDeveloperRepository(ctx, hub.DeveloperRepository{
		ID: id, Name: filepath.Base(identity.Path), Path: identity.Path, CommonDir: identity.CommonDir,
		CreatedAt: time.Now().UTC().UnixMilli(),
	})
	if err != nil {
		return hub.DeveloperRepository{}, err
	}
	if err = ctx.Err(); err != nil {
		return hub.DeveloperRepository{}, err
	}
	if value.Validate() != nil || value.Path != identity.Path || value.CommonDir != identity.CommonDir {
		return hub.DeveloperRepository{}, errors.New("developer repository import returned an invalid identity receipt")
	}
	return value, nil
}

func (s *DeveloperWorkspacesService) Worktrees(ctx context.Context, repositoryID string) ([]hub.DeveloperWorktree, error) {
	ctx, end, err := s.owner.begin(ctx)
	if err != nil {
		return nil, err
	}
	defer end()
	repositories, err := s.store.ListDeveloperRepositories(ctx)
	if err != nil {
		return nil, err
	}
	var repository hub.DeveloperRepository
	for _, candidate := range repositories {
		if candidate.ID == repositoryID {
			repository = candidate
			break
		}
	}
	if repository.ID == "" || repository.Validate() != nil {
		return nil, hub.ErrNotFound
	}
	// The saved repository identity is re-verified read-only against the approved
	// Git owner before its members are trusted: a same-directory .git that now
	// points at another common root must not lend its live members to this row.
	// Import is never used here because it would write.
	fresh, err := s.source.InspectDeveloperRepository(ctx, repository.Path)
	if err != nil {
		return nil, err
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	if fresh.Path != repository.Path || fresh.CommonDir != repository.CommonDir {
		return nil, hub.ErrDeveloperRepositoryConflict
	}
	worktrees, err := s.source.ListDeveloperWorktrees(ctx, repository.ID, repository.Path)
	if err != nil {
		return nil, err
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	if len(worktrees) == 0 || len(worktrees) > maxDeveloperWorktrees {
		return nil, errors.New("approved Git source returned an empty or oversized worktree list")
	}
	paths, main := make(map[string]bool, len(worktrees)), 0
	for index, worktree := range worktrees {
		if err = worktree.Validate(repository.ID); err != nil || paths[worktree.Path] {
			return nil, errors.New("approved Git source returned an invalid or duplicate worktree")
		}
		paths[worktree.Path] = true
		if worktree.Main {
			main++
			if index != 0 {
				return nil, errors.New("approved Git source did not return the main worktree first")
			}
		}
	}
	if main != 1 {
		return nil, errors.New("approved Git source needs exactly one main worktree")
	}
	return append([]hub.DeveloperWorktree(nil), worktrees...), nil
}

func (s *DeveloperWorkspacesService) Close(ctx context.Context) error {
	return s.owner.close(ctx)
}

// ConversationTabsService persists conversation-level tab metadata. It never
// controls model execution, closes sessions or removes Git worktrees.
type ConversationTabsService struct {
	store      hub.ConversationTabStore
	admission  hub.ConversationTabAdmissionStore
	target     hub.Target
	drafts     *ComposerDraftsService
	workspaces *DeveloperWorkspacesService
	owner      serviceOwner
}

// NewConversationTabsService takes the optional live developer-navigation owner
// explicitly: without it Read/Save metadata stays available, but first Send has
// no way to prove its scope is still a live worktree, so it fails closed.
func NewConversationTabsService(store hub.ConversationTabStore, target hub.Target, drafts *ComposerDraftsService, workspaces *DeveloperWorkspacesService) (*ConversationTabsService, error) {
	if storage.Missing(store) {
		return nil, errors.New("conversation tab store is required")
	}
	if err := target.Validate(); err != nil {
		return nil, err
	}
	service := &ConversationTabsService{store: store, target: target, drafts: drafts, workspaces: workspaces, owner: newServiceOwner()}
	service.admission, _ = store.(hub.ConversationTabAdmissionStore)
	return service, nil
}

func (s *ConversationTabsService) Read(ctx context.Context, scope hub.WorktreeScope) (hub.ConversationTabSet, error) {
	if err := scope.Validate(); err != nil {
		return hub.ConversationTabSet{}, err
	}
	ctx, end, err := s.owner.begin(ctx)
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	defer end()
	value, err := s.store.ConversationTabs(ctx, scope)
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	if err = ctx.Err(); err != nil {
		return hub.ConversationTabSet{}, err
	}
	if value.Scope != scope || value.Validate() != nil {
		return hub.ConversationTabSet{}, errors.New("conversation tab store returned an invalid or foreign read receipt")
	}
	return hub.CloneConversationTabSet(value), nil
}

func (s *ConversationTabsService) Save(ctx context.Context, next hub.ConversationTabSet, expected uint64) (hub.ConversationTabSet, error) {
	next = hub.CloneConversationTabSet(next)
	if expected >= 1<<63-1 {
		return hub.ConversationTabSet{}, hub.ErrConversationTabsConflict
	}
	if err := next.Validate(); err != nil {
		return hub.ConversationTabSet{}, err
	}
	ctx, end, err := s.owner.begin(ctx)
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	defer end()
	value, err := s.store.SaveConversationTabs(ctx, hub.CloneConversationTabSet(next), expected)
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	if err = ctx.Err(); err != nil {
		return hub.ConversationTabSet{}, err
	}
	next.Revision = expected + 1
	if value.Validate() != nil || !reflect.DeepEqual(value, next) {
		return hub.ConversationTabSet{}, errors.New("conversation tab save returned an invalid receipt; reload before saving again")
	}
	return hub.CloneConversationTabSet(value), nil
}

// CreateFirstSession performs the only explicit tab-to-HubSession binding. It
// creates no Turn and starts no model; unsupported backends fail without fallback.
func (s *ConversationTabsService) CreateFirstSession(ctx context.Context, captured hub.ComposerDraft, title string, scope hub.WorktreeScope, tabID string, expected uint64) (hub.ConversationTabAdmission, error) {
	captured = hub.CloneComposerDraft(captured)
	if _, err := DecodeComposerDraftPreparations(captured); err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	if err := scope.Validate(); err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	if captured.Scope.Kind != "home" || captured.Revision == 0 || captured.Revision >= 1<<63-1 ||
		captured.Scope.ID != hub.ConversationTabDraftID(tabID) || expected >= 1<<63-1 ||
		strings.TrimSpace(title) == "" || len(title) > 256 || !utf8.ValidString(title) || strings.ContainsRune(title, 0) {
		return hub.ConversationTabAdmission{}, errors.New("conversation tab first Send needs its committed home draft, title and current revision")
	}
	if s.admission == nil {
		return hub.ConversationTabAdmission{}, errors.New("conversation tab first-Send admission is unavailable")
	}
	ctx, end, err := s.owner.begin(ctx)
	if err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	defer end()
	canonical, err := s.liveWorktreeScope(ctx, scope)
	if err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	scope.Path = canonical
	value, err := s.admission.AdmitConversationTab(ctx, captured, title, scope, tabID, expected, s.target)
	if err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	if err = ctx.Err(); err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	if err = validateConversationTabAdmission(captured, title, scope, tabID, expected, value); err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	if s.drafts != nil {
		if err = s.drafts.publish(value.Draft); err != nil {
			return hub.ConversationTabAdmission{}, err
		}
	}
	value.Draft = hub.CloneComposerDraft(value.Draft)
	value.Tabs = hub.CloneConversationTabSet(value.Tabs)
	return value, nil
}

// liveWorktreeScope is the verify-only admission gate for a developer tab's
// first Send: the scope path must still exist and resolve to the same canonical
// path, and it must still be a live member of the same repository identity.
// Nothing is ever created, released or repaired here.
func (s *ConversationTabsService) liveWorktreeScope(ctx context.Context, scope hub.WorktreeScope) (string, error) {
	canonical, err := verifyExistingCanonicalDirectory(scope.Path)
	if err != nil {
		return "", errors.New("conversation tab worktree must be an existing directory")
	}
	if canonical != scope.Path {
		return "", errors.New("conversation tab worktree path is not its canonical directory; refresh the worktree navigation")
	}
	if s.workspaces == nil {
		return "", errors.New("conversation tab first Send needs live developer worktree navigation")
	}
	worktrees, err := s.workspaces.Worktrees(ctx, scope.RepositoryID)
	if err != nil {
		return "", err
	}
	for _, worktree := range worktrees {
		if worktree.Path == canonical {
			return canonical, nil
		}
	}
	return "", errors.New("conversation tab worktree is no longer a live worktree of this repository; refresh the worktree navigation")
}

func validateConversationTabAdmission(captured hub.ComposerDraft, title string, scope hub.WorktreeScope, tabID string, expected uint64, value hub.ConversationTabAdmission) error {
	invalid := errors.New("conversation tab admission returned an invalid receipt; reload before retrying")
	draftRevision := captured.Revision + 1
	tabsRevision := expected + 1
	if captured.PendingSessionID != "" {
		draftRevision = captured.Revision
		tabsRevision = expected
	}
	check := hub.CloneComposerDraft(value.Draft)
	check.Revision, check.PendingSessionID = captured.Revision, captured.PendingSessionID
	if value.Session.ID == "" || value.Session.Workspace != scope.Path ||
		value.Draft.Scope != captured.Scope || value.Draft.PendingSessionID != value.Session.ID ||
		value.Draft.Revision != draftRevision || !reflect.DeepEqual(check, captured) ||
		value.Tabs.Scope != scope || value.Tabs.Revision != tabsRevision || value.Tabs.Validate() != nil {
		return invalid
	}
	found := false
	for _, tab := range value.Tabs.Tabs {
		if tab.ID != tabID {
			continue
		}
		found = tab.SessionID == value.Session.ID && tab.DraftID == captured.Scope.ID && !tab.Archived
	}
	if !found {
		return invalid
	}
	return nil
}

func (s *ConversationTabsService) Close(ctx context.Context) error {
	return s.owner.close(ctx)
}
