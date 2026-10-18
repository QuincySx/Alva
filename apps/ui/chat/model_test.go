// INPUT:  testing, errors, strings
// OUTPUT: headless presentation state transition regression tests
// POS: [L6 application]    Native foundation tests independent from MyGo and platform libraries
package chat

import (
	"errors"
	"strings"
	"testing"
)

func TestNoAutomaticSendAndFailedAdmissionPreservesDraft(t *testing.T) {
	m := NewModel(true)
	if m.CanSend() || m.ActiveRunID != "" || len(m.Messages) != 0 {
		t.Fatal("startup must not create work")
	}
	m.Draft = "检查日志🙂"
	prompt, err := m.Prompt()
	if err != nil || prompt != m.Draft {
		t.Fatal(prompt, err)
	}
	m.FailAdmission(errors.New("runtime unavailable"))
	if m.Draft != prompt || m.Error != "runtime unavailable" || len(m.Messages) != 0 {
		t.Fatal("failed admission lost the draft or invented a transcript")
	}
}

func TestStreamingCompletionAndStableRows(t *testing.T) {
	m := NewModel(true)
	m.Draft = "hello"
	m.Admitted("run-1", m.Draft)
	if m.CanSend() || m.Draft != "" || len(m.Messages) != 2 {
		t.Fatal("admission not reflected")
	}
	rowID := m.Messages[1].ID
	if !m.Observe("run-1", 2, "**stream", Running, "") || m.Messages[1].Text != "**stream" {
		t.Fatal("missing partial output")
	}
	if m.Observe("stale-run", 3, "wrong", Completed, "") || m.Observe("run-1", 1, "old", Running, "") {
		t.Fatal("accepted stale results")
	}
	m.Observe("run-1", 3, "**stream** complete", Completed, "")
	if m.ActiveRunID != "" || m.Status != Completed || m.Messages[1].ID != rowID {
		t.Fatal("completion lost row identity")
	}
	if m.Observe("run-1", 4, "late", Running, "") {
		t.Fatal("late result resurrected terminal run")
	}
}

func TestStopWaitsForRuntimeAndKeepsPartialText(t *testing.T) {
	m := NewModel(true)
	m.Admitted("run", "question")
	m.Observe("run", 1, "partial", Running, "")
	if m.RequestStop() != "run" || m.Status != Stopping || m.ActiveRunID != "run" || m.RequestStop() != "" {
		t.Fatal("stop must be explicit, idempotent, and await acknowledgement")
	}
	m.Observe("run", 2, "partial", Running, "")
	if m.Status != Stopping || m.RequestStop() != "" {
		t.Fatal("late running progress undid the pending Stop request")
	}
	m.Observe("run", 3, "partial", Cancelled, "")
	if m.ActiveRunID != "" || m.Messages[1].Text != "partial" || m.Messages[1].Status != Cancelled {
		t.Fatal("cancel discarded partial reply")
	}
}

func TestPromptLimitsAndBoundedPresentation(t *testing.T) {
	m := NewModel(false)
	for _, value := range []string{"  \n", strings.Repeat("x", MaxDraftBytes+1)} {
		m.Draft = value
		if m.CanSend() {
			t.Fatal("invalid message enabled Send")
		}
		if _, err := m.Prompt(); err == nil {
			t.Fatal("invalid message admitted")
		}
	}
	for i := 0; i < 100; i++ {
		m.Admitted(strings.Repeat("r", i+1), "prompt")
		m.Observe(m.ActiveRunID, 1, "answer", Completed, "")
	}
	if len(m.Messages) != 64 || m.Notice == "" || m.Messages[0].Role != "You" {
		t.Fatal("presentation retention must be bounded and explicit")
	}
}

func TestFailureReasonSurvivesTheNextRun(t *testing.T) {
	m := NewModel(true)
	m.Admitted("failed", "first")
	m.Observe("failed", 1, "partial", Failed, "provider connection failed")
	m.Admitted("next", "second")
	if m.Error != "" || m.Messages[1].Error != "provider connection failed" || m.Messages[1].Status != Failed {
		t.Fatal("new run erased the prior failure evidence")
	}
}

func TestPresentationRetentionAndReattachedObserver(t *testing.T) {
	m := NewModelWithRetention(true, 128)
	for i := range 100 {
		m.Admitted(strings.Repeat("r", i+1), "prompt")
		m.Observe(m.ActiveRunID, 20, "answer", Completed, "")
	}
	if len(m.Messages) != 128 || m.LastSequence() != 20 {
		t.Fatal("product retention or observed sequence was lost")
	}
	draft, rows := "unsent draft", len(m.Messages)
	m.Draft = draft
	m.AttachRun("reattached", Running)
	if !m.Observe("reattached", 1, "current snapshot", Running, "") || m.LastSequence() != 1 || m.Draft != draft || len(m.Messages) != rows {
		t.Fatal("reattaching must reset observation without admitting or replaying a prompt")
	}
	m.AttachRun("reattached", Completed)
	if m.ActiveRunID != "" || m.Observe("reattached", 2, "late", Running, "") {
		t.Fatal("a terminal attachment must reject late progress")
	}
}
