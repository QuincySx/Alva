// INPUT: Explicit imported repositories, approved Git worktree facts and bounded conversation-tab metadata.
// OUTPUT: Repository/tab DTOs, validation, durable ports and atomic first-Send admission contract.
// POS: [L7 Hub product] Developer navigation and view metadata stay separate from execution authority and resource tabs.
package hub

import (
	"context"
	"encoding/hex"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

const (
	// MaxConversationTabs is the budget of open (unarchived) tabs in one
	// worktree. Closing a tab archives it and releases its slot.
	MaxConversationTabs = 1024
	// MaxArchivedConversationTabs bounds the closed tabs kept for history. Tabs
	// are never deleted, so this is metadata storage protection rather than a
	// product limit; archived tabs are never rendered eagerly.
	MaxArchivedConversationTabs = 4096
	// ConversationMaxTabID keeps ConversationTabDraftID within the existing
	// ComposerDraftScope.ID budget.
	ConversationMaxTabID = 251
)

var (
	ErrDeveloperRepositoryConflict = errors.New("developer repository identity changed; refresh explicit imports")
	ErrConversationTabsConflict    = errors.New("conversation tabs changed; reload the worktree before saving")
)

// DeveloperRepository is an explicit user import, never a recent-directory or
// filesystem discovery result. Path is the canonical main worktree root and
// CommonDir is the canonical Git common directory used for identity deduplication.
type DeveloperRepository struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Path      string `json:"path"`
	CommonDir string `json:"common_dir"`
	CreatedAt int64  `json:"created_at"`
}

func (r DeveloperRepository) Validate() error {
	if !boundedDeveloperText(r.ID, 256, false) || !boundedDeveloperText(r.Name, 256, false) ||
		!canonicalAbsolutePath(r.Path) || !canonicalAbsolutePath(r.CommonDir) ||
		r.CreatedAt <= 0 {
		return errors.New("developer repository needs explicit bounded canonical identity")
	}
	return nil
}

// DeveloperRepositoryIdentity is produced by the approved Git owner before an
// import is persisted. It contains no credentials or ambient configuration.
type DeveloperRepositoryIdentity struct {
	Path      string `json:"path"`
	CommonDir string `json:"common_dir"`
}

func (i DeveloperRepositoryIdentity) Validate() error {
	if !canonicalAbsolutePath(i.Path) || !canonicalAbsolutePath(i.CommonDir) {
		return errors.New("developer repository identity requires canonical absolute paths")
	}
	return nil
}

// DeveloperWorktree is a live read-only Git fact. It is not a Worker binding and
// grants no authority to create, remove, reset or execute in that worktree.
type DeveloperWorktree struct {
	RepositoryID string `json:"repository_id"`
	Path         string `json:"path"`
	Name         string `json:"name"`
	Branch       string `json:"branch,omitempty"`
	Head         string `json:"head"`
	Main         bool   `json:"main"`
	Detached     bool   `json:"detached"`
}

func (w DeveloperWorktree) Validate(repositoryID string) error {
	if w.RepositoryID != repositoryID || !boundedDeveloperText(repositoryID, 256, false) ||
		!canonicalAbsolutePath(w.Path) || !boundedDeveloperText(w.Name, 256, false) ||
		!validDeveloperOID(w.Head) || !boundedDeveloperText(w.Branch, 256, true) ||
		w.Branch == "" && !w.Detached || w.Branch != "" && w.Detached {
		return errors.New("developer worktree needs bounded live Git identity")
	}
	return nil
}

// DeveloperRepositoryStore persists only explicit imports. Repeated imports
// must resolve to the same canonical path and Git common-directory identity.
type DeveloperRepositoryStore interface {
	ListDeveloperRepositories(context.Context) ([]DeveloperRepository, error)
	ImportDeveloperRepository(context.Context, DeveloperRepository) (DeveloperRepository, error)
}

// DeveloperWorktreeSource is implemented only by an approved Git owner. It
// performs live read-only repository inspection and worktree enumeration.
type DeveloperWorktreeSource interface {
	InspectDeveloperRepository(context.Context, string) (DeveloperRepositoryIdentity, error)
	ListDeveloperWorktrees(context.Context, string, string) ([]DeveloperWorktree, error)
}

// WorktreeScope is the storage key for one live worktree's conversation tabs.
// The path is canonical and the repository identity prevents path-only aliasing.
type WorktreeScope struct {
	RepositoryID string `json:"repository_id"`
	Path         string `json:"path"`
}

func (s WorktreeScope) Validate() error {
	if !boundedDeveloperText(s.RepositoryID, 256, false) || !canonicalAbsolutePath(s.Path) {
		return errors.New("worktree scope requires a repository identity and canonical absolute path")
	}
	return nil
}

// ConversationTab is a conversation level, not a right-side resource tab.
// Its ID and derived DraftID are globally owned across all persisted worktree
// scopes, including archived tabs. SessionID is set only after an explicit
// first Send created the HubSession.
type ConversationTab struct {
	ID         string `json:"id"`
	SessionID  string `json:"session_id,omitempty"`
	DraftID    string `json:"draft_id,omitempty"`
	Title      string `json:"title"`
	CreatedAt  int64  `json:"created_at"`
	SelectedAt int64  `json:"selected_at,omitempty"`
	Archived   bool   `json:"archived,omitempty"`
}

// ConversationTabDraftID derives the stable home draft identity for a new tab.
// The UI must use this helper rather than inventing a per-view scope.
func ConversationTabDraftID(tabID string) string {
	return "tab:" + tabID
}

func (t ConversationTab) Validate() error {
	if !boundedDeveloperText(t.ID, ConversationMaxTabID, false) ||
		!boundedDeveloperText(t.SessionID, 256, true) ||
		!boundedDeveloperText(t.DraftID, 256, true) ||
		!boundedDeveloperText(t.Title, 512, false) ||
		t.CreatedAt <= 0 || t.SelectedAt < 0 {
		return errors.New("conversation tab exceeds its bounded identity, title or timestamp budget")
	}
	if t.SessionID == "" && t.DraftID == "" {
		return errors.New("an unopened conversation tab needs a durable draft identity")
	}
	if t.DraftID != "" && t.DraftID != ConversationTabDraftID(t.ID) {
		return errors.New("conversation tab draft identity must derive from its stable tab ID")
	}
	return nil
}

// ConversationTabSet stores order exactly as Tabs order. Persisted tabs cannot
// be deleted to release their home draft to another scope; close uses Archived.
// ActiveTabID and SelectedAt are explicit user-selection facts; session UpdatedAt
// never changes them.
type ConversationTabSet struct {
	Scope       WorktreeScope     `json:"scope"`
	Revision    uint64            `json:"revision"`
	Tabs        []ConversationTab `json:"tabs"`
	ActiveTabID string            `json:"active_tab_id,omitempty"`
}

func (s ConversationTabSet) Validate() error {
	if err := s.Scope.Validate(); err != nil {
		return err
	}
	if s.Revision >= 1<<63 || len(s.Tabs) > MaxConversationTabs+MaxArchivedConversationTabs ||
		!boundedDeveloperText(s.ActiveTabID, ConversationMaxTabID, true) {
		return errors.New("conversation tab set exceeds its revision, count or active-tab budget")
	}
	ids, sessions, drafts := make(map[string]bool, len(s.Tabs)), make(map[string]bool, len(s.Tabs)), make(map[string]bool, len(s.Tabs))
	active, unarchived := false, false
	open := 0
	for _, tab := range s.Tabs {
		if err := tab.Validate(); err != nil || ids[tab.ID] {
			return errors.New("conversation tab set contains an invalid or duplicate tab")
		}
		ids[tab.ID] = true
		if tab.SessionID != "" {
			if sessions[tab.SessionID] {
				return errors.New("one HubSession cannot be bound to multiple conversation tabs")
			}
			sessions[tab.SessionID] = true
		}
		if tab.DraftID != "" {
			if drafts[tab.DraftID] {
				return errors.New("one durable draft cannot be bound to multiple conversation tabs")
			}
			drafts[tab.DraftID] = true
		}
		if !tab.Archived {
			unarchived = true
			open++
			active = active || tab.ID == s.ActiveTabID
		}
	}
	if open > MaxConversationTabs || len(s.Tabs)-open > MaxArchivedConversationTabs {
		return errors.New("conversation tab set exceeds its open or archived tab budget")
	}
	if s.ActiveTabID != "" && !active || s.ActiveTabID == "" && unarchived {
		return errors.New("active conversation tab must be one unarchived tab")
	}
	if active {
		for _, tab := range s.Tabs {
			if tab.ID == s.ActiveTabID && tab.SelectedAt <= 0 {
				return errors.New("active conversation tab needs an explicit user selection time")
			}
		}
	}
	return nil
}

func CloneConversationTabSet(value ConversationTabSet) ConversationTabSet {
	value.Tabs = append([]ConversationTab(nil), value.Tabs...)
	return value
}

type ConversationTabStore interface {
	ConversationTabs(context.Context, WorktreeScope) (ConversationTabSet, error)
	SaveConversationTabs(context.Context, ConversationTabSet, uint64) (ConversationTabSet, error)
}

// ConversationTabAdmission is an immutable first-Send receipt. It binds exactly
// one tab, one home draft and one HubSession; it does not admit or start a Turn.
type ConversationTabAdmission struct {
	Session Session            `json:"session"`
	Draft   ComposerDraft      `json:"draft"`
	Tabs    ConversationTabSet `json:"tabs"`
}

// ConversationTabAdmissionStore performs the tab, draft and session identity
// binding in one storage transaction. A retry returns that same admitted session.
type ConversationTabAdmissionStore interface {
	AdmitConversationTab(context.Context, ComposerDraft, string, WorktreeScope, string, uint64, Target) (ConversationTabAdmission, error)
}

func boundedDeveloperText(value string, max int, empty bool) bool {
	return (empty || strings.TrimSpace(value) != "") && len(value) <= max &&
		utf8.ValidString(value) && !strings.ContainsRune(value, 0)
}

func canonicalAbsolutePath(value string) bool {
	return value != "" && len(value) <= 4096 && utf8.ValidString(value) &&
		!strings.ContainsRune(value, 0) && filepath.IsAbs(value) && filepath.Clean(value) == value
}

func validDeveloperOID(value string) bool {
	if len(value) != 40 && len(value) != 64 || value != strings.ToLower(value) {
		return false
	}
	_, err := hex.DecodeString(value)
	return err == nil
}
