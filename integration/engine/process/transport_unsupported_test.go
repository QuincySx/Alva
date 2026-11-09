// INPUT: Codex app-server and Pi RPC helper children that refuse their handshake the way an engine too old for approvals would
// OUTPUT: A typed startup failure naming the unsupported approval transport, with no silent fallback to a transport that never asks
// POS: [integration / native external-engine adapter tests] Old-engine detection without installed CLIs or accounts
package process

import (
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

type engineFixture func(*testing.T, string) *Adapter

var handshakeFixtures = map[string]engineFixture{
	"codex": func(t *testing.T, mode string) *Adapter { a, _ := codexAppServerFixture(t, mode); return a },
	"pi":    func(t *testing.T, mode string) *Adapter { a, _, _ := piRPCFixture(t, mode); return a },
	"claude": func(t *testing.T, mode string) *Adapter {
		return newClaudeStreamAdapter(t, mode, "stream-json").adapter
	},
}

// A refused handshake that is not an unknown method/option (auth failure) keeps
// the generic startup-failure text: it must not claim the engine is too old.
func TestHandshakeFailuresThatAreNotUnknownMethodKeepTheGenericMessage(t *testing.T) {
	for name, build := range handshakeFixtures {
		t.Run(name, func(t *testing.T) {
			a := build(t, "auth-failure")
			var failure engine.Event
			if err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt"}, func(event engine.Event) error {
				if event.Kind == engine.Error {
					failure = event
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			if failure.Text == "" || strings.Contains(failure.Text, "版本过旧") {
				t.Fatalf("auth failure must not be reported as an old engine: %+v", failure)
			}
		})
	}
}

func TestOldEngineHandshakeFailureIsTypedAndNeverFallsBackToExec(t *testing.T) {
	for name, build := range map[string]engineFixture{"codex": handshakeFixtures["codex"], "pi": handshakeFixtures["pi"]} {
		t.Run(name, func(t *testing.T) {
			a := build(t, "old-engine")
			var failure engine.Event
			var terminal engine.Event
			if err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt"}, func(event engine.Event) error {
				if event.Kind == engine.Error {
					failure = event
				}
				if event.Kind == engine.Completed {
					terminal = event
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			if terminal.Outcome != engine.Failed || failure.Failure != engine.FailureStartup || !strings.Contains(failure.Text, "版本过旧") || !strings.Contains(failure.Text, "不会自动降级") {
				t.Fatalf("an engine that cannot do the approval transport must fail as a typed startup error: %+v / %+v", failure, terminal)
			}
			if !a.Capabilities().PermissionCallback {
				t.Fatal("the transport still declares approvals; fallback is the user's explicit choice, not ours")
			}
		})
	}
}
