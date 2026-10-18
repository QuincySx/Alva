// INPUT: testing and canonical engine events
// OUTPUT: stream lifecycle, pairing and fatal-edge regressions
// POS: [L4.5 engine contract] Portable contract conformance independent of concrete engines
package engine

import "testing"

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
