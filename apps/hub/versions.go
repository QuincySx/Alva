// INPUT: context, errors and engine outcomes
// OUTPUT: Line, LineSegment, LineGroup, LineFork, VersionStore, PendingInput, PendingStore and their bounds and errors
// POS: [L7 Hub product] Version lines (edit/regenerate versions of one conversation) and the durable input queue as product facts independent of engines and UI
package hub

import (
	"context"
	"errors"
	"strings"
	"unicode/utf8"
)

// Version line bounds.
const (
	MaxLinesPerSession    = 64
	MaxSegmentsPerLine    = 64
	MaxBranchesPerSession = 256
)

// LineSegment is one visible run of a line, in session event and Turn
// sequences. An open segment (the active line's newest run) has no upper
// bound; a closed one is inclusive on both ends, and a bound below its lower
// one means it holds nothing on that axis.
type LineSegment struct {
	FromSeq  uint64 `json:"from_seq"`
	ToSeq    uint64 `json:"to_seq,omitempty"`
	FromTurn uint64 `json:"from_turn"`
	ToTurn   uint64 `json:"to_turn,omitempty"`
	Closed   bool   `json:"closed,omitempty"`
}

// Open reports whether the segment still grows.
func (s LineSegment) Open() bool { return !s.Closed }

// LineGroup places a line at one fork position: Group names the position,
// TurnID is the Turn this line shows there, Seq that Turn's ordinal.
type LineGroup struct {
	Group  string `json:"group"`
	TurnID string `json:"turn_id"`
	Seq    uint64 `json:"seq"`
}

// Line is one version of a conversation. Its visible history is its segments;
// a session without lines has none and reads its whole history.
type Line struct {
	ID        string        `json:"id"`
	SessionID string        `json:"session_id"`
	ParentID  string        `json:"parent_id,omitempty"`
	Ordinal   int           `json:"ordinal"`
	CreatedAt int64         `json:"created_at"`
	TouchedAt int64         `json:"touched_at"`
	Segments  []LineSegment `json:"segments"`
	Groups    []LineGroup   `json:"groups,omitempty"`
	// ActiveBranchID is the engine branch the line last ran on, restored when
	// the line becomes current again.
	ActiveBranchID string `json:"active_branch_id,omitempty"`
}

// LineFork asks Turn admission to start a new version line cut before TurnID
// in the same transaction that admits the Turn. NativeFork says the engine of
// the active branch can continue from a cut copy of its native conversation;
// otherwise the new line's branch gets a fresh native conversation and the
// Turn index.
type LineFork struct {
	TurnID     string
	Regenerate bool
	NativeFork bool
}

var (
	// ErrVersionBusy refuses a version change while the session runs or waits.
	ErrVersionBusy = errors.New("versions change only while the conversation is idle")
	// ErrVersionUnsupported refuses a Turn that cannot be edited, regenerated
	// or forked: one with files or Skills, a continue Turn, a scheduled Turn or
	// one of a Team, companion or automation session.
	ErrVersionUnsupported = errors.New("this Turn does not support versions")
	// ErrVersionBudget reports a full line budget.
	ErrVersionBudget = errors.New("version line budget exhausted")
)

// VersionStore is the optional product port for version lines and forks.
type VersionStore interface {
	// Lines lists the session's version lines (none when it has no versions).
	Lines(ctx context.Context, id string) ([]Line, error)
	// SelectLine switches the shown version of an idle session.
	SelectLine(ctx context.Context, id, lineID string) error
	// ForkSession creates a new session continuing from the end of a Turn.
	ForkSession(ctx context.Context, id string, input SessionFork) (Session, error)
	// ForkAnchor fixes the native cut of a fork before a Turn as an absolute
	// anchor. It reports false when the Turn did not run on the active branch's
	// own native conversation, so no reliable anchor exists.
	// keepThrough selects a fork that keeps the Turn (a new conversation);
	// otherwise the Turn is the first one dropped (edit, regenerate).
	ForkAnchor(ctx context.Context, id, turnID string, keepThrough bool) (BranchFork, bool, error)
	// VersionsDisabled explains why the whole conversation has no versions or
	// forks (Team, companion and automation conversations); empty when it can.
	VersionsDisabled(ctx context.Context, id string) (string, error)
	// TurnVersionable reads a Turn of the current version and explains why it
	// cannot be edited, regenerated or forked (Reason empty when it can).
	TurnVersionable(ctx context.Context, id, turnID string) (VersionTurn, error)
}

// VersionTurn is a Turn with its original user text and version eligibility.
type VersionTurn struct {
	Turn     Turn
	UserText string
	Reason   string
}

// SessionFork is the input of ForkSession.
type SessionFork struct {
	TurnID     string
	NativeFork bool
}

// Pending input bounds and states.
const (
	MaxPendingInputs    = 8
	MaxPendingInputText = 16 << 10
)

// PendingState is a queued input's state.
type PendingState string

const (
	// PendingQueued waits for dispatch.
	PendingQueued PendingState = "queued"
	// PendingInterjecting was handed to a running engine and has no delivery
	// receipt yet. A delivery receipt deletes it; the end of the Turn without
	// one returns it to the head of the queue.
	PendingInterjecting PendingState = "interjecting"
	// PendingUncertain was interjecting when the application exited: it may or
	// may not have reached the model, so it is never sent again automatically.
	PendingUncertain PendingState = "uncertain"
)

// PendingInput is one persisted queued message.
type PendingInput struct {
	ID        string       `json:"id"`
	SessionID string       `json:"session_id"`
	Text      string       `json:"text"`
	State     PendingState `json:"state"`
	CreatedAt int64        `json:"created_at"`
	// Error is why the last dispatch attempt did not start the Turn.
	Error string `json:"error,omitempty"`
	// Position orders the queue; filled when read.
	Position int64 `json:"-"`
}

var (
	// ErrPendingFull refuses a ninth queued input.
	ErrPendingFull = errors.New("at most 8 queued inputs per conversation")
	// ErrPendingGone reports an input that was dispatched or cancelled meanwhile.
	ErrPendingGone = errors.New("queued input is no longer queued")
)

// ValidPendingText checks a queued or interjected message: 1..16 KiB of
// UTF-8 with something visible in it.
func ValidPendingText(text string) error {
	return validMessageText(text, MaxPendingInputText)
}

// MaxTurnText bounds a user message sent as a Turn (edit and resend), the
// same budget an ordinary prompt has.
const MaxTurnText = 64 << 10

// ValidTurnText checks the text of an edited message.
func ValidTurnText(text string) error {
	return validMessageText(text, MaxTurnText)
}

func validMessageText(text string, limit int) error {
	if strings.TrimSpace(text) == "" || len(text) > limit || !utf8.ValidString(text) {
		return errors.New("message must be non-blank valid text within its size limit")
	}
	for _, r := range text {
		if r == 0 {
			return errors.New("message cannot contain NUL")
		}
	}
	return nil
}

// PendingStore is the optional product port for the durable input queue.
type PendingStore interface {
	// QueueInput appends a queued input (state queued, or interjecting when
	// the caller is about to hand it to a running engine).
	QueueInput(ctx context.Context, id, text string, state PendingState) (PendingInput, error)
	// PendingInputs lists the queue in dispatch order (at most 8).
	PendingInputs(ctx context.Context, id string) ([]PendingInput, error)
	// EditPendingInput replaces a still-queued input's text.
	EditPendingInput(ctx context.Context, id, inputID, text string) (PendingInput, error)
	// CancelPendingInput deletes a still-queued input.
	CancelPendingInput(ctx context.Context, id, inputID string) error
	// RequeuePendingInput returns an interjecting input to the queue head and
	// records why, without delivery.
	RequeuePendingInput(ctx context.Context, id, inputID, reason string) error
	// NotePendingError records why dispatching the input failed.
	NotePendingError(ctx context.Context, id, inputID, reason string) error
}
