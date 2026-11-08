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
// SessionStarted. BeforeSequence is the native record sequence of the first
// record of the first dropped prompt when the product knows it (0 when not);
// DropTurns counts the most recent prompts on Session to drop (0 keeps all).
type ForkPoint struct {
	Session        string `json:"session"`
	BeforeSequence uint64 `json:"before_sequence,omitempty"`
	DropTurns      int    `json:"drop_turns"`
}

func (f ForkPoint) Validate() error {
	if !nativeID(f.Session) || f.DropTurns < 0 || f.DropTurns > 100000 {
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
