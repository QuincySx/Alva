// INPUT: context, errors, unicode/utf8
// OUTPUT: Interjection, optional Interjector, ForkPoint and their bounds
// POS: [L4.5 engine contract] Optional conversation operations an engine declares through Capabilities; no product policy
package engine

import (
	"context"
	"errors"
	"strings"
	"unicode/utf8"
)

// MaxInterjectionText bounds one interjected user message.
const MaxInterjectionText = 16 << 10

// Interjection is a user message delivered into an execution that is already
// running. It takes effect from the engine's next model request and never
// interrupts a tool call that has started. ID is the product's identity for
// the message; the engine echoes it in the InputDelivered receipt.
type Interjection struct {
	ID   string `json:"id"`
	Text string `json:"text"`
}

func (i Interjection) Validate() error {
	if !nativeID(i.ID) || strings.TrimSpace(i.Text) == "" || len(i.Text) > MaxInterjectionText || !utf8.ValidString(i.Text) || strings.ContainsRune(i.Text, '\x00') {
		return errors.New("bounded interjection identity and text required")
	}
	return nil
}

// Interjector is implemented by a Runtime whose Capabilities report
// Interjection. Interject only queues the message for the running execution
// of the native session; the engine emits InputDelivered with the same ID
// once the message actually entered its model context. A nil error is not a
// delivery receipt: an execution that ends first never delivers it.
type Interjector interface {
	Interject(ctx context.Context, nativeSessionID string, input Interjection) error
}

// ForkPoint asks Execute to continue from a new native session copied from
// Session and cut before an earlier prompt, leaving Session unchanged. An
// engine declaring Capabilities.Fork reports the new identity through
// SessionStarted.
//
// The cut is absolute: the product fixes it when it records the intent, from
// identities the source already holds, so a source that grows or is rolled
// back afterwards cannot move it. BeforeSequence is an upper bound on the
// native record sequence of the first dropped prompt (the first record the
// dropped Turn produced); an engine cuts before the last prompt committed
// below it. ThroughSequence keeps every message up to that record (a fork that
// drops nothing yet; the last record the kept Turn referenced, so an
// interjection delivered during it, which precedes its final reply, is kept).
// AnchorTurn is the engine's own turn identity of the Turn the cut is made
// at: AnchorKeep false drops it and everything after, true keeps it and drops
// what follows. DropTurns counts the most recent turns to drop and is only
// used when no anchor is given.
type ForkPoint struct {
	// OwnerID is the product session that owns Session. An engine whose
	// native conversations are owner-scoped opens the source under it.
	OwnerID        string `json:"owner_id,omitempty"`
	Session        string `json:"session"`
	BeforeSequence uint64 `json:"before_sequence,omitempty"`
	// ThroughSequence replaces BeforeSequence when nothing is dropped yet:
	// keep the messages up to and including this native record sequence.
	ThroughSequence uint64 `json:"through_sequence,omitempty"`
	AnchorTurn      string `json:"anchor_turn,omitempty"`
	AnchorKeep      bool   `json:"anchor_keep,omitempty"`
	DropTurns       int    `json:"drop_turns"`
}

// ErrForkUnreachable is returned by Execute, before it reports anything, when a
// fork's recorded cut cannot be reached in the source (its turn or record is
// gone or lies beyond what the engine can read). The product then turns the
// branch into a Turn-index continuation and runs the Turn again.
var ErrForkUnreachable = errors.New("the recorded fork cut is not reachable in the source conversation")

// ForkChecker is implemented by an engine that can say, before the product
// commits to a native fork, whether the cut is reachable (for example that it
// lies inside the part of the source the engine can read). A fork the engine
// refuses is recorded as unsupported and the conversation continues from the
// Turn index instead; nothing is created.
type ForkChecker interface {
	CheckFork(ctx context.Context, fork ForkPoint) error
}

func (f ForkPoint) Validate() error {
	if !nativeID(f.Session) || f.DropTurns < 0 || f.DropTurns > 100000 || f.AnchorTurn != "" && !nativeID(f.AnchorTurn) || f.OwnerID != "" && !nativeID(f.OwnerID) {
		return errors.New("bounded fork source and point required")
	}
	return nil
}

func nativeID(value string) bool {
	if value == "" || len(value) > 128 {
		return false
	}
	for _, c := range value {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_' || c == '.' || c == ':') {
			return false
		}
	}
	return true
}
