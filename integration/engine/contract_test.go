// INPUT: encoding/json, strings, testing and canonical engine events
// OUTPUT: stream lifecycle, pairing, fatal-edge and bounded tool-evidence regressions
// POS: [L4.5 engine contract] Portable contract conformance independent of concrete engines
package engine

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestSingleTerminalAndFatalOnlyCompletion(t *testing.T) {
	for _, events := range [][]Event{
		{{Kind: SessionStarted, SessionID: "native"}, {Kind: MessageDelta, ID: "m", Text: "hello"}, {Kind: Completed, Outcome: Succeeded}},
		{{Kind: Error, Text: "could not spawn", Failure: FailureStartup}, {Kind: Completed, Outcome: Failed}},
		{{Kind: ToolStart, ID: "t", Name: "read"}, {Kind: ToolEnd, ID: "t", Name: "read"}, {Kind: Completed, Outcome: Cancelled}},
	} {
		var contract StreamContract
		for _, event := range events {
			if err := contract.Accept(event); err != nil {
				t.Fatal(err)
			}
		}
		if !contract.Terminal {
			t.Fatal("missing terminal")
		}
		if err := contract.Accept(Event{Kind: Completed, Outcome: Succeeded}); err == nil {
			t.Fatal("accepted duplicate completion")
		}
	}
	var contract StreamContract
	_ = contract.Accept(Event{Kind: Error, Text: "fatal"})
	if err := contract.Accept(Event{Kind: MessageDelta, ID: "m", Text: "after fatal"}); err == nil {
		t.Fatal("accepted output after fatal")
	}
}
func TestDanglingToolsAndMissingIdentitiesFail(t *testing.T) {
	var c StreamContract
	if err := c.Accept(Event{Kind: ToolStart, ID: "t", Name: "tool"}); err != nil {
		t.Fatal(err)
	}
	if c.Accept(Event{Kind: Completed, Outcome: Succeeded}) == nil {
		t.Fatal("dangling tool accepted")
	}
	if c.Accept(Event{Kind: ToolStart, ID: "t", Name: "tool"}) == nil {
		t.Fatal("duplicate open accepted")
	}
	if (&StreamContract{}).Accept(Event{Kind: SessionStarted}) == nil {
		t.Fatal("fake native identity accepted")
	}
}

func TestKnownActivityNativeIdentityIsNotStartedAndCannotBeRebound(t *testing.T) {
	activity := Event{Kind: ExecutionActivity, SessionID: "actual-native", Activity: &Activity{ID: "prepare", Name: "known native launch", Effect: EffectUnknown}}
	var c StreamContract
	if err := c.Accept(activity); err != nil || c.Started || c.NativeSessionID != "actual-native" {
		t.Fatal("known activity falsely claimed Agent start", err)
	}
	other := c.Clone()
	if other.Accept(Event{Kind: SessionStarted, SessionID: "foreign"}) == nil {
		t.Fatal("activity native identity rebound")
	}
	if err := c.Accept(Event{Kind: SessionStarted, SessionID: "actual-native"}); err != nil {
		t.Fatal(err)
	}
}

func TestWaitingRequiresExclusiveStartedCheckpointReceipt(t *testing.T) {
	valid := &WaitCheckpoint{Revision: 3, Requests: 1, ExpiresAt: 1000}
	for _, test := range []struct {
		name           string
		started, fatal bool
		event          Event
		ok             bool
	}{
		{"waiting", true, false, Event{Kind: Completed, Outcome: Waiting, Wait: valid}, true},
		{"not-started", false, false, Event{Kind: Completed, Outcome: Waiting, Wait: valid}, false},
		{"missing-receipt", true, false, Event{Kind: Completed, Outcome: Waiting}, false},
		{"invalid-receipt", true, false, Event{Kind: Completed, Outcome: Waiting, Wait: &WaitCheckpoint{Revision: 1, Requests: 17, ExpiresAt: 1}}, false},
		{"failed-with-receipt", true, false, Event{Kind: Completed, Outcome: Failed, Wait: valid}, false},
		{"nonterminal-receipt", true, false, Event{Kind: Message, ID: "m", Wait: valid}, false},
		{"fatal-cannot-wait", true, true, Event{Kind: Completed, Outcome: Waiting, Wait: valid}, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			var c StreamContract
			if test.started {
				if err := c.Accept(Event{Kind: SessionStarted, SessionID: "native"}); err != nil {
					t.Fatal(err)
				}
			}
			if test.fatal {
				if err := c.Accept(Event{Kind: Error, Text: "cleanup failed"}); err != nil {
					t.Fatal(err)
				}
			}
			if err := c.Accept(test.event); (err == nil) != test.ok {
				t.Fatal(test.name, err)
			}
			if test.ok && !c.Terminal {
				t.Fatal("waiting did not end the actual execution")
			}
		})
	}
}

func TestTruncatedOnlyMarksBoundedFinalContent(t *testing.T) {
	var c StreamContract
	for _, event := range []Event{
		{Kind: ToolStart, ID: "t", Name: "read"},
		{Kind: ToolEnd, ID: "t", Name: "read", Text: "prefix", Truncated: true},
		{Kind: ContentBlock, ID: "b", Name: "text", Text: "prefix", Truncated: true},
		{Kind: Message, ID: "m", Text: "prefix", Truncated: true, Reference: &Reference{SessionID: "native", RecordID: "r"}},
	} {
		if err := c.Accept(event); err != nil {
			t.Fatal("bounded final content rejected", event.Kind, err)
		}
	}
	for _, kind := range []Kind{MessageDelta, ReasoningDelta, ToolStart, Error, Completed} {
		probe := StreamContract{}
		if probe.Accept(Event{Kind: kind, ID: "x", Name: "x", Text: "x", Outcome: Succeeded, Truncated: true}) == nil {
			t.Fatal("truncation accepted on a non-final event", kind)
		}
	}
}

func TestToolEvidenceStaysWithinEventBounds(t *testing.T) {
	var c StreamContract
	input := json.RawMessage(`{"path":"` + strings.Repeat("p", (16<<10)-12) + `"}`)
	reference := &Reference{SessionID: "native", RecordID: "outcome", Sequence: 9}
	for _, event := range []Event{
		{Kind: ToolStart, ID: "t", Name: "read_file", Input: input, Reference: &Reference{SessionID: "native", RecordID: "intent", Sequence: 8}},
		{Kind: ToolEnd, ID: "t", Name: "read_file", Text: strings.Repeat("r", 8<<10), Truncated: true, IsError: true, Reference: reference},
	} {
		if err := c.Accept(event); err != nil {
			t.Fatal("bounded tool evidence rejected", event.Kind, len(event.Input), err)
		}
	}
	for _, event := range []Event{
		{Kind: ToolStart, ID: "u", Name: "write_file", Input: json.RawMessage(`{"content":"` + strings.Repeat("c", 16<<10) + `"}`)},
		{Kind: ToolStart, ID: "v", Name: "execute_shell", Text: strings.Repeat("o", (64<<10)+1)},
	} {
		probe := c.Clone()
		if probe.Accept(event) == nil {
			t.Fatal("unbounded tool evidence accepted without a reference", event.Name)
		}
	}
}
