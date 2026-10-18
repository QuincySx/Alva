// INPUT: Actual active product branch, bounded question broker and verified typed native ancestry
// OUTPUT: PendingQuestions and RespondQuestion without client-selected native identity
// POS: [L7 Hub product] Human clarification routing remains independent from permission decisions.
package product

import (
	"context"
	"sort"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/extension/interaction"
	"github.com/QuincySx/Alva/go/platform/host"
)

func (s *HubService) PendingQuestions(ctx context.Context, id string) ([]interaction.Question, error) {
	if s.Questions == nil {
		return nil, nil
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	if detail.Session.Status != hub.Running {
		return nil, nil
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return nil, err
	}
	if branch.Target.EngineID != "alva" || branch.NativeSessionID == "" {
		return nil, nil
	}
	var out []interaction.Question
	for _, q := range s.Questions.PendingQuestionsSnapshot() {
		allowed, err := s.typedDescendant(ctx, id, branch.NativeSessionID, q.SessionID)
		if err != nil {
			return nil, err
		}
		if allowed {
			out = append(out, q)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].ID < out[j].ID })
	return out, nil
}
func (s *HubService) RespondQuestion(ctx context.Context, id, questionID string, answer interaction.Answer) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	questions, err := s.PendingQuestions(ctx, id)
	if err != nil {
		return err
	}
	for _, q := range questions {
		if q.ID == questionID {
			return s.Questions.RespondQuestion(q.SessionID, q.ID, answer)
		}
	}
	return host.ErrQuestionExpired
}
