// INPUT: hubFixture real Alva assembly, fakeModel held/failing streams, Runtime Stop by observed execution and a killed helper process
// OUTPUT: Scenarios for streaming, delta cleanup and usage, Stop mid-reply, provider failures with fallback, and recovery after a crash
// POS: [scenario] One Turn from the user's point of view, including the ways it can end badly.
package scenarios

import (
	"bytes"
	"context"
	"errors"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func lastReply(t *testing.T, blocks []product.Block) product.Block {
	t.Helper()
	replies := blocksOf(blocks, product.BlockKindReply)
	if len(replies) == 0 {
		t.Fatal("no reply block")
	}
	return replies[len(replies)-1]
}

func TestStreamingReplyShowsPartialTextThenFinalUsageAndCleansDeltas(t *testing.T) {
	for _, kind := range []provider.Kind{provider.OpenAIChatKind, provider.AnthropicKind} {
		t.Run(string(kind), func(t *testing.T) {
			hold, reached := make(chan struct{}), make(chan struct{})
			model := newFakeModel(t, kind, reply{Text: []string{"Hello", ", ", "streaming "}, Hold: hold, Reached: reached, After: []string{"world", "."}, Input: 12, Usage: 5})
			h := openHub(t, liveConfig(t, model))
			s := h.create("stream", "")
			h.send(s.ID, "say hello")
			received(t, reached, "the first deltas")
			// While the model pauses, the received text is already visible.
			waitFor(t, "partial reply in the Message Stream", func() bool {
				for _, b := range blocksOf(h.blocks(s.ID), product.BlockKindReply) {
					if b.Status == product.BlockStatusRunning && b.Text == "Hello, streaming " {
						return true
					}
				}
				return false
			})
			close(hold)
			h.expect(s.ID, engine.Succeeded)
			blocks := h.blocks(s.ID)
			if r := lastReply(t, blocks); r.Text != "Hello, streaming world." || r.Status != product.BlockStatusCompleted {
				t.Fatalf("final reply %+v", r)
			}
			var usage *engine.Usage
			for _, b := range blocksOf(blocks, product.BlockKindStatus) {
				if b.Name == "usage" {
					usage = b.Usage
				}
			}
			if usage == nil || usage.InputTokens != 12 || usage.OutputTokens != 5 {
				t.Fatalf("usage block %+v", usage)
			}
			// The final message replaced its deltas in the same commit.
			for _, e := range h.history(s.ID) {
				if e.Event.Kind == engine.MessageDelta {
					t.Fatalf("delta row %d survived the final message: %q", e.Sequence, e.Event.Text)
				}
			}
		})
	}
}

func TestStopMidReplyKeepsPartialTextAndTheUserCanContinue(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	defer close(hold)
	model := newFakeModel(t, provider.OpenAIChatKind,
		reply{Text: []string{"partial ", "answer"}, Hold: hold, Reached: reached},
		text("second answer"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("stop", "")
	h.send(s.ID, "long question")
	received(t, reached, "the partial reply")
	waitFor(t, "partial reply", func() bool {
		r := blocksOf(h.blocks(s.ID), product.BlockKindReply)
		return len(r) == 1 && r[0].Text == "partial answer"
	})
	execution, active := h.app.Runtime.ActiveExecution(s.ID)
	if !active || execution == "" {
		t.Fatal("no observed execution identity to stop")
	}
	if err := h.app.Runtime.CancelExecution(context.Background(), s.ID, execution); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Cancelled)
	r := lastReply(t, h.blocks(s.ID))
	if r.Text != "partial answer" || r.Status != product.BlockStatusCancelled {
		t.Fatalf("stopped reply %+v", r)
	}
	// A late Stop for the finished execution does nothing harmful.
	if err := h.app.Runtime.CancelExecution(context.Background(), s.ID, execution); err == nil {
		t.Log("late Stop for a finished execution returned nil")
	}
	h.send(s.ID, "continue please")
	h.expect(s.ID, engine.Succeeded)
	next := model.request(2)
	if !next.has("long question") || !next.has("continue please") {
		t.Fatalf("the next Turn lost the stopped conversation: %s", next.Body)
	}
	t.Logf("partial answer included in the next model request: %v", next.has("partial answer"))
	if r := lastReply(t, h.blocks(s.ID)); r.Text != "second answer" {
		t.Fatalf("next reply %+v", r)
	}
}

// A failed provider request ends the Turn with a typed failure the user can
// act on. Only rate limits and startup failures may move to an explicitly
// approved fallback route; authentication failures never do.
func TestProviderFailuresAreTypedAndFallBackOnlyWhenRetryable(t *testing.T) {
	for _, tc := range []struct {
		name     string
		status   int
		failure  engine.FailureKind
		fallback bool
	}{
		{"authentication", 401, engine.FailureAuth, false},
		{"rate-limit", 429, engine.FailureRateLimit, true},
		{"unavailable", 503, engine.FailureStartup, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			primary := newFakeModel(t, provider.OpenAIChatKind, reply{Status: tc.status}, reply{Status: tc.status})
			backup := newFakeModel(t, provider.OpenAIChatKind)
			backup.next = func(modelRequest) reply { return text("answered by backup") }
			h := openHub(t, liveConfig(t, primary))
			s := h.create("failure", "")
			h.send(s.ID, "hello")
			h.expect(s.ID, engine.Failed)
			if primary.calls() != 1 {
				t.Fatalf("failed request repeated without a fallback: %d", primary.calls())
			}
			errs := errorEvents(h.history(s.ID))
			if len(errs) != 1 || errs[0].Failure != tc.failure {
				t.Fatalf("error events %+v, want failure %s", errs, tc.failure)
			}
			var status *product.Block
			for _, b := range blocksOf(h.blocks(s.ID), product.BlockKindStatus) {
				if b.Name == "error" {
					b := b
					status = &b
				}
			}
			if status == nil || !strings.Contains(status.Text, "HTTP") {
				t.Fatalf("Message Stream error block %+v", status)
			}
			// The user approves a fallback route on another connection.
			saved, err := h.service().Connections.Save(context.Background(), hub.ConnectionUpdate{Name: "backup", Protocol: "openai_chat", Endpoint: backup.endpoint(), Auth: "none", Enabled: true, Models: []hub.ConnectionModel{{ID: "backup-model", Enabled: true}}})
			if err != nil {
				t.Fatal(err)
			}
			target := saved.Target(saved.Models[0])
			target.AutomaticAuthorized = true
			if err := h.service().SetFallback(context.Background(), s.ID, []hub.Target{target}); err != nil {
				t.Fatal(err)
			}
			h.send(s.ID, "hello again")
			outcome := h.settle(s.ID)
			if tc.fallback {
				if outcome != engine.Succeeded || backup.calls() != 1 {
					t.Fatalf("retryable failure did not use the approved fallback: %s, backup calls %d: %s", outcome, backup.calls(), h.dump(s.ID))
				}
				if r := lastReply(t, h.blocks(s.ID)); r.Text != "answered by backup" {
					t.Fatalf("fallback reply %+v", r)
				}
			} else if outcome != engine.Failed || backup.calls() != 0 {
				t.Fatalf("authentication failure fell back: %s, backup calls %d", outcome, backup.calls())
			}
		})
	}
}

// TestCrashHelperProcess is the Hub process the crash scenario kills.
func TestCrashHelperProcess(t *testing.T) {
	directory := os.Getenv("ALVA_SCENARIO_CRASH_DIR")
	if directory == "" {
		t.Skip("helper process for TestCrashMidTurnRecoversAndTheUserCanContinue")
	}
	config := bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings,
		DataDirectory: directory, ProviderKind: provider.OpenAIChatKind,
		Provider: provider.Config{Endpoint: os.Getenv("ALVA_SCENARIO_CRASH_ENDPOINT"), Model: "scenario-model", APIKey: "scenario-key"}}
	h := openHub(t, config)
	h.send(os.Getenv("ALVA_SCENARIO_CRASH_SESSION"), "crash during this reply")
	<-t.Context().Done() // killed by the parent mid-stream
}

func TestCrashMidTurnRecoversAndTheUserCanContinue(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	defer close(hold)
	model := newFakeModel(t, provider.OpenAIChatKind,
		reply{Text: []string{"half of ", "the reply"}, Hold: hold, Reached: reached},
		text("recovered answer"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("crash", "")
	h.close()
	command := exec.CommandContext(t.Context(), os.Args[0], "-test.run=^TestCrashHelperProcess$", "-test.timeout=60s")
	command.Env = append(os.Environ(), "ALVA_SCENARIO_CRASH_DIR="+h.config.DataDirectory, "ALVA_SCENARIO_CRASH_ENDPOINT="+model.endpoint(), "ALVA_SCENARIO_CRASH_SESSION="+s.ID)
	var output bytes.Buffer
	command.Stdout, command.Stderr = &output, &output
	if err := command.Start(); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- command.Wait() }()
	select {
	case <-reached:
	case err := <-done:
		t.Fatalf("helper exited early: %v\n%s", err, output.String())
	case <-time.After(30 * time.Second):
		_ = command.Process.Kill()
		t.Fatal("helper never reached the model")
	}
	// Let the coalesced partial text reach the database, then crash.
	time.Sleep(600 * time.Millisecond)
	if err := command.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	var exit *exec.ExitError
	if err := <-done; !errors.As(err, &exit) {
		t.Fatalf("helper was not killed: %v\n%s", err, output.String())
	}
	h.open()
	d := h.detail(s.ID)
	if d.Session.Status == hub.Running || h.app.Runtime.SessionActive(s.ID) {
		t.Fatalf("session stays running after the crash: %s", h.dump(s.ID))
	}
	if d.Session.Status != engine.Interrupted {
		t.Fatalf("crashed Turn ended %s, want interrupted: %s", d.Session.Status, h.dump(s.ID))
	}
	r := lastReply(t, h.blocks(s.ID))
	if r.Status != product.BlockStatusInterrupted || !strings.HasPrefix("half of the reply", r.Text) || r.Text == "" {
		t.Fatalf("interrupted reply block %+v", r)
	}
	h.send(s.ID, "are you there?")
	h.expect(s.ID, engine.Succeeded)
	if model.calls() != 2 {
		t.Fatalf("model requests %d, want the crashed one and the new one", model.calls())
	}
	if r := lastReply(t, h.blocks(s.ID)); r.Text != "recovered answer" {
		t.Fatalf("reply after recovery %+v", r)
	}
}
