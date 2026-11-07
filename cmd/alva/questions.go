// INPUT: Explicit sole-reader REPL input and the existing interaction Question/Answer contract.
// OUTPUT: Serialized bounded single/multiple choice and Other/free-text clarification answers.
// POS: [L6 application] Renders human questions at the terminal without turning answers into permissions.
package main

import (
	"context"
	"fmt"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/extension/interaction"
)

// Both human questions and approvals hold this gate through the complete read,
// including Other's second line. No competing reader or background input task.
func (r *directREPL) acquireInput(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	select {
	case r.gate <- struct{}{}:
		if err := ctx.Err(); err != nil {
			<-r.gate
			return err
		}
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (r *directREPL) Ask(ctx context.Context, question interaction.Question) (interaction.Answer, error) {
	if err := question.Validate(); err != nil {
		return interaction.Answer{}, err
	}
	question = interaction.CloneQuestion(question)
	if err := r.acquireInput(ctx); err != nil {
		return interaction.Answer{}, err
	}
	defer func() { <-r.gate }()
	var display strings.Builder
	fmt.Fprintf(&display, "Question %s", question.ID)
	if question.Header != "" {
		fmt.Fprintf(&display, " · %s", question.Header)
	}
	fmt.Fprintf(&display, "\n%s\n", question.Prompt)
	for index, choice := range question.Options {
		fmt.Fprintf(&display, "%d) %s\n", index+1, choice.Label)
	}
	if len(question.Options) > 0 {
		fmt.Fprintf(&display, "%d) Other — type a custom answer\n", len(question.Options)+1)
	}
	display.WriteString("Clarification only; this does not approve any tool action.\n")
	if err := writeString(r.diagnostic, display.String()); err != nil {
		return interaction.Answer{}, err
	}
	for {
		prompt := "Answer (free text): "
		if len(question.Options) > 0 {
			prompt = "Answer (one option number, or Other): "
			if question.MultiSelect {
				prompt = "Answer (comma-separated option numbers, or Other alone): "
			}
		}
		if err := writeString(r.diagnostic, prompt); err != nil {
			return interaction.Answer{}, err
		}
		line, err := r.line(ctx)
		if ctx.Err() != nil {
			return interaction.Answer{}, ctx.Err()
		}
		if err != nil {
			return interaction.Answer{}, err
		}
		var answer interaction.Answer
		if len(question.Options) == 0 {
			answer.Text = &line
		} else if strings.EqualFold(strings.TrimSpace(line), "other") || strings.TrimSpace(line) == strconv.Itoa(len(question.Options)+1) {
			if err := writeString(r.diagnostic, "Other text (clarification only): "); err != nil {
				return interaction.Answer{}, err
			}
			text, err := r.line(ctx)
			if ctx.Err() != nil {
				return interaction.Answer{}, ctx.Err()
			}
			if err != nil {
				return interaction.Answer{}, err
			}
			answer.Text = &text
		} else {
			for _, selected := range strings.Split(line, ",") {
				n, err := strconv.Atoi(strings.TrimSpace(selected))
				if err != nil {
					answer.Selected = nil
					break
				}
				answer.Selected = append(answer.Selected, n)
			}
		}
		if _, err := question.Resolve(answer); err == nil && (answer.Text == nil || strings.TrimSpace(*answer.Text) != "") {
			return answer, nil
		}
		if err := writeString(r.diagnostic, "Invalid answer; choose valid options or Other, or enter nonempty text. No answer was submitted.\n"); err != nil {
			return interaction.Answer{}, err
		}
	}
}

var _ interaction.Handler = (*directREPL)(nil)
