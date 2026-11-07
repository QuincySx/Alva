// INPUT: Explicit companion storage, configured account routes and the existing Hub session/Skill/runtime owners.
// OUTPUT: Shared Profile/group CRUD, selected pinned Skills, isolated private/group sends and joined operation shutdown.
// POS: [L7 Hub product] Companion operations never create a second runtime or inherit private conversation authority into groups.
package product

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
)

type companionOperation struct {
	cancel   context.CancelFunc
	done     chan struct{}
	group    string
	identity string
	context  context.Context
}
type CompanionService struct {
	store      hub.CompanionStore
	mu         sync.Mutex
	closed     bool
	operations map[*companionOperation]bool
}

func NewCompanionService(store hub.CompanionStore) (*CompanionService, error) {
	if storage.Missing(store) {
		return nil, errors.New("companion storage required")
	}
	return &CompanionService{store: store, operations: make(map[*companionOperation]bool)}, nil
}
func (s *CompanionService) operation(ctx context.Context, groups ...string) (context.Context, func(), error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil, nil, storage.ErrClosed
	}
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	group := ""
	if len(groups) > 0 {
		group = groups[0]
		for op := range s.operations {
			if op.group == group {
				return nil, nil, hub.ErrSessionBusy
			}
		}
	}
	owned, cancel := context.WithCancel(ctx)
	identity, err := hub.NewID("companion-operation-")
	if err != nil {
		cancel()
		return nil, nil, err
	}
	op := &companionOperation{cancel: cancel, done: make(chan struct{}), group: group, identity: identity, context: owned}
	s.operations[op] = true
	return owned, func() { cancel(); s.mu.Lock(); delete(s.operations, op); close(op.done); s.mu.Unlock() }, nil
}
func (s *CompanionService) Close(ctx context.Context) error {
	s.mu.Lock()
	s.closed = true
	pending := make([]*companionOperation, 0, len(s.operations))
	for op := range s.operations {
		op.cancel()
		pending = append(pending, op)
	}
	s.mu.Unlock()
	for _, op := range pending {
		select {
		case <-op.done:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return nil
}
func (s *CompanionService) List(ctx context.Context, owner string) ([]hub.CompanionProfile, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return nil, err
	}
	defer done()
	out, err := s.store.Companions(ctx, owner)
	if err != nil {
		return nil, companionError(err)
	}
	seen := map[string]bool{}
	if len(out) > 256 {
		return nil, companionReceiptError()
	}
	for _, p := range out {
		if !validCompanion(p, owner, "") || seen[p.ID] || p.Deleted {
			return nil, companionReceiptError()
		}
		seen[p.ID] = true
	}
	for i := range out {
		out[i] = cloneCompanionProfile(out[i])
	}
	return append([]hub.CompanionProfile(nil), out...), nil
}
func (s *CompanionService) Read(ctx context.Context, owner, id string) (hub.CompanionProfile, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionProfile{}, err
	}
	defer done()
	return s.readProfile(ctx, owner, id)
}
func (s *CompanionService) Save(ctx context.Context, p hub.CompanionProfile, expected uint64) (hub.CompanionProfile, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionProfile{}, err
	}
	defer done()
	return s.saveProfile(ctx, p, expected)
}
func (s *CompanionService) Delete(ctx context.Context, owner, id string, expected uint64) error {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return err
	}
	defer done()
	if err = s.store.DeleteCompanion(ctx, owner, id, expected); err != nil {
		return companionError(err)
	}
	out, err := s.readProfile(ctx, owner, id)
	if err != nil {
		return err
	}
	if !out.Deleted || out.Enabled || out.Revision != expected+1 {
		return companionReceiptError()
	}
	return nil
}
func (s *CompanionService) Groups(ctx context.Context, owner string) ([]hub.CompanionGroup, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return nil, err
	}
	defer done()
	out, err := s.store.CompanionGroups(ctx, owner)
	if err != nil {
		return nil, companionError(err)
	}
	seen := map[string]bool{}
	if len(out) > 256 {
		return nil, companionReceiptError()
	}
	for _, g := range out {
		if !validCompanionGroup(g, owner, "") || seen[g.ID] || g.Deleted {
			return nil, companionReceiptError()
		}
		seen[g.ID] = true
	}
	for i := range out {
		out[i] = cloneCompanionGroup(out[i])
	}
	return append([]hub.CompanionGroup(nil), out...), nil
}
func (s *CompanionService) Group(ctx context.Context, owner, id string) (hub.CompanionGroup, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionGroup{}, err
	}
	defer done()
	return s.readGroup(ctx, owner, id)
}
func (s *CompanionService) SaveGroup(ctx context.Context, g hub.CompanionGroup, expected uint64) (hub.CompanionGroup, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionGroup{}, err
	}
	defer done()
	return s.saveGroup(ctx, g, expected)
}
func (s *CompanionService) DeleteGroup(ctx context.Context, owner, id string, expected uint64) error {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return err
	}
	defer done()
	if err = s.store.DeleteCompanionGroup(ctx, owner, id, expected); err != nil {
		return companionError(err)
	}
	out, err := s.readGroup(ctx, owner, id)
	if err != nil {
		return err
	}
	if !out.Deleted || out.Enabled || out.Revision != expected+1 {
		return companionReceiptError()
	}
	return nil
}
func (s *CompanionService) Messages(ctx context.Context, owner, id string, scope uint64, limit int) ([]hub.CompanionMessage, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return nil, err
	}
	defer done()
	return s.messages(ctx, owner, id, scope, limit)
}
func (s *CompanionService) Rounds(ctx context.Context, owner, id string, scope uint64, limit int) ([]hub.CompanionRound, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return nil, err
	}
	defer done()
	return s.rounds(ctx, owner, id, scope, limit)
}

func (s *HubService) companionOperation(ctx context.Context, group ...string) (context.Context, func(), error) {
	if s.Companions == nil {
		return nil, nil, errors.New("companion service unavailable")
	}
	return s.Companions.operation(ctx, group...)
}
func (s *HubService) validateCompanionRoute(target hub.Target, approved bool) error {
	if !approved {
		return errors.New("companion account/billing route requires explicit approval")
	}
	if s.ValidateTarget(target) == nil {
		return nil
	}
	return errors.New("companion route is not one of the actually configured engine routes")
}
func (s *HubService) SaveCompanionProfile(ctx context.Context, p hub.CompanionProfile, expected uint64) (hub.CompanionProfile, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return hub.CompanionProfile{}, err
	}
	defer done()
	if err = p.Validate(); err != nil {
		return hub.CompanionProfile{}, err
	}
	for _, ref := range p.Skills {
		if _, err = s.validateCompanionSkill(ctx, ref, p.Workspace); err != nil {
			return hub.CompanionProfile{}, err
		}
	}
	return s.Companions.saveProfile(ctx, p, expected)
}
func (s *HubService) SaveCompanionGroup(ctx context.Context, g hub.CompanionGroup, expected uint64) (hub.CompanionGroup, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return hub.CompanionGroup{}, err
	}
	defer done()
	if g.Workspace == "" {
		id := g.ID
		if id == "" {
			id, err = hub.NewID("group-workspace-")
			if err != nil {
				return hub.CompanionGroup{}, err
			}
		}
		g.Workspace, err = private.EnsureDir(filepath.Join(s.DataDirectory, "companion-groups", id))
		if err != nil {
			return hub.CompanionGroup{}, err
		}
	}
	canonical, err := filepath.EvalSymlinks(g.Workspace)
	if err != nil {
		return hub.CompanionGroup{}, err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return hub.CompanionGroup{}, errors.New("group workspace must be an existing directory")
	}
	g.Workspace = canonical
	return s.Companions.saveGroup(ctx, g, expected)
}

// PinCompanionSkill is an explicit cold selection from an already enabled
// project Skill. GroupAllowed is a separate sharing choice, never inferred.
func (s *HubService) PinCompanionSkill(ctx context.Context, sourceSession, name string, groupAllowed bool) (hub.CompanionSkill, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return hub.CompanionSkill{}, err
	}
	defer done()
	prepared, err := s.PrepareComposerSelection(ctx, sourceSession, ComposerSelectionRequest{EntryID: "skill:" + name})
	if err != nil {
		return hub.CompanionSkill{}, err
	}
	if prepared.SkillRef == nil {
		return hub.CompanionSkill{}, errors.New("actual project Skill required")
	}
	r := prepared.SkillRef
	return hub.CompanionSkill{SourceSessionID: r.SessionID, SourceBranchID: r.BranchID, Name: r.Name, SHA256: r.SHA256, GroupAllowed: groupAllowed}, nil
}
func (s *HubService) validateCompanionSkill(ctx context.Context, ref hub.CompanionSkill, workspace string) (ComposerPreparation, error) {
	detail, err := s.Store.Detail(ctx, ref.SourceSessionID)
	if err != nil {
		return ComposerPreparation{}, err
	}
	if workspace == "" || detail.Session.Workspace != workspace {
		return ComposerPreparation{}, errors.New("pinned Skill must belong to the explicitly selected companion/group workspace")
	}
	current, err := s.PrepareComposerSelection(ctx, ref.SourceSessionID, ComposerSelectionRequest{EntryID: "skill:" + ref.Name})
	if err != nil {
		return current, err
	}
	if current.SkillRef == nil || current.SkillRef.BranchID != ref.SourceBranchID || current.SkillRef.SHA256 != ref.SHA256 {
		return current, errors.New("pinned Skill source or version changed; select it again")
	}
	return current, nil
}
func (s *HubService) OpenCompanion(ctx context.Context, owner, id string, approved bool) (hub.CompanionConversation, error) {
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return hub.CompanionConversation{}, err
	}
	defer done()
	p, err := s.Companions.readProfile(ctx, owner, id)
	if err != nil {
		return hub.CompanionConversation{}, err
	}
	if err = s.validateCompanionRoute(p.Target, approved); err != nil {
		return hub.CompanionConversation{}, err
	}
	workspace := p.Workspace
	if workspace == "" {
		workspace, err = private.EnsureDir(filepath.Join(s.DataDirectory, "companions", p.ID))
		if err != nil {
			return hub.CompanionConversation{}, err
		}
	}
	canonical, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return hub.CompanionConversation{}, err
	}
	out, err := s.Companions.store.OpenCompanionConversation(ctx, owner, id, "", 0, canonical)
	if err != nil {
		return hub.CompanionConversation{}, companionError(err)
	}
	if out.ID == "" || out.SessionID == "" || out.OwnerID != owner || out.CompanionID != id || out.GroupID != "" || out.GroupRevision != 0 || !validCompanion(out.Profile, owner, id) || out.Profile.Revision != p.Revision {
		return hub.CompanionConversation{}, companionReceiptError()
	}
	actual, err := s.Store.Detail(ctx, out.SessionID)
	if err != nil {
		return hub.CompanionConversation{}, companionError(err)
	}
	if actual.Session.Workspace != canonical {
		return hub.CompanionConversation{}, companionReceiptError()
	}
	return out, nil
}
func (s *HubService) companionPromptLocked(ctx context.Context, c hub.CompanionConversation, prompt string) (string, error) {
	if strings.TrimSpace(prompt) == "" || len(prompt) > 64<<10 || !utf8.ValidString(prompt) {
		return "", errors.New("companion prompt requires 1..16 KiB UTF-8")
	}
	p := c.Profile
	if err := s.validateCompanionRoute(p.Target, true); err != nil {
		return "", err
	}
	detail, err := s.Store.Detail(ctx, c.SessionID)
	if err != nil {
		return "", err
	}
	if hub.BlocksInput(detail.Session.Status) || s.Runtime.SessionActive(c.SessionID) {
		return "", hub.ErrSessionBusy
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return "", err
	}
	if hub.RouteKey(branch.Target) != hub.RouteKey(p.Target) {
		branch, err = s.Store.Switch(ctx, c.SessionID, p.Target, "explicit companion Profile route")
		if err != nil {
			return "", err
		}
	}
	if p.Target.EngineID == "alva" {
		if err = s.validateNativePolicyChangeLocked(ctx, c.SessionID); err != nil {
			return "", err
		}
		if err = s.Store.SetPermissionPolicy(ctx, c.SessionID, p.PermissionPolicy, false); err != nil {
			return "", err
		}
	} else if p.PermissionPolicy.Effective() != hub.PermissionAsk || len(p.Skills) > 0 {
		return "", engine.ErrUnsupported
	}
	if c.GroupID != "" {
		if err = s.Store.SetPersistentMemory(ctx, c.SessionID, false); err != nil {
			return "", err
		}
	}
	applicable := 0
	for _, ref := range p.Skills {
		if c.GroupID == "" || ref.GroupAllowed {
			applicable++
		}
	}
	if err = s.Store.SetSkillsEnabled(ctx, c.SessionID, applicable > 0); err != nil {
		return "", err
	}
	scope := &hub.CompanionSkillScope{}
	for _, ref := range p.Skills {
		if c.GroupID == "" || ref.GroupAllowed {
			scope.Skills = append(scope.Skills, ref)
		}
	}
	if p.Target.EngineID == "alva" {
		if err = s.Store.SetCompanionSkills(ctx, c.SessionID, scope); err != nil {
			return "", err
		}
	}
	var selections []ComposerPreparation
	for _, ref := range p.Skills {
		if c.GroupID != "" && !ref.GroupAllowed {
			continue
		}
		if _, err = s.validateCompanionSkill(ctx, ref, detail.Session.Workspace); err != nil {
			return "", err
		}
		if err = s.Store.SetSkillsEnabled(ctx, c.SessionID, true); err != nil {
			return "", err
		}
		selected, err := s.PrepareComposerSelection(ctx, c.SessionID, ComposerSelectionRequest{EntryID: "skill:" + ref.Name})
		if err != nil {
			return "", err
		}
		if selected.SkillRef == nil || selected.SkillRef.SHA256 != ref.SHA256 {
			return "", errors.New("target Skill source differs from the frozen companion Profile")
		}
		selections = append(selections, selected)
	}
	if err = s.prepareComposerDraftLocked(ctx, c.SessionID, selections); err != nil {
		return "", err
	}
	for _, selected := range selections {
		prompt = selected.Text + "\n\n" + prompt
	}
	prompt = fmt.Sprintf("[Explicit companion Profile: %s; revision %d. This role text grants no tools, files or permissions.]\n%s\n\n%s", p.Name, p.Revision, p.Role, prompt)
	return s.draftPromptLocked(c.SessionID, branch, detail.Session.Workspace, prompt, nil)
}
func (s *HubService) SubmitCompanion(ctx context.Context, owner, id, prompt string, approved bool, approvedProfiles ...hub.CompanionProfile) (string, error) {
	if strings.TrimSpace(prompt) == "" || len(prompt) > 16<<10 || !utf8.ValidString(prompt) {
		return "", errors.New("companion prompt requires 1..16 KiB UTF-8")
	}
	ctx, done, err := s.companionOperation(ctx)
	if err != nil {
		return "", err
	}
	defer done()
	c, err := s.OpenCompanion(ctx, owner, id, approved)
	if err != nil {
		return "", err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	p, err := s.Companions.readProfile(ctx, owner, id)
	if err != nil {
		return "", err
	}
	if len(approvedProfiles) > 0 && !companionApprovalMatches(p, approvedProfiles) {
		return "", hub.ErrCompanionConflict
	}
	if p.Revision != c.Profile.Revision || !p.Enabled || p.Deleted {
		return "", hub.ErrCompanionConflict
	}
	prompt, err = s.companionPromptLocked(ctx, c, prompt)
	if err != nil {
		return "", err
	}
	detail, err := s.Store.Detail(ctx, c.SessionID)
	if err != nil {
		return "", err
	}
	if err = ctx.Err(); err != nil {
		return "", err
	}
	turn, _, err := s.Runtime.StartOnBranch(context.Background(), c.SessionID, detail.Session.ActiveBranchID, prompt)
	return turn, err
}

// RunCompanionGroup is a finite, explicitly requested round. The existing
// Runtime owns every physical execution. No member text can start another round.
// Cancellation waits for the current physical owner before the operation ends.
func (s *HubService) RunCompanionGroup(ctx context.Context, owner, id string, expected uint64, prompt string, members []string, approved bool, approvedProfiles ...hub.CompanionProfile) (out hub.CompanionRound, result error) {
	ctx, done, err := s.companionOperation(ctx, owner+"\x00"+id)
	if err != nil {
		return out, err
	}
	defer done()
	g, err := s.Companions.readGroup(ctx, owner, id)
	if err != nil {
		return out, err
	}
	if g.Revision != expected {
		return out, hub.ErrCompanionConflict
	}
	expectedProfiles := map[string]uint64{}
	for _, member := range g.Members {
		p, e := s.Companions.readProfile(ctx, owner, member)
		if e != nil {
			return out, e
		}
		selected := g.ReplyMode == "all"
		for _, choice := range members {
			selected = selected || choice == member
		}
		if selected {
			if len(approvedProfiles) > 0 && !companionApprovalMatches(p, approvedProfiles) {
				return out, hub.ErrCompanionConflict
			}
			expectedProfiles[member] = p.Revision
			if e = s.validateCompanionRoute(p.Target, approved); e != nil {
				return out, e
			}
			for _, ref := range p.Skills {
				if ref.GroupAllowed {
					if _, e = s.validateCompanionSkill(ctx, ref, g.Workspace); e != nil {
						return out, e
					}
				}
			}
		}
	}
	out, err = s.Companions.store.BeginCompanionRound(ctx, owner, id, expected, prompt, members)
	if err != nil {
		return out, companionError(err)
	}
	if out.ID == "" || out.OwnerID != owner || out.GroupID != id || out.GroupRevision != g.ScopeRevision || out.Status != "running" || len(out.Members) != len(expectedProfiles) {
		return hub.CompanionRound{}, companionReceiptError()
	}
	seenMembers := map[string]bool{}
	seenSessions := map[string]bool{}
	for _, c := range out.Members {
		if c.ID == "" || c.SessionID == "" || seenMembers[c.CompanionID] || seenSessions[c.SessionID] || c.OwnerID != owner || c.GroupID != id || c.GroupRevision != g.ScopeRevision || !validCompanion(c.Profile, owner, c.CompanionID) || expectedProfiles[c.CompanionID] != c.Profile.Revision {
			return hub.CompanionRound{}, companionReceiptError()
		}
		seenMembers[c.CompanionID] = true
		seenSessions[c.SessionID] = true
	}
	out.OperationID = s.Companions.operationIdentity(ctx)
	out, err = s.saveCompanionProgress(ctx, out)
	if err != nil {
		return out, err
	}
	return s.executeCompanionRound(ctx, out, nil)
}

func (s *HubService) companionTurnText(ctx context.Context, sid, turn string) (string, error) {
	var before uint64
	var found []hub.HistoryEntry
	seenTarget := false
	for page := 0; page < 32; page++ {
		entries, err := s.Store.HistoryBefore(ctx, sid, before, 128)
		if err != nil {
			return "", err
		}
		if len(entries) == 0 {
			break
		}
		before = entries[0].Sequence
		found = append(entries, found...)
		minTarget := uint64(0)
		for _, entry := range entries {
			if entry.TurnID == turn {
				seenTarget = true
				if minTarget == 0 || entry.Sequence < minTarget {
					minTarget = entry.Sequence
				}
			}
		}
		older := false
		if seenTarget {
			for _, entry := range entries {
				if entry.TurnID != "" && entry.TurnID != turn && (minTarget == 0 || entry.Sequence < minTarget) {
					older = true
				}
			}
		}
		if len(entries) < 128 || older {
			break
		}
		if page == 31 {
			return "", errors.New("group member original Turn exceeds bounded retrieval budget")
		}
	}
	if !seenTarget {
		return "", errors.New("original group member Turn was not found in bounded history; no reply was published")
	}
	order := []string{}
	messages := map[string]string{}
	for _, entry := range found {
		if entry.TurnID != turn || (entry.Event.Kind != engine.MessageDelta && entry.Event.Kind != engine.Message) {
			continue
		}
		key := entry.AttemptID + "/" + entry.Event.ID
		if _, ok := messages[key]; !ok {
			order = append(order, key)
		}
		if entry.Event.Kind == engine.Message {
			messages[key] = entry.Event.Text
		} else {
			messages[key] += entry.Event.Text
		}
	}
	var text strings.Builder
	for _, key := range order {
		if text.Len()+len(messages[key]) > 32<<10 {
			return "", errors.New("group reply exceeds bounded message budget; original member history remains available")
		}
		text.WriteString(messages[key])
	}
	return text.String(), nil
}

func (s *HubService) StopCompanionGroup(ctx context.Context, owner, groupID, roundID string, expected uint64, operationID string) error {
	owned, done, err := s.companionOperation(ctx)
	if err != nil {
		return err
	}
	defer done()
	r, err := s.Companions.round(owned, owner, roundID)
	if err != nil {
		return err
	}
	if r.GroupID != groupID || r.Revision != expected || operationID == "" || r.OperationID != operationID {
		return hub.ErrCompanionConflict
	}
	s.Companions.mu.Lock()
	var runs []*companionOperation
	for op := range s.Companions.operations {
		if op.group == owner+"\x00"+groupID && op.identity == operationID {
			op.cancel()
			runs = append(runs, op)
		}
	}
	s.Companions.mu.Unlock()
	if len(runs) == 0 {
		if r.NextMember == len(r.Members) && r.Stage == "ready" {
			_, err = s.finishCompanionRound(owned, r, "completed", nil)
			return err
		}
		return hub.ErrSessionNotRunning
	}
	for _, op := range runs {
		select {
		case <-op.done:
		case <-owned.Done():
			return owned.Err()
		}
	}
	return nil
}
func (s *CompanionService) operationIdentity(ctx context.Context) string {
	s.mu.Lock()
	defer s.mu.Unlock()
	for op := range s.operations {
		if op.context == ctx {
			return op.identity
		}
	}
	return ""
}

// Conversation is a cold lookup, including disabled/tombstoned identities. It
// never creates a session, changes an account route or starts an execution.
func (s *CompanionService) Conversation(ctx context.Context, owner, id, group string, scope uint64) (hub.CompanionConversation, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionConversation{}, err
	}
	defer done()
	if group != "" && scope == 0 {
		g, err := s.readGroup(ctx, owner, group)
		if err != nil {
			return hub.CompanionConversation{}, err
		}
		scope = g.ScopeRevision
	}
	out, err := s.store.CompanionConversation(ctx, owner, id, group, scope)
	if err != nil {
		return hub.CompanionConversation{}, companionError(err)
	}
	if out.ID == "" || out.SessionID == "" || out.OwnerID != owner || out.CompanionID != id || out.GroupID != group || out.GroupRevision != scope || !validCompanion(out.Profile, owner, id) {
		return hub.CompanionConversation{}, companionReceiptError()
	}
	return out, nil
}

func companionApprovalMatches(current hub.CompanionProfile, approved []hub.CompanionProfile) bool {
	for _, snapshot := range approved {
		if snapshot.ID == current.ID && snapshot.OwnerID == current.OwnerID && snapshot.Revision == current.Revision && hub.RouteKey(snapshot.Target) == hub.RouteKey(current.Target) {
			return true
		}
	}
	return false
}

func companionLiveRound(status string) bool {
	return status == "running" || status == "waiting" || status == "interrupted" || status == "unknown"
}

// Session is a tracked cold binding lookup for shared admission consumers.
// A missing binding denotes an ordinary session; other failures stay explicit.
func (s *CompanionService) Session(ctx context.Context, id string) (hub.CompanionConversation, error) {
	ctx, done, err := s.operation(ctx)
	if err != nil {
		return hub.CompanionConversation{}, err
	}
	defer done()
	out, err := s.store.CompanionSession(ctx, id)
	if err != nil {
		return hub.CompanionConversation{}, companionError(err)
	}
	if out.ID == "" || out.SessionID != id || out.OwnerID == "" || out.CompanionID == "" || !validCompanion(out.Profile, out.OwnerID, out.CompanionID) {
		return hub.CompanionConversation{}, companionReceiptError()
	}
	out.Profile = cloneCompanionProfile(out.Profile)
	return out, nil
}
