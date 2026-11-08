// INPUT: Product InteractionView blocks, pending Interaction items bound by product.BindInteractions, the existing approval/question/native panel states and HubService.RespondInteraction
// OUTPUT: In-stream approval/question cards (waiting with in-card actions, settled as one line), read problems with retry, usage lines, the pending-request jump and the other views' one-line entry
// POS: [L7 Hub UI] Renders interaction and usage blocks by type only; a card is another entry to the same product responses and drops late receipts after a session or branch change
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

// workspaceInteractionDraft is a question card's unsent answer: chosen
// options by index (a shown label may be clipped) and typed text.
type workspaceInteractionDraft struct {
	choices map[string][]int
	text    map[string]*string
}

// workspaceInteractionRead names the pending read a card's request came
// from, so its draft or error outlives a failed read of that path.
type workspaceInteractionRead uint8

const (
	readApprovals workspaceInteractionRead = iota + 1
	readQuestions
	readNative
)

func interactionRead(item product.Interaction) workspaceInteractionRead {
	if _, ok := item.Approval(); ok {
		return readApprovals
	}
	if _, ok := item.Question(); ok {
		return readQuestions
	}
	return readNative
}

// workspaceInteractionCards is the conversation's card state for one session
// and branch: the actionable request of each waiting block, drafts, in-flight
// responses and errors by request. A session or branch change resets it, so
// a late receipt lands nowhere.
type workspaceInteractionCards struct {
	scope   workspaceToolScope
	epoch   uint64
	actions map[string]product.Interaction
	// first is the row of the first actionable card, set with the rows;
	// pending is the request set those rows were bound to.
	first   int
	pending string
	drafts  map[string]*workspaceInteractionDraft
	busy    map[string]bool
	problem map[string]string
	// reads remembers which pending read each draft or error belongs to.
	reads map[string]workspaceInteractionRead
}

func (p *NativePage) syncInteractionCards() *workspaceInteractionCards {
	current := p.toolScope()
	if p.interactionCards.actions == nil || p.interactionCards.scope != current {
		p.interactionCards = workspaceInteractionCards{scope: current, epoch: p.interactionCards.epoch + 1, actions: map[string]product.Interaction{}, drafts: map[string]*workspaceInteractionDraft{}, busy: map[string]bool{}, problem: map[string]string{}, reads: map[string]workspaceInteractionRead{}}
	}
	return &p.interactionCards
}

// pendingInteractions combines the existing pending reads of the selected
// session; native requests count only while their observation is current.
func (p *NativePage) pendingInteractions() []product.Interaction {
	if p.selected == "" {
		return nil
	}
	v := p.prepareEngineInteractionScope()
	var native []product.EngineInteraction
	if v.current && v.session == p.selected {
		native = v.items
	}
	return product.CombineInteractions(p.approvals, p.questions, native)
}

// pendingSignature names the current pending request set; rows are bound
// again only when it changes.
func (p *NativePage) pendingSignature() string {
	var keys []string
	for _, item := range p.pendingInteractions() {
		keys = append(keys, item.Key())
	}
	return strings.Join(keys, "\x00")
}

// projectInteractionsIfChanged binds rows again after a pending read only
// when the request set it observed differs from the one already bound.
func (p *NativePage) projectInteractionsIfChanged() {
	if p.pendingSignature() != p.syncInteractionCards().pending {
		p.projectTranscript()
	}
}

// readCurrent reports that a path's last pending read succeeded, so a
// request missing from it is really gone.
func (p *NativePage) readCurrent(read workspaceInteractionRead) bool {
	switch read {
	case readApprovals:
		return p.approvalsCurrent
	case readQuestions:
		return p.questionsCurrent
	}
	v := p.prepareEngineInteractionScope()
	return v.current && v.session == p.selected
}

// bindInteractionBlocks places the pending requests among the window's blocks
// and keeps the actionable request of each waiting card. A draft (secret
// input included) or error is dropped only when its own path's read
// succeeded without the request, never because a read failed.
func (p *NativePage) bindInteractionBlocks(blocks []product.Block) []product.Block {
	cards := p.syncInteractionCards()
	pending := p.pendingInteractions()
	bound, actions := product.BindInteractions(blocks, pending)
	cards.actions = actions
	keep := map[string]bool{}
	var keys []string
	for _, item := range pending {
		keep[item.Key()] = true
		keys = append(keys, item.Key())
	}
	cards.pending = strings.Join(keys, "\x00")
	for key, read := range cards.reads {
		if !keep[key] && p.readCurrent(read) {
			delete(cards.drafts, key)
			delete(cards.problem, key)
			delete(cards.reads, key)
		}
	}
	return bound
}

// interactionBusy reports a request in flight from its card or its own
// path's panel, so neither entry can submit it again meanwhile.
func (p *NativePage) interactionBusy(item product.Interaction) bool {
	if p.interactionCards.busy[item.Key()] {
		return true
	}
	if id, ok := item.Approval(); ok {
		return p.approvalSubmitting[id]
	}
	if id, ok := item.Question(); ok {
		return p.questionSubmitting[id]
	}
	handle, _ := item.Engine()
	return p.prepareEngineInteractionScope().submitting[handle.ID()]
}

// buildInteractionReadProblems shows the pending reads' failures where the
// panels that normally show them are hidden, with a retry, so a run waiting
// on an unreadable request always has a visible way forward.
func (p *NativePage) buildInteractionReadProblems(c *ui.Context) {
	var problems []string
	for _, problem := range []string{p.approvalReadProblem, p.questionReadProblem, p.prepareEngineInteractionScope().problem} {
		if problem != "" {
			problems = append(problems, problem)
		}
	}
	if len(problems) == 0 {
		return
	}
	ui.Row(c).Label("Pending request read problems").FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		ui.Text(c, strings.Join(problems, "；")).Role(ui.RoleStatus).TextColor(c.Theme().Danger).FontSize(workspaceFontCaption).MinWidth(0).Shrink(1)
		workspaceControlButton(c, "interaction-read-retry", "重新读取请求", "secondary", p.polling).Shrink(0).OnClick(func() {
			p.poll()
			p.pollEngineInteractions()
		})
	})
}

// markWaitingCardRow records the first actionable card's row once per
// projection, so frames never scan the rows.
func (p *NativePage) markWaitingCardRow() {
	cards := p.syncInteractionCards()
	cards.first = -1
	if len(cards.actions) == 0 {
		return
	}
	for i, row := range p.transcriptRows {
		if _, actionable := cards.actions[row.ID]; actionable {
			cards.first = i
			return
		}
	}
}

// waitingCardRow is the first actionable card's row, or -1.
func (p *NativePage) waitingCardRow() int {
	if p.interactionCards.actions == nil || p.interactionCards.scope != p.toolScope() {
		return -1
	}
	return p.interactionCards.first
}

// jumpToInteraction returns to the conversation and scrolls to the first
// waiting card without changing which requests are pending.
func (p *NativePage) jumpToInteraction() {
	p.trace.active = false
	if p.pane != "" {
		p.setPane("")
	}
	if row := p.waitingCardRow(); row >= 0 {
		p.chat.ScrollToRow(row)
	}
}

// buildInteractionJump offers a jump when a waiting card is out of view; it
// never scrolls the list by itself.
func (p *NativePage) buildInteractionJump(c *ui.Context) {
	row := p.waitingCardRow()
	if row < 0 {
		return
	}
	first, last := p.chat.VisibleRows()
	if last >= first && row >= first && row <= last {
		return
	}
	ui.Button(c, "有请求等待处理").Label("Jump to the waiting request").OnClick(func() { p.chat.ScrollToRow(row) })
}

// buildInteractionEntry is the one-line entry other views keep: it names how
// many requests wait and returns to their cards in the conversation.
func (p *NativePage) buildInteractionEntry(c *ui.Context) {
	count := len(p.pendingInteractions())
	if count == 0 {
		return
	}
	ui.Button(c, fmt.Sprintf("有 %d 个请求等待处理", count)).Label("Open waiting requests in the conversation").OnClick(p.jumpToInteraction)
}

// interactionActivity names what a permission asks to run, by display kind.
func interactionActivity(v product.InteractionView) string {
	if v.Tool != nil {
		switch v.Tool.Kind {
		case engine.ToolCommand:
			if command := firstLine(v.Tool.Command); command != "" {
				return "运行命令 `" + command + "`"
			}
			return "运行命令"
		case engine.ToolEdit:
			if v.Tool.Path != "" {
				return "修改 " + v.Tool.Path
			}
			return "修改文件"
		case engine.ToolRead:
			if v.Tool.Path != "" {
				return "读取 " + v.Tool.Path
			}
			return "读取文件"
		case engine.ToolSearch:
			if v.Tool.Query != "" {
				return "搜索 " + firstLine(v.Tool.Query)
			}
			return "搜索"
		case engine.ToolSubagent:
			if task := firstLine(v.Tool.Task); task != "" {
				return "子 Agent · " + task
			}
			return "子 Agent"
		}
	}
	if v.Name != "" {
		return "使用 " + v.Name
	}
	return "执行操作"
}

// interactionQuestionText is a question card's one-line subject.
func interactionQuestionText(v product.InteractionView) string {
	if len(v.Questions) == 0 {
		return "提问"
	}
	q := v.Questions[0]
	text := firstLine(q.Text)
	if q.Header != "" && text == "" {
		text = q.Header
	}
	if len(v.Questions) > 1 {
		text += " 等 " + strconv.Itoa(len(v.Questions)) + " 个问题"
	}
	return text
}

// interactionAnswerText summarizes stored answers, e.g. "选择了 B".
func interactionAnswerText(v product.InteractionView) string {
	var parts []string
	for _, answer := range v.Answers {
		switch {
		case len(answer.Choices) > 0:
			parts = append(parts, "选择了 "+strings.Join(answer.Choices, "、"))
		case answer.Text != "":
			parts = append(parts, "回答："+firstLine(answer.Text))
		}
		// An answer kept without its body shows only as answered.
	}
	return strings.Join(parts, "；")
}

func interactionStateText(state product.InteractionState) string {
	switch state {
	case product.InteractionWaiting:
		return "等待处理"
	case product.InteractionAllowed:
		return "已允许"
	case product.InteractionDenied:
		return "已拒绝"
	case product.InteractionAnswered:
		return "已回答"
	case product.InteractionSkipped:
		return "已跳过"
	case product.InteractionExpired:
		return "已过期"
	case product.InteractionCancelled:
		return "已取消"
	}
	return "已处理"
}

// interactionLine is a settled (or not actionable) card's one line.
func interactionLine(v product.InteractionView) string {
	subject := interactionActivity(v)
	if v.Kind == product.InteractionKindQuestion {
		subject = interactionQuestionText(v)
		if answers := interactionAnswerText(v); answers != "" {
			subject = answers
		}
	}
	return interactionStateText(v.State) + " · " + subject
}

// buildWorkspaceInteractionCard renders one approval or question block: a
// waiting actionable request as an accented card with its controls, any
// other one as a single muted line.
func (p *NativePage) buildWorkspaceInteractionCard(c *ui.Context, row view.Message, block product.Block) {
	cards := p.syncInteractionCards()
	v := product.InteractionView{Kind: product.InteractionKindPermission, State: product.InteractionResolved}
	if block.Interaction != nil {
		v = *block.Interaction
	}
	item, actionable := cards.actions[row.ID]
	ui.Row(c.Key("interaction-" + row.ID)).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
		ui.Box(c).Width(32).Shrink(0)
		if !actionable || v.State != product.InteractionWaiting {
			line := interactionLine(v)
			icon := "check"
			switch v.State {
			case product.InteractionDenied, product.InteractionExpired, product.InteractionCancelled:
				icon = "x"
			case product.InteractionWaiting:
				icon = "clock"
			}
			ui.Row(c).Label("Interaction " + row.ID).FillWidth().MinWidth(0).Height(37).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, workspaceIcon(icon)).Label("Interaction icon "+row.ID).Size(16, 16).Shrink(0)
				ui.Text(c, line).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted).MinWidth(0).Shrink(1).SingleLine()
			})
			return
		}
		key := item.Key()
		busy := p.interactionBusy(item)
		ui.Column(c).Label("Interaction card "+row.ID).FillWidth().MinWidth(0).Padding(12).Gap(8).Radius(12).
			Border(1, c.Theme().Accent).Background(c.Theme().Surface).Children(func() {
			icon, title := "shield-check", "需要你的允许"
			if v.Kind == product.InteractionKindQuestion {
				icon, title = "circle-question-mark", "需要你的回答"
			}
			ui.Row(c).FillWidth().Height(28).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, workspaceIcon(icon)).Label("Interaction icon "+row.ID).Size(16, 16).Shrink(0).TextColor(c.Theme().Accent)
				ui.Text(c, title).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).MinWidth(0).SingleLine()
			})
			if v.Kind == product.InteractionKindQuestion {
				p.buildInteractionQuestion(c, row.ID, item, v, busy)
			} else {
				p.buildInteractionPermission(c, row.ID, item, v, busy)
			}
			if v.Truncated {
				ui.Text(c, "内容较长，这里只显示开头部分。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
			}
			if problem := cards.problem[key]; problem != "" {
				ui.Text(c, problem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().Danger)
			}
		})
	})
}

func (p *NativePage) buildInteractionPermission(c *ui.Context, rowID string, item product.Interaction, v product.InteractionView, busy bool) {
	ui.Text(c, interactionActivity(v)).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().Text)
	if v.Detail != "" {
		ui.Text(c, v.Detail).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).MaxLines(4)
	}
	ui.Text(c, "只允许这一次调用；不会自动批准后续请求。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	ui.Row(c).Gap(8).Children(func() {
		p.workspaceControlAction(c, "interaction-allow", "允许", "", "primary", busy, func() {
			p.respondInteractionCard(rowID, item, product.InteractionResponse{Allow: true})
		})
		p.workspaceControlAction(c, "interaction-deny", "拒绝", "", "secondary", busy, func() {
			p.respondInteractionCard(rowID, item, product.InteractionResponse{Allow: false})
		})
	})
}

func (p *NativePage) interactionDraft(item product.Interaction) *workspaceInteractionDraft {
	cards := p.syncInteractionCards()
	key := item.Key()
	if cards.drafts[key] == nil {
		cards.drafts[key] = &workspaceInteractionDraft{choices: map[string][]int{}, text: map[string]*string{}}
		cards.reads[key] = interactionRead(item)
	}
	return cards.drafts[key]
}

// interactionResponse is the draft's answer, or why it cannot be sent yet.
func (d *workspaceInteractionDraft) response(v product.InteractionView) (product.InteractionResponse, bool) {
	response := product.InteractionResponse{Answers: map[string]product.InteractionAnswer{}}
	for _, q := range v.Questions {
		answer := product.InteractionAnswer{QuestionID: q.ID, Options: append([]int(nil), d.choices[q.ID]...)}
		for _, index := range answer.Options {
			answer.Choices = append(answer.Choices, q.Options[index])
		}
		if len(answer.Choices) == 0 {
			if text := d.text[q.ID]; text != nil && q.Other {
				answer.Text = *text
			}
		}
		if len(answer.Choices) == 0 && strings.TrimSpace(answer.Text) == "" {
			return response, false
		}
		response.Answers[q.ID] = answer
	}
	return response, true
}

func (p *NativePage) buildInteractionQuestion(c *ui.Context, rowID string, item product.Interaction, v product.InteractionView, busy bool) {
	draft := p.interactionDraft(item)
	for _, question := range v.Questions {
		q := question
		ui.Column(c.Key("interaction-question-" + q.ID)).FillWidth().Gap(6).Children(func() {
			if q.Header != "" {
				ui.Text(c, q.Header).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5)
			}
			ui.Text(c, q.Text).FontSize(workspaceFontBody).LineHeight(1.5)
			if len(q.Options) > 0 {
				ui.Row(c).FillWidth().Gap(8).Wrap().Children(func() {
					for i, option := range q.Options {
						index := i
						chosen := false
						for _, value := range draft.choices[q.ID] {
							chosen = chosen || value == index
						}
						label := option
						if chosen {
							label = "✓ " + option
						}
						workspaceControlButton(c, "interaction-choice-"+strconv.Itoa(i), label, "secondary", busy).OnClick(func() {
							draft.choose(q, index)
						})
					}
				})
			}
			if q.Other {
				if draft.text[q.ID] == nil {
					text := ""
					draft.text[q.ID] = &text
				}
				label := "自由回答"
				if len(q.Options) == 0 {
					label = "回答"
				}
				text := draft.text[q.ID]
				// Typing an answer of one's own replaces a chosen option.
				input := workspaceControlInput(c, "interaction-answer", text).Label(label + " · " + rowID).Disabled(busy).OnChange(func() {
					if strings.TrimSpace(*text) != "" {
						delete(draft.choices, q.ID)
					}
				})
				if q.Secret {
					input.Password()
				}
			}
		})
	}
	response, ready := draft.response(v)
	ui.Row(c).Gap(8).Children(func() {
		p.workspaceControlAction(c, "interaction-submit", "提交回答", "", "primary", busy, func() {
			p.respondInteractionCard(rowID, item, response)
		}, !ready)
		if item.CanSkip {
			p.workspaceControlAction(c, "interaction-skip", "跳过", "", "secondary", busy, func() {
				p.respondInteractionCard(rowID, item, product.InteractionResponse{})
			})
		}
	})
}

// choose toggles an option; a single-choice question keeps only the latest,
// and a chosen option replaces typed text.
func (d *workspaceInteractionDraft) choose(q product.InteractionQuestion, option int) {
	current := d.choices[q.ID]
	for i, value := range current {
		if value == option {
			d.choices[q.ID] = append(append([]int(nil), current[:i]...), current[i+1:]...)
			return
		}
	}
	if q.Multiple {
		d.choices[q.ID] = append(append([]int(nil), current...), option)
	} else {
		d.choices[q.ID] = []int{option}
	}
	if text := d.text[q.ID]; text != nil {
		*text = ""
	}
}

// interactionInFlight marks a request's own path as answering, so its panel
// shows the same busy request and observations begun meanwhile are dropped.
func (p *NativePage) interactionInFlight(item product.Interaction, busy bool) {
	mark := func(flags *map[string]bool, id string) {
		if *flags == nil {
			*flags = map[string]bool{}
		}
		if busy {
			(*flags)[id] = true
		} else {
			delete(*flags, id)
		}
	}
	if id, ok := item.Approval(); ok {
		mark(&p.approvalSubmitting, id)
		p.approvalEpoch++
		return
	}
	if id, ok := item.Question(); ok {
		mark(&p.questionSubmitting, id)
		p.questionEpoch++
		return
	}
	if handle, ok := item.Engine(); ok {
		v := p.prepareEngineInteractionScope()
		mark(&v.submitting, handle.ID())
		v.epoch++
		v.polling = false
	}
}

// removeInteraction drops an answered request from its path's pending list.
func (p *NativePage) removeInteraction(item product.Interaction) {
	if id, ok := item.Approval(); ok {
		for i, request := range p.approvals {
			if request.ID == id {
				p.approvals = append(p.approvals[:i:i], p.approvals[i+1:]...)
				break
			}
		}
	}
	if id, ok := item.Question(); ok {
		for i, q := range p.questions {
			if q.ID == id {
				p.questions = append(p.questions[:i:i], p.questions[i+1:]...)
				break
			}
		}
	}
	if handle, ok := item.Engine(); ok {
		v := p.prepareEngineInteractionScope()
		for i, current := range v.items {
			if current.Handle == handle {
				v.items = append(v.items[:i:i], v.items[i+1:]...)
				break
			}
		}
	}
}

// respondInteractionCard answers through the product's one response method.
// The receipt is kept only for the session and branch it was sent from.
func (p *NativePage) respondInteractionCard(rowID string, item product.Interaction, response product.InteractionResponse) {
	cards := p.syncInteractionCards()
	key := item.Key()
	current, exists := cards.actions[rowID]
	if p.selected == "" || p.interactionBusy(item) || !exists || !current.Same(item) {
		return
	}
	cards.busy[key] = true
	delete(cards.problem, key)
	p.interactionInFlight(item, true)
	epoch, scope, id := cards.epoch, cards.scope, p.selected
	go func() {
		err := p.Service.RespondInteraction(p.ctx, id, item, response)
		p.dispatch(func() {
			cards := &p.interactionCards
			current := cards.epoch == epoch && p.toolScope() == scope
			// Broker flags are keyed by request and always end; a native
			// request's flags belong to the scope it was sent from.
			if _, native := item.Engine(); current || !native {
				p.interactionInFlight(item, false)
			}
			if !current {
				return
			}
			delete(cards.busy, key)
			if err != nil {
				cards.problem[key] = err.Error()
				cards.reads[key] = interactionRead(item)
				return
			}
			delete(cards.drafts, key)
			p.removeInteraction(item)
			p.projectTranscript()
			p.applyDetail()
			p.poll()
		})
	}()
}

// formatTokens groups digits by thousands.
func formatTokens(n uint64) string {
	text := strconv.FormatUint(n, 10)
	for i := len(text) - 3; i > 0; i -= 3 {
		text = text[:i] + "," + text[i:]
	}
	return text
}

// usageText is the attempt's faint usage line. The cache hit rate shows only
// when the engine reported cache counts; unreported is not 0%.
func usageText(usage engine.Usage) string {
	text := "输入 " + formatTokens(usage.InputTokens) + " · 输出 " + formatTokens(usage.OutputTokens)
	if usage.CachedInputTokens != nil && usage.InputTokens > 0 {
		text += " · 缓存命中 " + strconv.FormatUint((*usage.CachedInputTokens*100+usage.InputTokens/2)/usage.InputTokens, 10) + "%"
	}
	return text
}

func buildWorkspaceUsageLine(c *ui.Context, rowID string, usage engine.Usage) {
	ui.Row(c).Label("Usage " + rowID).FillWidth().Gap(12).Children(func() {
		ui.Box(c).Width(32).Shrink(0)
		ui.Text(c, usageText(usage)).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(workspacePalette(c).controlBorder).SingleLine()
	})
}
