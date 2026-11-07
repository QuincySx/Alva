// INPUT: Explicit owner identity, engine targets, product permissions and durable session identities.
// OUTPUT: Companion/Profile/group snapshots, independent mutation storage and optional read-only history pages.
// POS: [L7 Hub product] Stable companions and isolated group participation remain distinct from temporary Teams and execution owners.
package hub

import (
	"context"
	"encoding/hex"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

const LocalCompanionOwner = "local"

var ErrCompanionConflict = errors.New("companion configuration changed; reload before saving")
var ErrCompanionDisabled = errors.New("companion or group is disabled")

// CompanionProfile is configuration only. Saving it grants no account, tool,
// filesystem or billing access. Actual capabilities belong to its Hub session.
type CompanionProfile struct {
	ID               string           `json:"id"`
	OwnerID          string           `json:"owner_id"`
	Revision         uint64           `json:"revision"`
	Name             string           `json:"name"`
	Role             string           `json:"role"`
	Target           Target           `json:"target"`
	PermissionPolicy PermissionPolicy `json:"permission_policy"`
	Workspace        string           `json:"workspace,omitempty"`
	Skills           []CompanionSkill `json:"skills,omitempty"`
	Enabled          bool             `json:"enabled"`
	Deleted          bool             `json:"deleted,omitempty"`
	CreatedAt        int64            `json:"created_at"`
	UpdatedAt        int64            `json:"updated_at"`
}

func (p CompanionProfile) Validate() error {
	if p.ID != "" && !companionText(p.ID, 128) || p.Deleted && p.Enabled {
		return errors.New("invalid companion identity/state")
	}
	if !companionText(p.OwnerID, 128) || !companionText(p.Name, 128) || len(p.Role) > 8192 || !utf8.ValidString(p.Role) || strings.ContainsRune(p.Role, 0) {
		return errors.New("bounded companion owner, name and role required")
	}
	if p.Workspace != "" && (!filepath.IsAbs(p.Workspace) || filepath.Clean(p.Workspace) != p.Workspace) {
		return errors.New("companion workspace must be an explicit absolute path")
	}
	if len(p.Skills) > 8 {
		return errors.New("at most 8 pinned companion Skills")
	}
	seen := map[string]bool{}
	for _, s := range p.Skills {
		if !companionText(s.SourceSessionID, 128) || !companionText(s.SourceBranchID, 128) || !companionText(s.Name, 128) || len(s.SHA256) != 64 || seen[s.Name] {
			return errors.New("invalid or duplicate pinned companion Skill")
		}
		if _, err := hex.DecodeString(s.SHA256); err != nil {
			return errors.New("Skill SHA256 must be hexadecimal")
		}
		seen[s.Name] = true
	}
	if err := p.Target.Validate(); err != nil {
		return err
	}
	if err := p.PermissionPolicy.Validate(); err != nil {
		return err
	}
	if p.PermissionPolicy.Effective() == PermissionFull {
		return errors.New("companion profiles cannot grant Full access; approve it separately on the actual session")
	}
	return nil
}
func companionText(s string, n int) bool {
	return strings.TrimSpace(s) != "" && len(s) <= n && utf8.ValidString(s) && !strings.ContainsAny(s, "\x00\r\n")
}

type CompanionSkill struct {
	SourceSessionID string `json:"source_session_id"`
	SourceBranchID  string `json:"source_branch_id"`
	Name            string `json:"name"`
	SHA256          string `json:"sha256"`
	GroupAllowed    bool   `json:"group_allowed"`
}

type CompanionGroup struct {
	ID            string   `json:"id"`
	OwnerID       string   `json:"owner_id"`
	Revision      uint64   `json:"revision"`
	Name          string   `json:"name"`
	Members       []string `json:"members"`
	ScopeRevision uint64   `json:"scope_revision"`
	ReplyMode     string   `json:"reply_mode"`
	ReplyLimit    int      `json:"reply_limit"`
	Workspace     string   `json:"workspace"`
	Enabled       bool     `json:"enabled"`
	Deleted       bool     `json:"deleted,omitempty"`
	CreatedAt     int64    `json:"created_at"`
	UpdatedAt     int64    `json:"updated_at"`
}

func (g CompanionGroup) Validate() error {
	if g.ID != "" && !companionText(g.ID, 128) || g.Deleted && g.Enabled {
		return errors.New("invalid group identity/state")
	}
	if !companionText(g.OwnerID, 128) || !companionText(g.Name, 128) || len(g.Members) < 1 || len(g.Members) > 16 || g.ReplyLimit < 1 || g.ReplyLimit > 16 {
		return errors.New("group requires owner, name, 1..16 members and 1..16 replies")
	}
	if g.ReplyMode != "mentions" && g.ReplyMode != "all" {
		return errors.New("group reply mode must be mentions or all")
	}
	if g.Workspace != "" && (!filepath.IsAbs(g.Workspace) || filepath.Clean(g.Workspace) != g.Workspace) {
		return errors.New("group workspace must be an explicit absolute path")
	}
	seen := map[string]bool{}
	for _, id := range g.Members {
		if !companionText(id, 128) || seen[id] {
			return errors.New("invalid or duplicate group member")
		}
		seen[id] = true
	}
	return nil
}

type CompanionConversation struct {
	ID            string           `json:"id"`
	OwnerID       string           `json:"owner_id"`
	CompanionID   string           `json:"companion_id"`
	GroupID       string           `json:"group_id,omitempty"`
	GroupRevision uint64           `json:"group_revision,omitempty"`
	SessionID     string           `json:"session_id"`
	Profile       CompanionProfile `json:"profile"`
	CreatedAt     int64            `json:"created_at"`
}

// Each round pins the original member order and profile revisions. Messages
// from another revision/private conversation are never implicitly inherited.
type CompanionRound struct {
	OperationID         string                  `json:"operation_id"`
	Revision            uint64                  `json:"revision"`
	NextMember          int                     `json:"next_member"`
	Stage               string                  `json:"stage"`
	ActiveBranchID      string                  `json:"active_branch_id,omitempty"`
	ActiveTurnID        string                  `json:"active_turn_id,omitempty"`
	BeforeTurnID        string                  `json:"before_turn_id,omitempty"`
	PendingPromptSHA256 string                  `json:"pending_prompt_sha256,omitempty"`
	ID                  string                  `json:"id"`
	OwnerID             string                  `json:"owner_id"`
	GroupID             string                  `json:"group_id"`
	GroupRevision       uint64                  `json:"group_revision"`
	Prompt              string                  `json:"prompt"`
	Members             []CompanionConversation `json:"members"`
	Status              string                  `json:"status"`
	Error               string                  `json:"error,omitempty"`
	CreatedAt           int64                   `json:"created_at"`
	EndedAt             int64                   `json:"ended_at"`
}
type CompanionMessage struct {
	ID            string `json:"id"`
	RoundID       string `json:"round_id"`
	GroupID       string `json:"group_id"`
	GroupRevision uint64 `json:"group_revision"`
	CompanionID   string `json:"companion_id,omitempty"`
	Author        string `json:"author"`
	Text          string `json:"text"`
	SessionID     string `json:"session_id,omitempty"`
	TurnID        string `json:"turn_id,omitempty"`
	Status        string `json:"status"`
	CreatedAt     int64  `json:"created_at"`
}

type CompanionStore interface {
	Companions(context.Context, string) ([]CompanionProfile, error)
	Companion(context.Context, string, string) (CompanionProfile, error)
	SaveCompanion(context.Context, CompanionProfile, uint64) (CompanionProfile, error)
	DeleteCompanion(context.Context, string, string, uint64) error
	CompanionGroups(context.Context, string) ([]CompanionGroup, error)
	CompanionGroup(context.Context, string, string) (CompanionGroup, error)
	SaveCompanionGroup(context.Context, CompanionGroup, uint64) (CompanionGroup, error)
	DeleteCompanionGroup(context.Context, string, string, uint64) error
	CompanionSession(context.Context, string) (CompanionConversation, error)
	CompanionConversation(context.Context, string, string, string, uint64) (CompanionConversation, error)
	OpenCompanionConversation(context.Context, string, string, string, uint64, string) (CompanionConversation, error)
	BeginCompanionRound(context.Context, string, string, uint64, string, []string) (CompanionRound, error)
	CompanionRound(context.Context, string, string) (CompanionRound, error)
	SaveCompanionRoundProgress(context.Context, string, CompanionRound, uint64) (CompanionRound, error)
	PublishCompanionReply(context.Context, string, string, uint64, CompanionMessage) (CompanionRound, error)
	FinishCompanionRound(context.Context, string, string, string, string) error
	AppendCompanionMessage(context.Context, string, CompanionMessage) error
	CompanionMessages(context.Context, string, string, uint64, int) ([]CompanionMessage, error)
	CompanionRounds(context.Context, string, string, uint64, int) ([]CompanionRound, error)
	RecoverCompanions(context.Context) error
}

// CompanionArchive exposes deleted identities for history discovery only.
// These snapshots never restore membership, account approval or execution.
type CompanionArchive struct {
	Profiles []CompanionProfile `json:"profiles"`
	Groups   []CompanionGroup   `json:"groups"`
}

// CompanionHistoryQuery carries an opaque, owner/group/scope-bound keyset.
// An empty Before starts at the newest facts; each page can load older facts.
type CompanionHistoryQuery struct {
	Before string `json:"before,omitempty"`
	Limit  int    `json:"limit"`
}
type CompanionMessagePage struct {
	Messages   []CompanionMessage `json:"messages"`
	NextBefore string             `json:"next_before,omitempty"`
	HasMore    bool               `json:"has_more"`
}
type CompanionRoundPage struct {
	Rounds     []CompanionRound `json:"rounds"`
	NextBefore string           `json:"next_before,omitempty"`
	HasMore    bool             `json:"has_more"`
}

// CompanionHistoryStore is optional browsing capability, separate from both
// CompanionStore mutation and the six-method ExecutionStore.
type CompanionHistoryStore interface {
	CompanionArchive(context.Context, string) (CompanionArchive, error)
	CompanionMessagePage(context.Context, string, string, uint64, CompanionHistoryQuery) (CompanionMessagePage, error)
	CompanionRoundPage(context.Context, string, string, uint64, CompanionHistoryQuery) (CompanionRoundPage, error)
}
