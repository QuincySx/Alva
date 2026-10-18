// INPUT: context, errors, testing/time, interaction contracts.
// OUTPUT: Actual broker response/identity/cancel/late-answer/clone/capacity and shutdown regressions distinct from approvals.
// POS: [platform / native execution host] Human-question queues have bounded ownership and cannot be reused as permission decisions.
package host

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/extension/interaction"
	"testing"
	"time"
)

func questionFixture(id string) interaction.Question {
	return interaction.Question{ID: id, SessionID: "native", RunID: "run", CallID: "call", Prompt: "actual question", Options: []interaction.Choice{{Label: "A"}, {Label: "B"}}}
}
func awaitQuestion(t *testing.T, b *QuestionBroker) interaction.Question {
	t.Helper()
	timeout := time.NewTimer(time.Second)
	defer timeout.Stop()
	tick := time.NewTicker(time.Millisecond)
	defer tick.Stop()
	for {
		pending := b.PendingQuestionsSnapshot()
		if len(pending) > 0 {
			return pending[0]
		}
		select {
		case <-timeout.C:
			t.Fatal("question not queued")
		case <-tick.C:
		}
	}
}
func TestQuestionBrokerRealResponseIdentityAndDetachedSnapshots(t *testing.T) {
	b, _ := NewQuestionBroker(2)
	defer b.Close()
	done := make(chan error, 1)
	go func() {
		answer, err := b.Ask(context.Background(), questionFixture("question"))
		if err == nil {
			resolved, e := questionFixture("question").Resolve(answer)
			err = e
			if resolved.Text != "B" {
				err = errors.New("wrong actual choice")
			}
		}
		done <- err
	}()
	q := awaitQuestion(t, b)
	q.Options[0].Label = "mutated"
	if b.PendingQuestionsSnapshot()[0].Options[0].Label != "A" {
		t.Fatal("snapshot mutates broker")
	}
	if err := b.RespondQuestion("foreign", q.ID, interaction.Answer{Selected: []int{2}}); !errors.Is(err, ErrQuestionExpired) {
		t.Fatal("foreign answer accepted")
	}
	if err := b.RespondQuestion("native", q.ID, interaction.Answer{Selected: []int{5}}); err == nil {
		t.Fatal("invalid choice accepted")
	}
	if err := b.RespondQuestion("native", q.ID, interaction.Answer{Selected: []int{2}}); err != nil {
		t.Fatal(err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if err := b.RespondQuestion("native", q.ID, interaction.Answer{Selected: []int{1}}); !errors.Is(err, ErrQuestionExpired) {
		t.Fatal("replayed answer accepted")
	}
}
func TestQuestionBrokerCanceledOldRunCapacityAndClose(t *testing.T) {
	b, _ := NewQuestionBroker(1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { _, err := b.Ask(ctx, questionFixture("old")); done <- err }()
	awaitQuestion(t, b)
	if _, err := b.Ask(context.Background(), questionFixture("second")); err == nil {
		t.Fatal("pending capacity ignored")
	}
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if err := b.RespondQuestion("native", "old", interaction.Answer{Selected: []int{1}}); !errors.Is(err, ErrQuestionExpired) {
		t.Fatal("canceled late answer accepted")
	}
	go func() { _, err := b.Ask(context.Background(), questionFixture("new")); done <- err }()
	awaitQuestion(t, b)
	b.Close()
	if err := <-done; !errors.Is(err, ErrQuestionExpired) {
		t.Fatal(err)
	}
	if len(b.PendingQuestionsSnapshot()) != 0 {
		t.Fatal("pending question leaked")
	}
}
