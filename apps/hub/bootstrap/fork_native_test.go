// INPUT: Hand-built conversation windows with prompts, tool exchanges, interjected user messages and native record sequences
// OUTPUT: Regression of the absolute fork cut: it keys on native record sequences, never counts an interjection as a prompt, ignores prompts the source gained later, and refuses a cut it cannot reach instead of returning an empty context
// POS: [L7 Hub bootstrap] The fork cut decides how much context a forked Alva session keeps; a count-based cut shifts when the source grows or holds fewer prompts than the product has Turns
package bootstrap

import (
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// forkWindowFixture is P1 A1 | P2 A2call tool I2 A2 | P3 A3 with native sequences
// 1..9 (the dropped Turn's first record sits just above its prompt).
func forkWindowFixture() forkWindow {
	user := func(id, text string) wire.Message {
		m := wire.TextMessage(wire.RoleUser, text)
		m.ID = id
		return m
	}
	return forkWindow{
		messages: []wire.Message{
			user("m1", "P1"), wire.TextMessage(wire.RoleAssistant, "A1"),
			user("m2", "P2"), wire.TextMessage(wire.RoleAssistant, "A2-call"), wire.TextMessage(wire.RoleTool, "tool-result"),
			user(alva.InterjectionMessagePrefix+"input-1", "I2"), wire.TextMessage(wire.RoleAssistant, "A2"),
			user("m3", "P3"), wire.TextMessage(wire.RoleAssistant, "A3"),
		},
		sequences: []uint64{1, 3, 5, 7, 9, 11, 13, 15, 17},
	}
}

func texts(messages []wire.Message) string {
	var parts []string
	for _, m := range messages {
		parts = append(parts, m.Text())
	}
	return strings.Join(parts, ",")
}

func TestForkCutIsAnAbsoluteNativeSequenceAndIgnoresInterjections(t *testing.T) {
	window := forkWindowFixture()
	for _, c := range []struct {
		name string
		fork engine.ForkPoint
		want string
	}{
		{"drop the third Turn (first record 16, prompt at 15)", engine.ForkPoint{BeforeSequence: 16}, "P1,A1,P2,A2-call,tool-result,I2,A2"},
		{"drop the second Turn (first record 6)", engine.ForkPoint{BeforeSequence: 6}, "P1,A1"},
		{"the interjection at 11 is never a prompt: a bound just above it still drops the whole second Turn", engine.ForkPoint{BeforeSequence: 12}, "P1,A1"},
		{"drop the first Turn (prompt is the first message)", engine.ForkPoint{BeforeSequence: 2}, ""},
		{"keep through the second Turn's last record", engine.ForkPoint{ThroughSequence: 13}, "P1,A1,P2,A2-call,tool-result,I2,A2"},
		{"keep through the first Turn", engine.ForkPoint{ThroughSequence: 3}, "P1,A1"},
	} {
		kept, err := cutForkWindow(window, c.fork)
		if err != nil || texts(kept) != c.want {
			t.Fatalf("%s: kept %q (%v), want %q", c.name, texts(kept), err, c.want)
		}
	}
	// Prompts the source gained after the cut was recorded do not move it: the
	// same anchor on a longer source keeps exactly the same prefix.
	longer := forkWindowFixture()
	longer.messages = append(longer.messages, wire.TextMessage(wire.RoleUser, "P4"), wire.TextMessage(wire.RoleAssistant, "A4"), wire.TextMessage(wire.RoleUser, "P5"))
	longer.sequences = append(longer.sequences, 19, 21, 23)
	kept, err := cutForkWindow(longer, engine.ForkPoint{BeforeSequence: 6})
	if err != nil || texts(kept) != "P1,A1" {
		t.Fatal("a source that grew moved the cut", texts(kept), err)
	}
	// The source has fewer prompts than the product has Turns (one Turn wrote
	// none): a count-based cut would drop too much; the sequence cut does not care.
	fewer := forkWindow{messages: window.messages[2:], sequences: window.sequences[2:]}
	kept, err = cutForkWindow(fewer, engine.ForkPoint{BeforeSequence: 16})
	if err != nil || texts(kept) != "P2,A2-call,tool-result,I2,A2" {
		t.Fatal("a source with fewer prompts moved the cut", texts(kept), err)
	}
}

func TestForkCutRefusesWhatItCannotReachInsteadOfAnEmptyContext(t *testing.T) {
	window := forkWindowFixture()
	for _, c := range []struct {
		name   string
		window forkWindow
		fork   engine.ForkPoint
	}{
		{"no anchor", window, engine.ForkPoint{}},
		{"no prompt below the anchor in a short source", forkWindow{messages: window.messages[3:5], sequences: window.sequences[3:5]}, engine.ForkPoint{BeforeSequence: 10}},
		{"the prompt is older than a truncated tail", forkWindow{messages: window.messages[3:], sequences: window.sequences[3:], truncated: true}, engine.ForkPoint{BeforeSequence: 8}},
		{"a through-cut before a truncated tail", forkWindow{messages: window.messages[3:], sequences: window.sequences[3:], truncated: true}, engine.ForkPoint{ThroughSequence: 3}},
	} {
		if kept, err := cutForkWindow(c.window, c.fork); err == nil {
			t.Fatalf("%s: an unreachable cut returned %q instead of an error", c.name, texts(kept))
		}
	}
}
