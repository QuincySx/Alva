// INPUT: context and bounded engine-owned human question DTOs
// OUTPUT: UserInputRequest, UserInputResponse and optional UserInputResponder
// POS: [L4.5 engine contract] Human answers remain separate from execution permission decisions
package engine

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"
)

type UserInputOption struct {
	Label       string `json:"label"`
	Description string `json:"description,omitempty"`
}

type UserInputQuestion struct {
	ID       string            `json:"id"`
	Header   string            `json:"header"`
	Question string            `json:"question"`
	Options  []UserInputOption `json:"options,omitempty"`
	IsOther  bool              `json:"is_other,omitempty"`
	IsSecret bool              `json:"is_secret,omitempty"`
}

type UserInputRequest struct {
	Questions []UserInputQuestion `json:"questions"`
}

type UserInputResponse struct {
	Answers map[string][]string `json:"answers"`
}

// UserInputResponder is optional: Runtime's permission response never answers a
// human question. Both IDs must identify the actual pending native request.
type UserInputResponder interface {
	RespondUserInput(context.Context, string, string, UserInputResponse) error
}

func boundedQuestionText(value string, limit int, required bool) bool {
	return len(value) <= limit && utf8.ValidString(value) && !strings.ContainsRune(value, '\x00') && (!required || strings.TrimSpace(value) != "")
}

func (r UserInputRequest) Validate() error {
	if len(r.Questions) < 1 || len(r.Questions) > 3 {
		return errors.New("human input requires 1..3 questions")
	}
	ids := make(map[string]bool)
	for _, q := range r.Questions {
		if !boundedQuestionText(q.ID, 128, true) || ids[q.ID] || !boundedQuestionText(q.Header, 256, false) || !boundedQuestionText(q.Question, 4096, true) || len(q.Options) > 16 {
			return errors.New("invalid bounded human question")
		}
		ids[q.ID] = true
		labels := make(map[string]bool)
		for _, option := range q.Options {
			if !boundedQuestionText(option.Label, 512, true) || labels[option.Label] || !boundedQuestionText(option.Description, 2048, false) {
				return errors.New("invalid bounded human question option")
			}
			labels[option.Label] = true
		}
	}
	data, err := json.Marshal(r)
	if err != nil || len(data) > 16<<10 {
		return errors.New("human question exceeds event budget")
	}
	return nil
}

// ValidateFor rejects foreign question IDs, oversized answers and choices that
// the question did not offer. An empty map explicitly skips the request.
func (r UserInputResponse) ValidateFor(request UserInputRequest) error {
	if err := request.Validate(); err != nil {
		return err
	}
	questions := make(map[string]UserInputQuestion)
	for _, q := range request.Questions {
		questions[q.ID] = q
	}
	for id, answers := range r.Answers {
		q, ok := questions[id]
		if !ok || len(answers) > 16 {
			return errors.New("answer does not identify a pending question")
		}
		for _, answer := range answers {
			if !boundedQuestionText(answer, 4096, true) {
				return errors.New("invalid bounded human answer")
			}
			if len(q.Options) > 0 && !q.IsOther {
				found := false
				for _, option := range q.Options {
					found = found || answer == option.Label
				}
				if !found {
					return errors.New("answer is not an offered choice")
				}
			}
		}
	}
	data, err := json.Marshal(r)
	if err != nil || len(data) > 16<<10 {
		return errors.New("human answer exceeds response budget")
	}
	return nil
}
