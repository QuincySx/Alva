// INPUT: context, crypto/rand, encoding/json, engine
// OUTPUT: Product sessions, branches, turns, attempts, handoffs and durable Store contract
// POS: [L7 Hub product] Hub product facts independent of native execution records and UI frameworks
package hub

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sort"
	"strings"
	"unicode"

	"github.com/QuincySx/Alva/integration/engine"
)

type Target struct {
	EngineID             string   `json:"engine_id"`
	ModelID              string   `json:"model_id"`
	ProviderID           string   `json:"provider_id"`
	AccountSource        string   `json:"account_source"`
	BillingRoute         string   `json:"billing_route"`
	AutomaticAuthorized  bool     `json:"automatic_authorized"`
	RequiredCapabilities []string `json:"required_capabilities,omitempty"`
}

func (t Target) Validate() error {
	for _, v := range []string{t.EngineID, t.ModelID, t.ProviderID, t.AccountSource, t.BillingRoute} {
		if strings.TrimSpace(v) == "" || len(v) > 256 {
			return errors.New("engine target requires bounded engine/model/provider/account/billing identities")
		}
	}
	if len(t.RequiredCapabilities) > 16 {
		return errors.New("too many required capabilities")
	}
	return nil
}

// ConnectionAccount reports the stable model-connection ID of a target saved
// by this or an older build. Older builds appended "@<revision>"; that suffix
// is a cosmetic edit counter, never routing identity.
func ConnectionAccount(target Target) (string, bool) {
	if target.EngineID != "alva" {
		return "", false
	}
	return ConnectionAccountID(target.AccountSource)
}

// ConnectionAccountID is ConnectionAccount for an account source alone, for
// presentation that groups every Agent's routes through one connection.
func ConnectionAccountID(accountSource string) (string, bool) {
	if !strings.HasPrefix(accountSource, "connection:") {
		return "", false
	}
	id := strings.TrimPrefix(accountSource, "connection:")
	if i := strings.LastIndexByte(id, '@'); i > 0 {
		digits := id[i+1:]
		if digits != "" && strings.Trim(digits, "0123456789") == "" {
			id = id[:i]
		}
	}
	return id, id != ""
}

// RouteKey is the routing identity. A model connection route is exactly
// (engine, connection ID, model ID): renaming, re-keying, adding headers or
// moving the endpoint keeps the same route and is resolved at execution time.
func RouteKey(target Target) string {
	if id, ok := ConnectionAccount(target); ok {
		data, _ := json.Marshal([]string{"connection", target.EngineID, id, target.ModelID})
		return string(data)
	}
	target.AutomaticAuthorized = false
	target.RequiredCapabilities = append([]string(nil), target.RequiredCapabilities...)
	sort.Strings(target.RequiredCapabilities)
	data, _ := json.Marshal(target)
	return string(data)
}

// RouteFacts is what a route resolves to right now.
type RouteFacts struct {
	Target   Target // the same route with current protocol/endpoint facts
	Revision uint64 // connection revision actually used (0 for static routes)
	Endpoint string
}

// RouteSeen is the last resolved connection facts of one route on a branch.
type RouteSeen struct {
	Revision uint64 `json:"revision"`
	Endpoint string `json:"endpoint"`
}

// RouteResolver revalidates a saved route against its current owner. An error
// means the route is no longer usable (deleted, disabled, protocol changed).
type RouteResolver interface {
	ResolveRoute(context.Context, Target) (RouteFacts, error)
}

// RouteRecorder is an optional ExecutionStore capability: audit the connection
// facts an attempt really used and surface a moved endpoint.
type RouteRecorder interface {
	RecordRoute(ctx context.Context, attemptID string, facts RouteFacts) error
}

// FallbackNotifier is an optional ExecutionStore capability: make a skipped or
// failed fallback visible in the message stream.
type FallbackNotifier interface {
	RecordFallbackSkip(ctx context.Context, attemptID string, target Target, reason string) error
}

// TurnRecoverer is an optional ExecutionStore capability: interrupt one
// session's running Turn whose execution owner is gone.
type TurnRecoverer interface {
	RecoverSession(ctx context.Context, sessionID string) error
}

type Session struct {
	ID             string         `json:"id"`
	Title          string         `json:"title"`
	Workspace      string         `json:"workspace"`
	ActiveBranchID string         `json:"active_branch_id"`
	Status         engine.Outcome `json:"status"`
	CreatedAt      int64          `json:"created_at"`
	UpdatedAt      int64          `json:"updated_at"`
	// Pinned keeps the session in the sidebar's pinned group. It never changes
	// UpdatedAt, so pinning does not reorder recent history.
	Pinned bool `json:"pinned,omitempty"`
	// DeletedAt marks a soft delete: every list, search and navigation read
	// skips the session while its product and native data stay on disk.
	DeletedAt int64 `json:"deleted_at,omitempty"`
	// ActiveLineID names the version line the conversation currently shows.
	// Empty means the session never had a second version: every read then
	// takes the original, unfiltered path.
	ActiveLineID string `json:"active_line_id,omitempty"`
	// ForkedFrom records the conversation this one was forked from.
	ForkedFrom *ForkOrigin `json:"forked_from,omitempty"`
}

// ForkOrigin points a forked conversation back at the place it came from: the
// source session, the Turn it kept through and the history sequence of that
// Turn's last event, so the link can open the source around that spot.
type ForkOrigin struct {
	SessionID string `json:"session_id"`
	Title     string `json:"title"`
	TurnID    string `json:"turn_id"`
	// TurnSequence is the kept Turn's ordinal in the source session.
	TurnSequence uint64 `json:"turn_sequence"`
	// EventSequence is the source history sequence of the kept Turn's last event.
	EventSequence uint64 `json:"event_sequence"`
}

// TitleHasControl reports whether a title holds a control or format rune: C0/C1
// controls (including a lone CR and NUL), line and paragraph separators, and
// bidi/zero-width format characters that can disguise or reorder text.
func TitleHasControl(title string) bool {
	for _, r := range title {
		if unicode.IsControl(r) || unicode.Is(unicode.Cf, r) || r == '\u2028' || r == '\u2029' {
			return true
		}
	}
	return false
}

// MaxPinnedSessions bounds the pinned group; pinning past it is refused.
const MaxPinnedSessions = 32

// ErrSessionRunning refuses a management change (delete) while the session
// still owns an execution or is waiting on a human decision.
var ErrSessionRunning = errors.New("session is running; stop it before deleting")

// ErrSessionHasWorkers refuses a delete while a Team Worker led by the session
// still runs or waits; stop the Workers first.
var ErrSessionHasWorkers = errors.New("session still has running Team Workers; stop them before deleting")

// AutomationDeletedReason is stored as the paused automation's LastError when
// its owning session is deleted, so the automation list can say why.
const AutomationDeletedReason = "所属会话已删除"

// ErrNothingToContinue refuses a continue Turn unless the session's newest
// Turn was stopped; the store checks it in the admission transaction.
var ErrNothingToContinue = errors.New("only a stopped reply can be continued")

// ErrPinnedBudget reports a full pinned group.
var ErrPinnedBudget = errors.New("pinned session budget exhausted")

type SessionCursor struct {
	Updated int64
	ID      string
}
type SessionPage struct {
	Sessions []Session
	Next     SessionCursor
	HasMore  bool
}
type Branch struct {
	ID              string `json:"id"`
	SessionID       string `json:"session_id"`
	Target          Target `json:"target"`
	NativeSessionID string `json:"native_session_id"`
	Watermark       uint64 `json:"watermark"`
	Parked          bool   `json:"parked"`
	// RoutesSeen remembers, per RouteKey, the connection revision and endpoint
	// that route's latest attempt actually used (at most 16 routes), so an
	// endpoint move is noticed per route, never across different routes.
	RoutesSeen map[string]RouteSeen `json:"routes_seen,omitempty"`
	// Fork is the persisted intent to start this branch's native conversation
	// as a cut copy of another one. It is written with the branch, before the
	// engine makes the copy, and cleared once the engine reports the new
	// native identity.
	Fork *BranchFork `json:"fork,omitempty"`
	// Index is the bounded Turn index a forked conversation hands an engine
	// that cannot fork natively. It is delivered with the branch's first Turn.
	Index []HandoffEntry `json:"index,omitempty"`
}

// BranchFork says which native conversation to copy and how many of its most
// recent prompts to drop. Owner is the product session that owns the source.
type BranchFork struct {
	Owner   string `json:"owner"`
	Session string `json:"session"`
	// The absolute cut, fixed when the intent was recorded from identities
	// the source already holds (see engine.ForkPoint).
	BeforeSequence  uint64 `json:"before_sequence,omitempty"`
	ThroughSequence uint64 `json:"through_sequence,omitempty"`
	AnchorTurn      string `json:"anchor_turn,omitempty"`
	AnchorKeep      bool   `json:"anchor_keep,omitempty"`
}

// Point is the engine request of the recorded cut.
func (f BranchFork) Point() engine.ForkPoint {
	return engine.ForkPoint{OwnerID: f.Owner, Session: f.Session, BeforeSequence: f.BeforeSequence, ThroughSequence: f.ThroughSequence, AnchorTurn: f.AnchorTurn, AnchorKeep: f.AnchorKeep}
}

type Turn struct {
	// Origin is empty for an ordinary prompt and "continue" for a turn the
	// product started to continue a stopped reply.
	Origin          string              `json:"origin,omitempty"`
	ID              string              `json:"id"`
	AutomationID    string              `json:"automation_id,omitempty"`
	SessionID       string              `json:"session_id"`
	Sequence        uint64              `json:"sequence"`
	Prompt          string              `json:"prompt"`
	InputFiles      []engine.InputFile  `json:"input_files,omitempty"`
	InputSkills     []engine.InputSkill `json:"input_skills,omitempty"`
	Status          engine.Outcome      `json:"status"`
	ActiveAttemptID string              `json:"active_attempt_id"`
	CreatedAt       int64               `json:"created_at"`
	EndedAt         int64               `json:"ended_at"`
}
type Attempt struct {
	ModelOptions ModelOptions           `json:"model_options,omitempty"`
	Wait         *engine.WaitCheckpoint `json:"wait,omitempty"`
	// ApprovalLines counts the settled-approval timeline entries stored for this attempt.
	ApprovalLines       int                `json:"approval_lines,omitempty"`
	ID                  string             `json:"id"`
	TurnID              string             `json:"turn_id"`
	SessionID           string             `json:"session_id"`
	BranchID            string             `json:"branch_id"`
	Target              Target             `json:"target"`
	NativeSessionID     string             `json:"native_session_id"`
	Status              engine.Outcome     `json:"status"`
	SideEffectsPossible bool               `json:"side_effects_possible"`
	Failure             engine.FailureKind `json:"failure"`
	Error               string             `json:"error"`
	CreatedAt           int64              `json:"created_at"`
	EndedAt             int64              `json:"ended_at"`
}

const Running engine.Outcome = "running"

type Detail struct {
	Session  Session  `json:"session"`
	Branches []Branch `json:"branches"`
	Fallback []Target `json:"fallback"`
}
type HistoryEntry struct {
	// Origin mirrors Turn.Origin on the turn's input entry.
	Origin      string              `json:"origin,omitempty"`
	Input       string              `json:"input,omitempty"`
	InputFiles  []engine.InputFile  `json:"input_files,omitempty"`
	InputSkills []engine.InputSkill `json:"input_skills,omitempty"`
	Sequence    uint64              `json:"sequence"`
	SessionID   string              `json:"session_id"`
	TurnID      string              `json:"turn_id"`
	AttemptID   string              `json:"attempt_id"`
	Event       engine.Event        `json:"event"`
	Time        int64               `json:"time"`
	Timeline    *TimelineEvent      `json:"timeline,omitempty"`
	// TurnSequence is the owning Turn's ordinal, filled when history is read.
	// It is never written into the stored event; events without a Turn keep 0.
	TurnSequence uint64 `json:"turn_sequence,omitempty"`
}

// Timeline kinds the product projection reads: a durable wait continued or
// abandoned on its own Turn.
const (
	TimelineWaitResumed   = "wait_resumed"
	TimelineWaitAbandoned = "wait_abandoned"
	// TimelineApprovalSettled is the one-line record of an Alva approval the
	// user decided, so the conversation keeps "allowed/denied" like it does for
	// an external engine's resolved permission.
	TimelineApprovalSettled = "approval_settled"
)

// MaxApprovalSettledPerAttempt bounds the settled-approval lines one attempt
// may store; further decisions still take effect but leave no line.
const MaxApprovalSettledPerAttempt = 256

// ApprovalSettled is a decided Alva approval: the call, the user's decision,
// the declared effect and the call's own bounded, redacted ToolDisplay.
type ApprovalSettled struct {
	CallID  string              `json:"call_id,omitempty"`
	Allowed bool                `json:"allowed"`
	Effect  engine.Effect       `json:"effect,omitempty"`
	Tool    *engine.ToolDisplay `json:"tool,omitempty"`
}

// ApprovalSettledStore is an optional port beside Store: it appends one
// settled-approval timeline entry to the attempt, bounded per attempt.
type ApprovalSettledStore interface {
	RecordApprovalSettled(ctx context.Context, sessionID, attemptID string, settled ApprovalSettled) error
}

type TimelineEvent struct {
	Kind         string  `json:"kind"`
	Text         string  `json:"text"`
	FromBranchID string  `json:"from_branch_id,omitempty"`
	ToBranchID   string  `json:"to_branch_id,omitempty"`
	HandoffID    string  `json:"handoff_id,omitempty"`
	Target       *Target `json:"target,omitempty"`
	// Revision and Endpoint audit the connection facts a route resolved to.
	Revision uint64 `json:"revision,omitempty"`
	Endpoint string `json:"endpoint,omitempty"`
	// Approval is set only on TimelineApprovalSettled entries.
	Approval *ApprovalSettled `json:"approval,omitempty"`
}
type Handoff struct {
	ID             string         `json:"id"`
	SessionID      string         `json:"session_id"`
	SourceBranchID string         `json:"source_branch_id"`
	TargetBranchID string         `json:"target_branch_id"`
	Workspace      string         `json:"workspace"`
	Through        uint64         `json:"through"`
	Entries        []HandoffEntry `json:"entries"`
	Team           *TeamSnapshot  `json:"team,omitempty"`
	Processing     string         `json:"processing"`
}
type HandoffEntry struct {
	TurnID       string         `json:"turn_id"`
	Prompt       string         `json:"prompt"`
	Status       engine.Outcome `json:"status"`
	HistoryAfter uint64         `json:"history_after"`
	BranchID     string         `json:"branch_id"`
}
type Claim struct {
	Session      Session
	Branch       Branch
	Turn         Turn
	Attempt      Attempt
	Handoff      *Handoff
	WaitRevision uint64
}

// ForkDegrader is an optional ExecutionStore capability: turn the running
// attempt's branch from a native fork into a Turn-index continuation, because
// the engine could not reach the recorded cut. It writes the fork_degraded
// notice and returns the index handoff the Turn is then run with.
type ForkDegrader interface {
	DegradeFork(ctx context.Context, attemptID string) (*Handoff, error)
}

// ExecutionStore is the complete persistence authority needed by Runtime.
// Optional settings and product browsing never become runtime dependencies.
type ExecutionStore interface {
	Detail(context.Context, string) (Detail, error)
	BeginTurn(context.Context, string, string) (Claim, error)
	BeginTurnOnBranch(context.Context, string, string, string) (Claim, error)
	BeginFallback(context.Context, string, Target) (Claim, error)
	Record(context.Context, string, engine.Event, bool) error
	FinishTurn(context.Context, string, engine.Outcome) error
}

var ErrNotFound = errors.New("product object is unavailable in this scope")

// NativeOwnerResolver reads the committed product route for the actual engine
// identity. The resolver is a storage capability, never a UI-supplied grant.
type NativeOwnerResolver interface {
	NativeOwner(context.Context, string, string) (string, error)
}

type SessionStore interface {
	CreateSession(context.Context, string, string, Target) (Session, error)
	ListSessions(context.Context, int) ([]Session, error)
	PageSessions(context.Context, string, SessionCursor, int) (SessionPage, error)
	// PinnedSessions lists the pinned, not-deleted ordinary sessions
	// (at most MaxPinnedSessions).
	PinnedSessions(context.Context) ([]Session, error)
	// LatestTurn is the session's newest Turn, or ErrNotFound when it has none.
	LatestTurn(ctx context.Context, id string) (Turn, error)
	// DetailIncludingDeleted is the internal read that still sees a soft-deleted
	// session; every other read answers ErrNotFound for it.
	DetailIncludingDeleted(ctx context.Context, id string) (Detail, error)
	RenameSession(ctx context.Context, id, title string) error
	PinSession(ctx context.Context, id string, pinned bool) error
	// DeleteSession soft-deletes one idle session. It refuses a running or
	// waiting one with ErrSessionRunning and keeps every stored fact.
	DeleteSession(ctx context.Context, id string) error
	Switch(context.Context, string, Target, string) (Branch, error)
	SetFallback(context.Context, string, []Target) error
	History(context.Context, string, uint64, int) ([]HistoryEntry, error)
	HistoryBefore(context.Context, string, uint64, int) ([]HistoryEntry, error)
	ReadHandoff(context.Context, string, string) (*Handoff, error)
	BindNative(context.Context, string, string, string) error
	Recover(context.Context) error
}

type SettingsStore interface {
	Settings(context.Context, string) (Settings, error)
	SaveSettings(context.Context, Settings) error
	SetPlanMode(context.Context, string, bool) error
	SetPermissionPolicy(context.Context, string, PermissionPolicy, bool) error
	ReplaceMCP(context.Context, string, []MCPServer) error
	SetMCPEnabled(context.Context, string, string, bool, bool) error
	SetSkillsEnabled(context.Context, string, bool) error
	SetCompanionSkills(context.Context, string, *CompanionSkillScope) error
	SetCompatibilityFile(context.Context, string, string) error
	SetSubagentsEnabled(context.Context, string, bool) error
	SetPersistentMemory(context.Context, string, bool) error
	SetMediaEnabled(context.Context, string, bool) error
	ReplaceMediaDecoder(context.Context, string, MediaDecoder) error
	SetMediaDecoderEnabled(context.Context, string, bool, bool) error
	ReplaceBrowser(context.Context, string, BrowserSettings) error
	SetBrowserEnabled(context.Context, string, bool, bool) error
	ReplaceCommands(context.Context, string, CommandSettings) error
	SetCommandEnabled(context.Context, string, string, string, bool, bool) error
	ReplaceAEP(context.Context, string, []AEPBundle) error
	SetAEPEnabled(context.Context, string, string, bool, bool) error
	RecordSkillArchive(context.Context, string, string) error
}

// Store composes the product service's ports. One SQLite implementation keeps
// the existing transactions; consumers may require only the port they use.
type Store interface {
	ExecutionStore
	SessionStore
	SettingsStore
}

func NewID(prefix string) (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return prefix + hex.EncodeToString(b[:]), nil
}
func EncodeHandoff(h *Handoff) (string, error) {
	if h == nil {
		return "", nil
	}
	data, err := json.Marshal(h)
	if err != nil {
		return "", err
	}
	if len(data) > 16<<10 {
		return "", errors.New("handoff exceeds bounded metadata budget")
	}
	return "[Hub handoff: references/index only; not original files or complete transcripts. Read relevant session history explicitly.]\n" + string(data) + "\n\n", nil
}
