// INPUT: Product-bound native engine interaction snapshots and explicit Foundation controls
// OUTPUT: Shared permission/question panels, masked answer drafts and generation-bound asynchronous responses
// POS: [L7 Hub presentation] Build performs no I/O; questions never grant tool permission and late clicks keep their original owner
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type workspaceEngineQuestionDraft struct {
	selected map[string]string
	other    map[string]bool
	text     map[string]*string
}

type workspaceEngineInteractionsState struct {
	session, branch  string
	generation       uint64
	epoch            uint64
	polling, current bool
	problem          string
	items            []product.EngineInteraction
	drafts           map[string]*workspaceEngineQuestionDraft
	submitting       map[string]bool
}

func (p *NativePage) engineInteractionScope() (string, string, uint64) {
	return p.selected, p.detail.Session.ActiveBranchID, p.generation
}

func (p *NativePage) prepareEngineInteractionScope() *workspaceEngineInteractionsState {
	id, branch, generation := p.engineInteractionScope()
	v := &p.engineInteractions
	if v.session != id || v.branch != branch || v.generation != generation {
		*v = workspaceEngineInteractionsState{session: id, branch: branch, generation: generation, epoch: v.epoch + 1, drafts: make(map[string]*workspaceEngineQuestionDraft), submitting: make(map[string]bool)}
	}
	return v
}

func (p *NativePage) pollEngineInteractions() {
	v := p.prepareEngineInteractionScope()
	activeBranch, err := product.ActiveBranch(p.detail)
	if err != nil || activeBranch.Target.EngineID == "alva" {
		// Alva retains its independent broker reads. Native interaction polling
		// must not consume their I/O failures or reinterpret their identities.
		v.items, v.problem, v.current = nil, "", true
		return
	}
	if v.session == "" || v.branch == "" || v.polling || len(v.submitting) > 0 || p.Service == nil {
		return
	}
	id, branch, generation, epoch := v.session, v.branch, v.generation, v.epoch
	v.polling = true
	go func() {
		items, err := p.Service.PendingEngineInteractions(p.ctx, id)
		p.dispatch(func() {
			v := p.prepareEngineInteractionScope()
			if v.session != id || v.branch != branch || v.generation != generation || v.epoch != epoch {
				return
			}
			v.polling = false
			if err != nil {
				v.current, v.problem = false, "Native requests could not be refreshed: "+err.Error()
				return
			}
			for _, item := range items {
				if item.BranchID != branch {
					v.current = false
					return
				}
			}
			v.current, v.problem, v.items = true, "", items
			keep := make(map[string]bool)
			for _, item := range items {
				keep[item.Handle.ID()] = true
			}
			for key := range v.drafts {
				if !keep[key] {
					delete(v.drafts, key)
				}
			}
		})
	}()
}

func (p *NativePage) engineQuestionDraft(item product.EngineInteraction) *workspaceEngineQuestionDraft {
	v := p.prepareEngineInteractionScope()
	key := item.Handle.ID()
	if v.drafts == nil {
		v.drafts = make(map[string]*workspaceEngineQuestionDraft)
	}
	if v.drafts[key] == nil {
		v.drafts[key] = &workspaceEngineQuestionDraft{selected: make(map[string]string), other: make(map[string]bool), text: make(map[string]*string)}
	}
	return v.drafts[key]
}

func (d *workspaceEngineQuestionDraft) response(request engine.UserInputRequest) (engine.UserInputResponse, error) {
	response := engine.UserInputResponse{Answers: make(map[string][]string)}
	for _, q := range request.Questions {
		answer := d.selected[q.ID]
		if d.other[q.ID] || len(q.Options) == 0 {
			answer = ""
			if text := d.text[q.ID]; text != nil {
				answer = *text
			}
		}
		if strings.TrimSpace(answer) == "" {
			return response, fmt.Errorf("Answer %s before sending", q.Header)
		}
		response.Answers[q.ID] = []string{answer}
	}
	return response, response.ValidateFor(request)
}

func (p *NativePage) submitEngineInteraction(item product.EngineInteraction, allow bool, answer *engine.UserInputResponse) {
	v := p.prepareEngineInteractionScope()
	key := item.Handle.ID()
	if !v.current || v.session == "" || v.submitting[key] {
		return
	}
	found := false
	for _, current := range v.items {
		found = found || current.Handle == item.Handle
	}
	if !found {
		return
	}
	if v.submitting == nil {
		v.submitting = make(map[string]bool)
	}
	v.submitting[key], v.polling = true, false
	v.epoch++
	id, branch, generation := v.session, v.branch, v.generation
	go func() {
		var err error
		if answer == nil {
			err = p.Service.RespondEnginePermission(p.ctx, id, item.Handle, allow)
		} else {
			err = p.Service.RespondEngineUserInput(p.ctx, id, item.Handle, *answer)
		}
		p.dispatch(func() {
			v := p.prepareEngineInteractionScope()
			if v.session != id || v.branch != branch || v.generation != generation {
				return
			}
			delete(v.submitting, key)
			v.epoch++
			v.polling = false
			if err != nil {
				v.problem = err.Error()
				return
			}
			for i, current := range v.items {
				if current.Handle == item.Handle {
					v.items = append(v.items[:i], v.items[i+1:]...)
					break
				}
			}
			delete(v.drafts, key)
			v.problem = ""
			p.pollEngineInteractions()
		})
	}()
}

func (p *NativePage) buildEnginePermissions(c *ui.Context) {
	v := p.prepareEngineInteractionScope()
	if v.problem != "" {
		ui.Text(c, v.problem).Role(ui.RoleStatus).TextColor(c.Theme().Danger).FontSize(workspaceFontCaption)
	}
	for _, item := range v.items {
		if item.Event.Kind != engine.PermissionRequest {
			continue
		}
		item := item
		key := item.Handle.ID()
		busy := v.submitting[key]
		ui.Column(c.Key("native-permission-" + key)).Gap(4).Children(func() {
			ui.Text(c, "Native permission: "+item.EngineID+" · "+item.Event.Name).Bold()
			ui.Scroll(c.Key("native-permission-review")).Label("Native permission request details").MaxHeight(180).Shrink(0).Gap(4).Children(func() {
				if item.Event.Text != "" {
					ui.Text(c, item.Event.Text).FontSize(workspaceFontCaption)
				}
				summary := string(item.Event.Input)
				if summary == "" && item.Event.Reference != nil {
					summary = "Complete request: " + item.Event.Reference.URI
				}
				workspaceControlTextArea(c, "native-permission-input", &summary).Label("Native requested input").ReadOnly(true).Height(90)
			})
			ui.Row(c).Gap(8).Children(func() {
				p.workspaceControlAction(c, "native-permission-allow", "Allow native once", "", "primary", busy, func() { p.submitEngineInteraction(item, true, nil) }, !v.current)
				p.workspaceControlAction(c, "native-permission-deny", "Deny native once", "", "secondary", busy, func() { p.submitEngineInteraction(item, false, nil) }, !v.current)
			})
		})
		break
	}
}

func (p *NativePage) buildEngineQuestions(c *ui.Context) {
	v := p.prepareEngineInteractionScope()
	for _, item := range v.items {
		if item.Event.Kind != engine.UserInputRequested || item.Event.Question == nil {
			continue
		}
		item := item
		key := item.Handle.ID()
		draft := p.engineQuestionDraft(item)
		busy := v.submitting[key]
		enabled := v.current && !busy
		ui.Scroll(c.Key("native-questions-" + key)).Label("Native human questions").Height(280).Gap(6).Children(func() {
			ui.Text(c, "Native questions · "+item.EngineID).Bold()
			ui.Text(c, "Answers clarify the task. Tool permissions use separate requests.").FontSize(11)
			for _, question := range item.Event.Question.Questions {
				q := question
				ui.Column(c.Key("native-question-" + q.ID)).Gap(4).Children(func() {
					ui.Text(c, q.Header).Bold()
					ui.Text(c, q.Question).FontSize(workspaceFontCaption)
					for index, option := range q.Options {
						option := option
						label := option.Label
						if draft.selected[q.ID] == option.Label && !draft.other[q.ID] {
							label = "✓ " + label
						}
						workspaceControlButton(c, fmt.Sprintf("choice-%d", index), label, "secondary", !enabled).OnClick(func() { draft.selected[q.ID] = option.Label; draft.other[q.ID] = false })
						if option.Description != "" {
							ui.Text(c, option.Description).FontSize(11)
						}
					}
					if q.IsOther && len(q.Options) > 0 {
						label := "Other · " + q.Header
						if draft.other[q.ID] {
							label = "✓ " + label
						}
						workspaceControlButton(c, "choose-other", label, "secondary", !enabled).OnClick(func() { draft.other[q.ID] = true })
					}
					if q.IsOther || len(q.Options) == 0 {
						if draft.text[q.ID] == nil {
							text := ""
							draft.text[q.ID] = &text
						}
						input := workspaceControlInput(c, "answer", draft.text[q.ID]).Label("Native answer · " + q.Header).Disabled(!enabled)
						if q.IsSecret {
							input.Password()
						}
					}
				})
			}
			response, err := draft.response(*item.Event.Question)
			ui.Row(c).Gap(8).Children(func() {
				p.workspaceControlAction(c, "send-native-answers", "Send native answers", "", "primary", busy, func() { p.submitEngineInteraction(item, false, &response) }, !enabled || err != nil)
				p.workspaceControlAction(c, "skip-native-questions", "Skip native questions", "", "secondary", busy, func() {
					empty := engine.UserInputResponse{Answers: map[string][]string{}}
					p.submitEngineInteraction(item, false, &empty)
				}, !v.current)
			})
		})
		break
	}
}
