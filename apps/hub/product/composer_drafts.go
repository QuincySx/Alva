// INPUT: Explicit scoped draft storage, frozen composer preparations and caller cancellation.
// OUTPUT: ComposerDraftsService with shared committed snapshots, CAS consumption and joined shutdown.
// POS: [L7 product service] Owns manual draft persistence without executing, replaying or expiring input.
package product

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"github.com/QuincySx/Alva/platform/storage"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type ComposerDraftsService struct {
	store   hub.ComposerDraftStore
	access  chan struct{}
	stopped chan struct{}
	mu      sync.Mutex
	closed  bool
	cancel  context.CancelFunc
	done    chan struct{}
	cacheMu sync.RWMutex
	cache   map[hub.ComposerDraftScope]hub.ComposerDraft
}

func NewComposerDraftsService(store hub.ComposerDraftStore) (*ComposerDraftsService, error) {
	if storage.Missing(store) {
		return nil, errors.New("composer draft storage is required")
	}
	s := &ComposerDraftsService{store: store, access: make(chan struct{}, 1), stopped: make(chan struct{}), cache: make(map[hub.ComposerDraftScope]hub.ComposerDraft)}
	s.access <- struct{}{}
	return s, nil
}

// Snapshot performs no storage I/O. A missing observation is distinct from a
// successfully read empty draft; callers load a scope outside Build/poll first.
func (s *ComposerDraftsService) Snapshot(scope hub.ComposerDraftScope) (hub.ComposerDraft, bool) {
	s.cacheMu.RLock()
	defer s.cacheMu.RUnlock()
	value, ok := s.cache[scope]
	return hub.CloneComposerDraft(value), ok
}

func (s *ComposerDraftsService) Read(ctx context.Context, scope hub.ComposerDraftScope) (hub.ComposerDraft, error) {
	if err := scope.Validate(); err != nil {
		return hub.ComposerDraft{}, err
	}
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	defer s.release()
	value, err := s.store.ComposerDraft(ctx, scope)
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	if value.Scope != scope || value.Revision == 0 && (value.Text != "" || value.PendingSessionID != "" || len(value.References) != 0 || len(value.PreparedCommands) != 0) {
		return hub.ComposerDraft{}, errors.New("composer draft store returned an invalid read receipt")
	}
	if err = s.publish(value); err != nil {
		return hub.ComposerDraft{}, err
	}
	return hub.CloneComposerDraft(value), nil
}

func (s *ComposerDraftsService) Save(ctx context.Context, next hub.ComposerDraft, expected uint64) (hub.ComposerDraft, error) {
	next = hub.CloneComposerDraft(next)
	if _, err := DecodeComposerDraftPreparations(next); err != nil {
		return hub.ComposerDraft{}, err
	}
	if expected >= 1<<63-1 {
		return hub.ComposerDraft{}, hub.ErrComposerDraftConflict
	}
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	defer s.release()
	value, err := s.store.SaveComposerDraft(ctx, hub.CloneComposerDraft(next), expected)
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	next.Revision = expected + 1
	if !reflect.DeepEqual(hub.CloneComposerDraft(value), next) {
		return hub.ComposerDraft{}, errors.New("composer draft save returned an invalid receipt; read the committed draft before retrying")
	}
	if err = s.publish(value); err != nil {
		return hub.ComposerDraft{}, err
	}
	return hub.CloneComposerDraft(value), nil
}

// Consume is called only after the captured submission was actually admitted.
// A newer revision (including text edited back to its original value) remains a
// manual draft. Empty successful consumption retains the monotonic revision.
func (s *ComposerDraftsService) Consume(ctx context.Context, scope hub.ComposerDraftScope, capturedRevision uint64) (bool, error) {
	if err := scope.Validate(); err != nil {
		return false, err
	}
	if capturedRevision == 0 {
		return false, nil
	}
	_, err := s.Save(ctx, hub.ComposerDraft{Scope: scope}, capturedRevision)
	if errors.Is(err, hub.ErrComposerDraftConflict) {
		return false, nil
	}
	return err == nil, err
}

func (s *ComposerDraftsService) CreateSession(ctx context.Context, captured hub.ComposerDraft, title, workspace string, target hub.Target) (hub.Session, hub.ComposerDraft, error) {
	if _, err := DecodeComposerDraftPreparations(captured); err != nil {
		return hub.Session{}, hub.ComposerDraft{}, err
	}
	if captured.Scope.Kind != "home" || captured.Revision == 0 || captured.Revision >= 1<<63-1 {
		return hub.Session{}, hub.ComposerDraft{}, errors.New("first Send requires a committed home draft")
	}
	admission, ok := s.store.(hub.ComposerDraftAdmissionStore)
	if !ok {
		return hub.Session{}, hub.ComposerDraft{}, errors.New("composer draft store does not provide atomic session admission")
	}
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.Session{}, hub.ComposerDraft{}, err
	}
	defer s.release()
	session, value, err := admission.CreateComposerSession(ctx, hub.CloneComposerDraft(captured), title, workspace, target)
	if err != nil {
		return hub.Session{}, hub.ComposerDraft{}, err
	}
	check := hub.CloneComposerDraft(value)
	check.Revision, check.PendingSessionID = captured.Revision, captured.PendingSessionID
	expected := captured.Revision + 1
	if captured.PendingSessionID != "" {
		expected = captured.Revision
	}
	if session.ID == "" || value.PendingSessionID != session.ID || captured.PendingSessionID != "" && captured.PendingSessionID != session.ID || value.Scope != captured.Scope || value.Revision != expected || !reflect.DeepEqual(check, captured) {
		return hub.Session{}, hub.ComposerDraft{}, errors.New("composer session admission returned an invalid receipt; reload before retrying")
	}
	if err = s.publish(value); err != nil {
		return hub.Session{}, hub.ComposerDraft{}, err
	}
	return session, hub.CloneComposerDraft(value), nil
}

func (s *HubService) prepareSessionWorkspace(workspace string) (string, error) {
	if workspace == "" {
		id, err := hub.NewID("workspace-")
		if err != nil {
			return "", err
		}
		workspace, err = private.EnsureDir(filepath.Join(s.DataDirectory, "workspaces", id))
		if err != nil {
			return "", err
		}
	}
	return verifyExistingCanonicalDirectory(workspace)
}

// verifyExistingCanonicalDirectory is the shared verify-only workspace gate for
// every first-Send path: it resolves symlinks and requires an existing
// directory, and it never creates one, so a developer conversation tab can
// never materialise a missing Git worktree directory.
func verifyExistingCanonicalDirectory(workspace string) (string, error) {
	canonical, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return "", errors.New("workspace must be an existing directory")
	}
	return canonical, nil
}

func (s *HubService) CreateComposerSession(ctx context.Context, captured hub.ComposerDraft, title, workspace string) (hub.Session, hub.ComposerDraft, error) {
	if s.ComposerDrafts == nil {
		return hub.Session{}, hub.ComposerDraft{}, errors.New("durable composer draft owner is unavailable")
	}
	if captured.PendingSessionID != "" && workspace == "" {
		detail, err := s.Store.Detail(ctx, captured.PendingSessionID)
		if err != nil {
			return hub.Session{}, hub.ComposerDraft{}, err
		}
		workspace = detail.Session.Workspace
	}
	canonical, err := s.prepareSessionWorkspace(workspace)
	if err != nil {
		return hub.Session{}, hub.ComposerDraft{}, err
	}
	return s.ComposerDrafts.CreateSession(ctx, captured, title, canonical, s.DefaultTarget)
}

func (s *ComposerDraftsService) publish(value hub.ComposerDraft) error {
	if _, err := DecodeComposerDraftPreparations(value); err != nil {
		return errors.New("composer draft store returned an invalid snapshot")
	}
	value = hub.CloneComposerDraft(value)
	s.cacheMu.Lock()
	defer s.cacheMu.Unlock()
	if current, ok := s.cache[value.Scope]; ok && (value.Revision < current.Revision || value.Revision == current.Revision && !reflect.DeepEqual(value, current)) {
		return errors.New("composer draft store returned a stale snapshot")
	}
	s.cache[value.Scope] = value
	return nil
}

func (s *ComposerDraftsService) acquire(ctx context.Context) (context.Context, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-s.stopped:
		return nil, storage.ErrClosed
	case <-s.access:
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		s.access <- struct{}{}
		return nil, storage.ErrClosed
	}
	if err := ctx.Err(); err != nil {
		s.access <- struct{}{}
		return nil, err
	}
	owned, cancel := context.WithCancel(ctx)
	s.cancel, s.done = cancel, make(chan struct{})
	return owned, nil
}

func (s *ComposerDraftsService) release() {
	s.mu.Lock()
	s.cancel()
	s.cancel = nil
	close(s.done)
	s.done = nil
	s.mu.Unlock()
	s.access <- struct{}{}
}

// Close cancels admitted I/O and waits for its real return. A timeout retains
// that owner; a late committed save is still published and never replayed.
func (s *ComposerDraftsService) Close(ctx context.Context) error {
	s.mu.Lock()
	if !s.closed {
		s.closed = true
		close(s.stopped)
	}
	cancel, done := s.cancel, s.done
	s.mu.Unlock()
	if cancel == nil {
		return nil
	}
	cancel()
	select {
	case <-done:
		return nil
	default:
	}
	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func EncodeComposerDraftPreparations(selections []ComposerPreparation) (json.RawMessage, error) {
	if len(selections) == 0 {
		return nil, nil
	}
	data, err := json.Marshal(selections)
	if err != nil {
		return nil, err
	}
	_, err = decodeSavedComposerPreparations(data)
	return data, err
}

// Decode validates the existing preparation DTO, including nested receipts and
// exact JSON fields. Dynamic Skill/Goal source revalidation still occurs on Send
// through the existing RevalidateComposerPreparation admission path.
func DecodeComposerDraftPreparations(draft hub.ComposerDraft) ([]ComposerPreparation, error) {
	if err := draft.Validate(); err != nil {
		return nil, err
	}
	commands, err := decodeSavedComposerPreparations(draft.PreparedCommands)
	if err != nil {
		return nil, err
	}
	for _, p := range commands {
		if p.SkillRef != nil && p.SkillRef.SessionID != "" && (draft.Scope.Kind != "session" || p.SkillRef.SessionID != draft.Scope.ID) || p.GoalRef != nil && p.GoalRef.SessionID != "" && (draft.Scope.Kind != "session" || p.GoalRef.SessionID != draft.Scope.ID) {
			return nil, errors.New("saved composer preparation belongs to another draft scope")
		}
	}
	return commands, nil
}

func decodeSavedComposerPreparations(data json.RawMessage) ([]ComposerPreparation, error) {
	if len(data) == 0 {
		return nil, nil
	}
	if len(data) > 32<<10 {
		return nil, errors.New("saved composer preparations exceed 32 KiB")
	}
	wrapper := append([]byte(`{"commands":`), data...)
	wrapper = append(wrapper, '}')
	var decoded struct {
		Commands []ComposerPreparation `json:"commands"`
	}
	if err := strictjson.ValidateObject(wrapper, &decoded); err != nil {
		return nil, err
	}
	if err := json.Unmarshal(wrapper, &decoded); err != nil {
		return nil, err
	}
	if len(decoded.Commands) > 8 || strings.TrimSpace(string(data)) == "null" {
		return nil, errors.New("saved composer preparations need at most 8 selections")
	}
	seen := make(map[string]bool, len(decoded.Commands))
	for _, prepared := range decoded.Commands {
		if seen[prepared.EntryID] || validateSavedComposerPreparation(prepared) != nil {
			return nil, errors.New("invalid or duplicate saved composer preparation")
		}
		seen[prepared.EntryID] = true
	}
	return decoded.Commands, nil
}

func validateSavedComposerPreparation(p ComposerPreparation) error {
	invalid := errors.New("invalid saved composer preparation")
	if !draftString(p.EntryID, 256, false) || !draftString(p.Origin, 256, false) || !draftString(p.Scope, 256, false) || !draftString(p.Text, 16<<10, false) || len(p.Arguments) > 1 {
		return invalid
	}
	for key, value := range p.Arguments {
		if key != "request" || !draftString(value, 8192, false) {
			return invalid
		}
	}
	switch p.Kind {
	case ComposerCommandEntry:
		current, err := (&HubService{}).PrepareComposerSelection(context.Background(), "", ComposerSelectionRequest{EntryID: p.EntryID, Arguments: p.Arguments})
		if err != nil || !reflect.DeepEqual(current, p) {
			return invalid
		}
	case ComposerSkillEntry:
		r := p.SkillRef
		if r == nil || p.GoalRef != nil || p.Intent != "" || p.Origin != "alva.project-skill" || p.Scope != "workspace" || !skill.ValidName(r.Name) || p.EntryID != "skill:"+r.Name || !draftString(r.SessionID, 256, true) || !draftString(r.BranchID, 256, true) || r.RelativePath != ManagedSkillDirectory+"/"+r.Name+"/SKILL.md" || !draftSHA(r.SHA256) || r.Policy != skill.Automatic {
			return invalid
		}
		if r.Workspace == "" {
			if r.SessionID == "" || r.BranchID == "" {
				return invalid
			}
		} else if !draftString(r.Workspace, 4096, false) || !filepath.IsAbs(r.Workspace) || filepath.Clean(r.Workspace) != r.Workspace || (r.SessionID == "") != (r.BranchID == "") {
			return invalid
		}
		expectedText := "请使用项目 Skill「" + r.Name + "」处理下一次请求。"
		if request := p.Arguments["request"]; request != "" {
			expectedText += "\n\n" + request
		}
		if p.Text != expectedText {
			return invalid
		}
	case ComposerGoalEntry:
		r := p.GoalRef
		if r == nil || p.SkillRef != nil || len(p.Arguments) != 0 || p.Intent != "" || p.Origin != "alva.goal" || p.Scope != string(r.Scope.Kind) || !draftString(r.GoalID, 128, false) || p.EntryID != "goal:"+r.GoalID || r.Revision != 1 || r.Scope.Validate() != nil || !draftString(r.SessionID, 256, true) || !draftString(r.BranchID, 256, true) {
			return invalid
		}
		if r.SessionID == "" {
			if r.Scope.Kind != hub.GoalProjectScope || r.BranchID != "" || r.Info != (storage.ArtifactInfo{}) || r.Reference != (wire.Reference{}) {
				return invalid
			}
		} else if r.BranchID == "" || r.Scope.Kind == hub.GoalSessionScope && r.Scope.SessionID != r.SessionID || !validSavedGoalArtifact(r) {
			return invalid
		}
	default:
		return invalid
	}
	return nil
}

func validSavedGoalArtifact(r *ComposerGoalReference) bool {
	i := r.Info
	return draftString(i.ID, 256, false) && draftString(i.SessionID, 256, false) && i.Kind == "goal-reference" && i.Name == r.GoalID+".json" && draftString(i.MediaType, 256, false) && draftString(i.Processing, 4096, true) && draftSHA(i.SHA256) && i.Bytes > 0 && i.Bytes <= 96<<10 && i.Text && r.Reference == i.Reference()
}

func draftString(value string, max int, empty bool) bool {
	return (empty || strings.TrimSpace(value) != "") && len(value) <= max && utf8.ValidString(value) && !strings.ContainsRune(value, 0)
}

func draftSHA(value string) bool {
	if len(value) != 64 || value != strings.ToLower(value) {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}
