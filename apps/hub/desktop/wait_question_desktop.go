// INPUT: Stored durable question/binding, interaction Answer validation and explicit MyGo input.
// OUTPUT: Single/multiple choice and text editors that save an exact answer without continuing execution.
// POS: [L7 presentation] Keeps drafts in the existing bound wait pane; adds no question or permission protocol.
//go:build desktop

package hubdesktop

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) buildWaitQuestion(c *ui.Context, request durable.Request, enabled bool) {
	var question interaction.Question
	if json.Unmarshal(request.Payload, &question) != nil || question.Validate() != nil {
		ui.Text(c, "Stored question cannot be validated. Review its complete request below.")
		return
	}
	ui.Text(c, "Clarification: "+question.Header).Bold()
	ui.Text(c, question.Prompt)
	ui.Text(c, "Answers clarify the task. File, shell and network permissions use separate requests. Saving keeps execution suspended until Continue.").FontSize(workspaceFontCaption)
	if len(request.Result) != 0 {
		var answer interaction.Answer
		if json.Unmarshal(request.Result, &answer) == nil {
			if resolved, err := question.Resolve(answer); err == nil {
				workspaceControlTextArea(c, "saved-answer", &resolved.Text).Label("Saved clarification answer").ReadOnly(true).Height(80)
			}
		}
		return
	}
	var answer interaction.Answer
	if draft := p.wait.Answers[request.ID]; draft != "" && json.Unmarshal([]byte(draft), &answer) != nil {
		ui.Text(c, "Answer draft cannot be decoded. Reopen this session and review the request.")
		return
	}
	setDraft := func(value interaction.Answer) {
		data, _ := json.Marshal(value)
		p.wait.Answers[request.ID] = string(data)
	}
	for i, choice := range question.Options {
		index := i + 1
		label := fmt.Sprintf("%d. %s", index, choice.Label)
		if slices.Contains(answer.Selected, index) {
			label = "✓ " + label
		}
		workspaceControlButton(c, fmt.Sprintf("choice-%d", index), label, "secondary", !enabled).OnClick(func() {
			selected := []int{index}
			if question.MultiSelect {
				selected = nil
				for next := 1; next <= len(question.Options); next++ {
					keep := slices.Contains(answer.Selected, next)
					if next == index {
						keep = !keep
					}
					if keep {
						selected = append(selected, next)
					}
				}
			}
			setDraft(interaction.Answer{Selected: selected})
		})
	}
	text := ""
	if answer.Text != nil {
		text = *answer.Text
	}
	label := "Clarification answer"
	if question.Header != "" {
		label += " · " + question.Header
	}
	workspaceControlTextArea(c, "answer-text", &text).Label(label).Height(80).Disabled(!enabled).OnChange(func() {
		setDraft(interaction.Answer{Text: &text})
	})
	data := json.RawMessage(p.wait.Answers[request.ID])
	_, err := question.Resolve(answer)
	if len(data) > 16<<10 {
		ui.Text(c, "Answer exceeds the 16 KiB saved JSON limit.").FontSize(workspaceFontCaption)
	}
	blank := answer.Text != nil && strings.TrimSpace(*answer.Text) == ""
	if blank {
		ui.Text(c, "Answer cannot be blank.").FontSize(workspaceFontCaption)
	}
	workspaceControlButton(c, "save-clarification", "Save clarification without executing", "primary", !enabled || err != nil || blank || len(data) == 0 || len(data) > 16<<10).OnClick(func() {
		p.resolveWait(request.ID, request.Binding, data)
	})
}
