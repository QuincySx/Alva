// INPUT: Human question/answer DTOs with bounded IDs, choices and text
// OUTPUT: Foreign answer, unavailable choice and event-budget regressions
// POS: [L4.5 engine contract tests] Human answers grant no tool execution permission
package engine

import (
	"strings"
	"testing"
)

func TestHumanQuestionBoundsAndAnswerIdentity(t *testing.T) {
	request := UserInputRequest{Questions: []UserInputQuestion{{ID: "color", Question: "Pick", Options: []UserInputOption{{Label: "Blue"}}}}}
	if err := request.Validate(); err != nil {
		t.Fatal(err)
	}
	if err := (UserInputResponse{Answers: map[string][]string{"color": {"Blue"}}}).ValidateFor(request); err != nil {
		t.Fatal(err)
	}
	for _, response := range []UserInputResponse{{Answers: map[string][]string{"foreign": {"Blue"}}}, {Answers: map[string][]string{"color": {"Red"}}}, {Answers: map[string][]string{"color": {strings.Repeat("x", 4097)}}}} {
		if response.ValidateFor(request) == nil {
			t.Fatal("invalid human answer accepted")
		}
	}
	request.Questions[0].IsOther = true
	if err := (UserInputResponse{Answers: map[string][]string{"color": {"custom"}}}).ValidateFor(request); err != nil {
		t.Fatal(err)
	}
	request.Questions = append(request.Questions, request.Questions[0])
	if request.Validate() == nil {
		t.Fatal("duplicate question identity accepted")
	}
}
