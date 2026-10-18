// INPUT: context, errors, sync, extension/interaction.
// OUTPUT: QuestionBroker, NewQuestionBroker and bounded PendingQuestionsSnapshot/RespondQuestion/Close independent of approval.
// POS: [platform / native execution host] Host-owned human-question lifecycle exposes actual session IDs and rejects canceled or stale responses.
package host

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/extension/interaction"
	"sync"
)

var ErrQuestionExpired = errors.New("question is no longer pending for this actual session")

type questionReply struct {
	answer interaction.Answer
	err    error
}
type pendingQuestion struct {
	ctx      context.Context
	question interaction.Question
	reply    chan questionReply
}
type QuestionBroker struct {
	mu      sync.Mutex
	maximum int
	pending map[string]*pendingQuestion
	closed  bool
}

func NewQuestionBroker(maximum int) (*QuestionBroker, error) {
	if maximum < 1 || maximum > 128 {
		return nil, errors.New("question budget must be 1..128")
	}
	return &QuestionBroker{maximum: maximum, pending: map[string]*pendingQuestion{}}, nil
}
func (b *QuestionBroker) Ask(ctx context.Context, q interaction.Question) (interaction.Answer, error) {
	if err := q.Validate(); err != nil {
		return interaction.Answer{}, err
	}
	if err := ctx.Err(); err != nil {
		return interaction.Answer{}, err
	}
	q = interaction.CloneQuestion(q)
	p := &pendingQuestion{ctx: ctx, question: q, reply: make(chan questionReply, 1)}
	b.mu.Lock()
	if b.closed || len(b.pending) >= b.maximum || b.pending[q.ID] != nil {
		b.mu.Unlock()
		return interaction.Answer{}, errors.New("question broker closed, duplicate or at capacity")
	}
	b.pending[q.ID] = p
	b.mu.Unlock()
	defer func() {
		b.mu.Lock()
		if b.pending[q.ID] == p {
			delete(b.pending, q.ID)
		}
		b.mu.Unlock()
	}()
	select {
	case result := <-p.reply:
		if err := ctx.Err(); err != nil {
			return interaction.Answer{}, err
		}
		return result.answer, result.err
	case <-ctx.Done():
		return interaction.Answer{}, ctx.Err()
	}
}

// Only trusted host assembly may enumerate across sessions. Product consumers
// must filter a claimed root and validate typed child ancestry before showing
// or responding to an actual child session, not accept an arbitrary UI ID.
func (b *QuestionBroker) PendingQuestionsSnapshot() []interaction.Question {
	b.mu.Lock()
	defer b.mu.Unlock()
	out := make([]interaction.Question, 0, len(b.pending))
	for _, p := range b.pending {
		if p.ctx.Err() == nil {
			out = append(out, interaction.CloneQuestion(p.question))
		}
	}
	return out
}
func (b *QuestionBroker) RespondQuestion(sessionID, questionID string, answer interaction.Answer) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	p := b.pending[questionID]
	if b.closed || p == nil || p.question.SessionID != sessionID || p.ctx.Err() != nil {
		return ErrQuestionExpired
	}
	if _, err := p.question.Resolve(answer); err != nil {
		return err
	}
	copy := interaction.Answer{Selected: append([]int(nil), answer.Selected...)}
	if answer.Text != nil {
		text := *answer.Text
		copy.Text = &text
	}
	delete(b.pending, questionID)
	p.reply <- questionReply{answer: copy}
	return nil
}
func (b *QuestionBroker) Close() {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.closed = true
	for id, p := range b.pending {
		delete(b.pending, id)
		p.reply <- questionReply{err: ErrQuestionExpired}
	}
}
