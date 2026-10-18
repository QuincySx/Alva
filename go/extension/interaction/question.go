// INPUT: encoding/json, errors, strings, unicode/utf8, context.
// OUTPUT: Question/Choice/Answer/ResolvedAnswer, Handler and strict bounded question/answer validation.
// POS: [L4 extension] Human clarification is separate from permission approval and never grants execution authority.
package interaction

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"
)

type Choice struct {
	Label string  `json:"label"`
	Value *string `json:"value,omitempty"`
}
type Question struct {
	ID, SessionID, RunID, CallID string
	Prompt                       string          `json:"question"`
	Header                       string          `json:"header,omitempty"`
	Options                      []Choice        `json:"options,omitempty"`
	MultiSelect                  bool            `json:"multi_select"`
	Metadata                     json.RawMessage `json:"metadata,omitempty"`
}
type Answer struct {
	Text     *string `json:"text,omitempty"`
	Selected []int   `json:"selected,omitempty"`
}
type ResolvedAnswer struct {
	Text       string `json:"text"`
	Selected   []int  `json:"selected,omitempty"`
	Structured bool   `json:"structured"`
}
type Handler interface {
	Ask(context.Context, Question) (Answer, error)
}
type HandlerFunc func(context.Context, Question) (Answer, error)

func (f HandlerFunc) Ask(ctx context.Context, q Question) (Answer, error) { return f(ctx, q) }
func small(s string, n int) bool {
	return utf8.ValidString(s) && len(s) <= n && !strings.ContainsRune(s, 0)
}
func (q Question) Validate() error {
	for _, id := range []string{q.ID, q.SessionID, q.RunID, q.CallID} {
		if id == "" || !small(id, 512) {
			return errors.New("actual bounded question identity required")
		}
	}
	if q.Prompt == "" || !small(q.Prompt, 8192) || !small(q.Header, 1024) || len(q.Metadata) > 4096 {
		return errors.New("bounded question/header/metadata required")
	}
	if len(q.Options) != 0 && (len(q.Options) < 2 || len(q.Options) > 4) {
		return errors.New("question options must contain 2..4 choices")
	}
	if q.MultiSelect && len(q.Options) == 0 {
		return errors.New("multi_select requires explicit options")
	}
	for _, c := range q.Options {
		if c.Label == "" || !small(c.Label, 512) || c.Value != nil && !small(*c.Value, 1024) {
			return errors.New("bounded choice label/value required")
		}
	}
	if len(q.Metadata) > 0 {
		var object map[string]json.RawMessage
		if json.Unmarshal(q.Metadata, &object) != nil || object == nil {
			return errors.New("question metadata requires JSON object")
		}
	}
	return nil
}
func (q Question) Resolve(a Answer) (ResolvedAnswer, error) {
	if err := q.Validate(); err != nil {
		return ResolvedAnswer{}, err
	}
	if (a.Text == nil) == (len(a.Selected) == 0) {
		return ResolvedAnswer{}, errors.New("answer requires explicit text OR selections, never both")
	}
	if a.Text != nil {
		if !small(*a.Text, 64<<10) {
			return ResolvedAnswer{}, errors.New("human answer exceeds 64 KiB UTF-8")
		}
		return ResolvedAnswer{Text: *a.Text}, nil
	}
	if len(q.Options) == 0 || len(a.Selected) > len(q.Options) || !q.MultiSelect && len(a.Selected) != 1 {
		return ResolvedAnswer{}, errors.New("invalid choice count")
	}
	seen := map[int]bool{}
	var values []string
	for _, index := range a.Selected {
		if index < 1 || index > len(q.Options) || seen[index] {
			return ResolvedAnswer{}, errors.New("invalid/duplicate 1-based choice index")
		}
		seen[index] = true
		c := q.Options[index-1]
		value := c.Label
		if c.Value != nil {
			value = *c.Value
		}
		values = append(values, value)
	}
	return ResolvedAnswer{Text: strings.Join(values, ", "), Selected: append([]int(nil), a.Selected...), Structured: true}, nil
}
func CloneQuestion(q Question) Question {
	data, _ := json.Marshal(q)
	var copy Question
	json.Unmarshal(data, &copy)
	return copy
}
