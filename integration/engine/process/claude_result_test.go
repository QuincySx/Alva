// INPUT: Local fixture subprocesses, temporary identity storage and typed engine events
// OUTPUT: Claude result error, identity and post-result lifecycle regressions without a user CLI
// POS: [integration / native external-engine adapter] A zero process exit cannot override failed or conflicting native results
package process

import (
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

func TestClaudeNativeResultTruthSurvivesSuccessfulProcessExit(t *testing.T) {
	for _, mode := range []string{"result-is-error", "result-foreign", "after-result", "result-notice"} {
		t.Run(mode, func(t *testing.T) {
			adapter, _ := fixtureAdapter(t, "claude", mode)
			var started string
			var outcome engine.Outcome
			var terminalCount int
			var messages []string
			err := adapter.Execute(t.Context(), engine.Request{OwnerID: "fixture-owner", Workspace: t.TempDir(), Prompt: "ordinary fixture input"}, func(event engine.Event) error {
				switch event.Kind {
				case engine.SessionStarted:
					started = event.SessionID
				case engine.Message, engine.MessageDelta:
					messages = append(messages, event.Text)
				case engine.Completed:
					outcome = event.Outcome
					terminalCount++
				}
				return nil
			})
			if err != nil || started != "fixture-native" || terminalCount != 1 {
				t.Fatal("fixture did not finish through the actual adapter", err, started, terminalCount)
			}
			want := engine.Failed
			if mode == "result-notice" {
				want = engine.Succeeded
			}
			if outcome != want {
				t.Fatalf("process exit hid native result: got %s, want %s", outcome, want)
			}
			for _, text := range messages {
				if text == "must not replace completed turn" {
					t.Fatal("post-result output rewrote the finished turn")
				}
			}
		})
	}
}
