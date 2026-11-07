// INPUT: Pi 1.4 JSON session events and project-owned helper subprocesses
// OUTPUT: Settled-result, recoverable overflow and zero-exit failure regressions
// POS: [integration / native external-engine adapter] Native completion stays distinct from a low-level agent run ending
package process

import (
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/engine"
)

func TestPiLowLevelEndCannotCompleteBeforeSessionSettles(t *testing.T) {
	m := newMapper("pi", "engine-journal://fixture")
	for _, line := range []string{
		`{"type":"session","id":"fixture-native"}`,
		`{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"context overflow"}}`,
		`{"type":"agent_end","willRetry":false}`,
		`{"type":"compaction_start","reason":"overflow"}`,
		`{"type":"compaction_end","reason":"overflow","willRetry":true,"aborted":false}`,
		`{"type":"agent_start"}`,
		`{"type":"message_start","message":{"role":"assistant"}}`,
		`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"recovered"}]}}`,
		`{"type":"agent_end","willRetry":false}`,
	} {
		if _, err := m.mapLine([]byte(line)); err != nil {
			t.Fatal(err)
		}
		if m.done {
			t.Fatal("low-level completion claimed a settled session")
		}
	}
	if _, err := m.mapLine([]byte(`{"type":"agent_settled","aborted":false}`)); err != nil || !m.done || m.pending != "" || m.failure != "" {
		t.Fatal("recovered session did not settle successfully", m.done, m.pending, m.failure, err)
	}
	if _, err := m.mapLine([]byte(`{"type":"message_start","message":{"role":"assistant"}}`)); err == nil {
		t.Fatal("execution reopened a settled session")
	}
}

func TestPiEndedMessagesReleaseTheirCumulativeText(t *testing.T) {
	m := newMapper("pi", "engine-journal://cache-fixture")
	mapFrame := func(value any) error { _, err := m.mapLine([]byte(mustJSON(value))); return err }
	for i := 0; i < 128; i++ {
		if err := mapFrame(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}}); err != nil {
			t.Fatal(err)
		}
		if err := mapFrame(map[string]any{"type": "message_update", "assistantMessageEvent": map[string]string{"type": "text_delta", "delta": strings.Repeat("x", 20<<10)}}); err != nil {
			t.Fatal(err)
		}
		if len(m.texts) != 1 || len(m.texts[m.current]) != 20<<10 {
			t.Fatal("Pi active text cache contains ended messages", i, len(m.texts))
		}
		reason := []string{"stop", "error", "aborted", "length"}[i%4]
		if err := mapFrame(map[string]any{"type": "message_end", "message": map[string]any{"role": "assistant", "stopReason": reason, "content": []any{map[string]string{"type": "text", "text": strings.Repeat("x", 20<<10)}}}}); err != nil {
			t.Fatal(err)
		}
		if len(m.texts) != 0 || m.current != "" {
			t.Fatal("Pi completed/error/aborted message retained cumulative text", i, reason, len(m.texts), m.current)
		}
	}
	if err := mapFrame(map[string]any{"type": "message_update", "assistantMessageEvent": map[string]string{"type": "text_delta", "delta": strings.Repeat("x", 1<<20)}}); err != nil {
		t.Fatal(err)
	}
	if err := mapFrame(map[string]any{"type": "message_update", "assistantMessageEvent": map[string]string{"type": "text_delta", "delta": "overflow"}}); err == nil {
		t.Fatal("Pi active cumulative message exceeded 1 MiB")
	}
	if len(m.texts) != 1 || len(m.texts[m.current]) != 1<<20 {
		t.Fatal("Pi active cache grew beyond its bounded message")
	}
	if err := mapFrame(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}}); err != nil {
		t.Fatal(err)
	}
	if len(m.texts) != 0 {
		t.Fatal("a replacement native message retained the abandoned current text")
	}
}

func TestPiZeroExitCannotHideIncompleteOrUnsettledNativeResult(t *testing.T) {
	for _, mode := range []string{"pi-aborted", "pi-length", "pi-unsettled"} {
		t.Run(mode, func(t *testing.T) {
			a, _ := fixtureAdapter(t, "pi", mode)
			var terminal engine.Outcome
			var failure engine.FailureKind
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fixture"}, func(event engine.Event) error {
				if event.Kind == engine.Completed {
					terminal = event.Outcome
				}
				if event.Kind == engine.Error {
					failure = event.Failure
				}
				return nil
			})
			if err != nil || terminal != engine.Failed || failure != engine.FailureExecution {
				t.Fatal("zero process exit hid an incomplete Pi result", terminal, failure, err)
			}
		})
	}
}
