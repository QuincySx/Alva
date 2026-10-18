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
func RouteKey(target Target) string {
	target.AutomaticAuthorized = false
	target.RequiredCapabilities = append([]string(nil), target.RequiredCapabilities...)
	sort.Strings(target.RequiredCapabilities)
	data, _ := json.Marshal(target)
	return string(data)
}

type Session struct {
	ID             string         `json:"id"`
	Title          string         `json:"title"`
	Workspace      string         `json:"workspace"`
	ActiveBranchID string         `json:"active_branch_id"`
	Status         engine.Outcome `json:"status"`
	CreatedAt      int64          `json:"created_at"`
	UpdatedAt      int64          `json:"updated_at"`
}
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
}
type Turn struct {
	ID              string         `json:"id"`
	AutomationID    string         `json:"automation_id,omitempty"`
	SessionID       string         `json:"session_id"`
	Sequence        uint64         `json:"sequence"`
	Prompt          string         `json:"prompt"`
	Status          engine.Outcome `json:"status"`
	ActiveAttemptID string         `json:"active_attempt_id"`
	CreatedAt       int64          `json:"created_at"`
	EndedAt         int64          `json:"ended_at"`
}
type Attempt struct {
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
	Input     string         `json:"input,omitempty"`
	Sequence  uint64         `json:"sequence"`
	SessionID string         `json:"session_id"`
	TurnID    string         `json:"turn_id"`
	AttemptID string         `json:"attempt_id"`
	Event     engine.Event   `json:"event"`
	Time      int64          `json:"time"`
	Timeline  *TimelineEvent `json:"timeline,omitempty"`
}
type TimelineEvent struct {
	Kind         string  `json:"kind"`
	Text         string  `json:"text"`
	FromBranchID string  `json:"from_branch_id,omitempty"`
	ToBranchID   string  `json:"to_branch_id,omitempty"`
	HandoffID    string  `json:"handoff_id,omitempty"`
	Target       *Target `json:"target,omitempty"`
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
	Session Session
	Branch  Branch
	Turn    Turn
	Attempt Attempt
	Handoff *Handoff
}
type Store interface {
	CreateSession(context.Context, string, string, Target) (Session, error)
	ListSessions(context.Context, int) ([]Session, error)
	PageSessions(context.Context, string, SessionCursor, int) (SessionPage, error)
	Detail(context.Context, string) (Detail, error)
	Switch(context.Context, string, Target, string) (Branch, error)
	SetFallback(context.Context, string, []Target) error
	BeginTurn(context.Context, string, string) (Claim, error)
	BeginTurnOnBranch(context.Context, string, string, string) (Claim, error)
	BeginFallback(context.Context, string, Target) (Claim, error)
	Record(context.Context, string, engine.Event, bool) error
	FinishTurn(context.Context, string, engine.Outcome) error
	History(context.Context, string, uint64, int) ([]HistoryEntry, error)
	HistoryBefore(context.Context, string, uint64, int) ([]HistoryEntry, error)
	ReadHandoff(context.Context, string, string) (*Handoff, error)
	BindNative(context.Context, string, string, string) error
	Settings(context.Context, string) (Settings, error)
	SaveSettings(context.Context, Settings) error
	ReplaceMCP(context.Context, string, []MCPServer) error
	SetMCPEnabled(context.Context, string, string, bool, bool) error
	SetSkillsEnabled(context.Context, string, bool) error
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
	Recover(context.Context) error
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
