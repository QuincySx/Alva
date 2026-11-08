// INPUT: Committed engine interaction events, the Alva approval/question broker snapshots, native engine pending handles and window blocks
// OUTPUT: InteractionView (bounded, UI independent), Interaction pending items with private handles and whole option labels, CombineInteractions, BindInteractions, PendingInteractions and RespondInteraction (choices by option index, sent whole)
// POS: [L7 Hub product] One approval/question view over both interaction paths; a card is only another entry to the existing product responses
package product

import (
	"context"
	"errors"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

// InteractionKind tells an approval from a question.
type InteractionKind string

const (
	InteractionKindPermission InteractionKind = "permission"
	InteractionKindQuestion   InteractionKind = "question"
)

// InteractionState is where a request stands. Resolved means it settled
// without a recorded decision (events from before resolutions were kept).
type InteractionState string

const (
	InteractionWaiting   InteractionState = "waiting"
	InteractionAllowed   InteractionState = "allowed"
	InteractionDenied    InteractionState = "denied"
	InteractionAnswered  InteractionState = "answered"
	InteractionSkipped   InteractionState = "skipped"
	InteractionResolved  InteractionState = "resolved"
	InteractionExpired   InteractionState = "expired"
	InteractionCancelled InteractionState = "cancelled"
)

// Bounds of every stored interaction text. A clipped value sets Truncated.
const (
	interactionNameBytes     = 128
	interactionDetailBytes   = 1 << 10
	interactionHeaderBytes   = 128
	interactionQuestionBytes = 1 << 10
	interactionOptionBytes   = 256
	interactionAnswerBytes   = 512
	interactionQuestions     = 4
	interactionOptions       = 16
	interactionAnswerValues  = 16
)

// InteractionQuestion is one bounded question of a request.
type InteractionQuestion struct {
	ID, Header, Text string
	Options          []string
	// Multiple allows several options; Other accepts free text; Secret masks
	// the typed answer.
	Multiple, Other, Secret bool
}

// InteractionAnswer is one question's settled answer as stored: the offered
// options chosen, or free text (a redacted answer stays redacted).
type InteractionAnswer struct {
	QuestionID string
	Choices    []string
	Text       string
	// Options names chosen offered options by their 0-based index in the
	// question. A response should use it: the view clips long labels for
	// display, and RespondInteraction sends the whole label for an index.
	Options []int
}

// InteractionView is an approval or question block's presentation. It is
// bounded, knows no tool or engine name and grants nothing. A published value
// is never mutated: the window replaces the pointer on change.
type InteractionView struct {
	Kind  InteractionKind
	State InteractionState
	// Permission: Tool is the asked activity's normalized display when known,
	// Name the engine's own name for it, Effect its declared effect and
	// Detail a reason or input summary.
	Tool   *engine.ToolDisplay
	Name   string
	Effect engine.Effect
	Detail string
	// Question: the questions asked and, once settled, the answers.
	Questions []InteractionQuestion
	Answers   []InteractionAnswer
	// Truncated says some text was clipped to its bound.
	Truncated bool
}

// clipText returns a UTF-8-safe prefix within limit and whether it clipped.
func clipText(value string, limit int) (string, bool) {
	value = strings.ToValidUTF8(strings.ReplaceAll(value, "\x00", ""), "")
	if len(value) <= limit {
		return value, false
	}
	value = value[:limit]
	for len(value) > 0 && !utf8.ValidString(value) {
		value = value[:len(value)-1]
	}
	return value, true
}

func (v *InteractionView) clip(value string, limit int) string {
	clipped, cut := clipText(value, limit)
	v.Truncated = v.Truncated || cut
	return clipped
}

func knownEffect(effect engine.Effect) engine.Effect {
	switch effect {
	case engine.EffectRead, engine.EffectWrite, engine.EffectExecute, engine.EffectNetwork, engine.EffectNone:
		return effect
	}
	return engine.EffectUnknown
}

// permissionView is a committed or pending engine permission request.
func permissionView(event engine.Event) *InteractionView {
	v := &InteractionView{Kind: InteractionKindPermission, State: InteractionWaiting, Effect: knownEffect(event.Effect), Tool: engine.CloneToolDisplay(event.Display)}
	v.Name = v.clip(event.Name, interactionNameBytes)
	// The reason, or the raw request input only when no display names it.
	detail := event.Text
	if detail == "" && event.Display == nil {
		detail = string(event.Input)
	}
	v.Detail = v.clip(detail, interactionDetailBytes)
	return v
}

// questionView is a committed or pending engine question.
func questionView(request *engine.UserInputRequest) *InteractionView {
	v := &InteractionView{Kind: InteractionKindQuestion, State: InteractionWaiting}
	if request == nil {
		return v
	}
	for i, q := range request.Questions {
		if i == interactionQuestions {
			v.Truncated = true
			break
		}
		question := InteractionQuestion{ID: q.ID, Header: v.clip(q.Header, interactionHeaderBytes), Text: v.clip(q.Question, interactionQuestionBytes), Other: q.IsOther || len(q.Options) == 0, Secret: q.IsSecret}
		for j, option := range q.Options {
			if j == interactionOptions {
				v.Truncated = true
				break
			}
			question.Options = append(question.Options, v.clip(option.Label, interactionOptionBytes))
		}
		v.Questions = append(v.Questions, question)
	}
	return v
}

// resolve settles a waiting view with an adapter's resolution, if any.
func (v InteractionView) resolve(resolution *engine.InteractionResolution) *InteractionView {
	v.State, v.Answers = InteractionResolved, nil
	switch {
	case resolution == nil:
	case v.Kind == InteractionKindPermission && resolution.Allowed != nil:
		v.State = InteractionDenied
		if *resolution.Allowed {
			v.State = InteractionAllowed
		}
	case v.Kind == InteractionKindQuestion && len(resolution.Answers) == 0:
		v.State = InteractionSkipped
	case v.Kind == InteractionKindQuestion:
		v.State = InteractionAnswered
		v.Answers = v.summarize(resolution.Answers)
	}
	return &v
}

// summarize keeps each answered question's choices when every value is an
// offered option, its free text otherwise, in question order and bounded.
func (v *InteractionView) summarize(answers map[string][]string) []InteractionAnswer {
	var out []InteractionAnswer
	seen := map[string]bool{}
	add := func(id string, options []string) {
		values, exists := answers[id]
		if !exists || seen[id] || len(out) == interactionQuestions {
			return
		}
		seen[id] = true
		answer := InteractionAnswer{QuestionID: id}
		offered := len(values) > 0 && len(options) > 0
		for _, value := range values {
			found := false
			for _, option := range options {
				found = found || value == option
			}
			offered = offered && found
		}
		if offered {
			for i, value := range values {
				if i == interactionAnswerValues {
					v.Truncated = true
					break
				}
				answer.Choices = append(answer.Choices, v.clip(value, interactionOptionBytes))
			}
		} else {
			answer.Text = v.clip(strings.Join(values, ", "), interactionAnswerBytes)
		}
		out = append(out, answer)
	}
	for _, q := range v.Questions {
		add(q.ID, q.Options)
	}
	// An answer to a question this view does not list (clipped questions or
	// a foreign key) still shows, after the listed ones, in key order.
	extra := make([]string, 0, len(answers))
	for id := range answers {
		if !seen[id] {
			extra = append(extra, id)
		}
	}
	sort.Strings(extra)
	for _, id := range extra {
		if len(out) == interactionQuestions {
			v.Truncated = true
			break
		}
		add(id, nil)
	}
	return out
}

// interactionViewBytes approximates a view's memory for the window bound.
func interactionViewBytes(v *InteractionView) int {
	if v == nil {
		return 0
	}
	size := 160 + len(v.Name) + len(v.Detail)
	if v.Tool != nil {
		size += toolViewBytes(&ToolView{ToolDisplay: *v.Tool})
	}
	for _, q := range v.Questions {
		size += 48 + len(q.ID) + len(q.Header) + len(q.Text)
		for _, option := range q.Options {
			size += 16 + len(option)
		}
	}
	for _, a := range v.Answers {
		size += 48 + len(a.QuestionID) + len(a.Text)
		for _, choice := range a.Choices {
			size += 16 + len(choice)
		}
	}
	return size
}

// interactionBlockID is a request's block: its attempt, kind and event ID.
// Permission and question requests may share an engine request ID.
func interactionBlockID(attempt string, kind InteractionKind, id string) string {
	return attempt + "/" + string(kind) + "/" + id
}

func resolvedKind(name string) InteractionKind {
	if name == "user_input" {
		return InteractionKindQuestion
	}
	return InteractionKindPermission
}

type interactionPath uint8

const (
	pathApproval interactionPath = iota + 1 // Alva approval broker
	pathQuestion                            // Alva question broker
	pathEngine                              // native engine callback
)

// Interaction is one actionable pending request from either path. It comes
// only from the pending reads; its private handle is what a response needs
// and cannot be built from block or native IDs.
type Interaction struct {
	View InteractionView
	// BlockID is the history block of this request when its history carries
	// it (native engine requests). Otherwise CallID and NativeSessionID name
	// the tool call that asked, which the request follows.
	BlockID, CallID, NativeSessionID string
	// CanSkip says an empty answer explicitly skips the question.
	CanSkip  bool
	path     interactionPath
	request  string
	question interaction.Question
	handle   EngineInteractionHandle
	// labels keeps each native question's whole offered labels, in order,
	// for the options the view shows; the view's labels may be clipped.
	labels map[string][]string
}

// Key is a stable identity for UI state such as drafts and busy flags.
func (i Interaction) Key() string {
	switch i.path {
	case pathEngine:
		return "engine/" + i.handle.ID()
	case pathQuestion:
		return "question/" + i.request
	case pathApproval:
		return "approval/" + i.request
	}
	return ""
}

// Approval, Question and Engine report which pending read an item came from
// and that read's own identity, for UI state kept per read.
func (i Interaction) Approval() (string, bool) { return i.request, i.path == pathApproval }
func (i Interaction) Question() (string, bool) { return i.request, i.path == pathQuestion }
func (i Interaction) Engine() (EngineInteractionHandle, bool) {
	return i.handle, i.path == pathEngine
}

// Same reports whether two values are the same pending request and handle.
func (i Interaction) Same(other Interaction) bool {
	return i.path == other.path && i.request == other.request && i.handle == other.handle
}

func approvalInteraction(request security.ApprovalRequest) Interaction {
	v := InteractionView{Kind: InteractionKindPermission, State: InteractionWaiting, Effect: knownEffect(engine.Effect(request.Effect.Kind))}
	v.Name = v.clip(request.ToolName, interactionNameBytes)
	detail := request.InputSummary
	if detail == "" {
		detail = request.Effect.Description
	}
	v.Detail = v.clip(detail, interactionDetailBytes)
	return Interaction{View: v, CallID: request.CallID, NativeSessionID: request.SessionID, path: pathApproval, request: request.ID}
}

func questionInteraction(q interaction.Question) Interaction {
	v := InteractionView{Kind: InteractionKindQuestion, State: InteractionWaiting}
	// Alva questions always accept free text as well as their options.
	question := InteractionQuestion{ID: q.ID, Header: v.clip(q.Header, interactionHeaderBytes), Text: v.clip(q.Prompt, interactionQuestionBytes), Multiple: q.MultiSelect, Other: true}
	for i, choice := range q.Options {
		if i == interactionOptions {
			v.Truncated = true
			break
		}
		question.Options = append(question.Options, v.clip(choice.Label, interactionOptionBytes))
	}
	v.Questions = []InteractionQuestion{question}
	return Interaction{View: v, CallID: q.CallID, NativeSessionID: q.SessionID, path: pathQuestion, request: q.ID, question: interaction.CloneQuestion(q)}
}

func engineInteraction(item EngineInteraction) Interaction {
	result := Interaction{path: pathEngine, request: item.Event.ID, handle: item.Handle}
	if item.Event.Kind == engine.UserInputRequested {
		result.View = *questionView(item.Event.Question)
		result.labels = map[string][]string{}
		for _, q := range item.Event.Question.Questions {
			for i, option := range q.Options {
				if i == interactionOptions {
					break
				}
				result.labels[q.ID] = append(result.labels[q.ID], option.Label)
			}
		}
		result.BlockID = interactionBlockID(item.Handle.binding.AttemptID, InteractionKindQuestion, item.Event.ID)
		result.CanSkip = true
	} else {
		result.View = *permissionView(item.Event)
		result.BlockID = interactionBlockID(item.Handle.binding.AttemptID, InteractionKindPermission, item.Event.ID)
	}
	return result
}

// CombineInteractions merges both paths' pending reads into one list: Alva
// broker approvals and questions, then native engine requests, each in the
// order its read returned.
func CombineInteractions(approvals []security.ApprovalRequest, questions []interaction.Question, native []EngineInteraction) []Interaction {
	items := make([]Interaction, 0, len(approvals)+len(questions)+len(native))
	for _, request := range approvals {
		items = append(items, approvalInteraction(request))
	}
	for _, q := range questions {
		items = append(items, questionInteraction(q))
	}
	for _, item := range native {
		if item.Event.Kind == engine.PermissionRequest || item.Event.Kind == engine.UserInputRequested {
			items = append(items, engineInteraction(item))
		}
	}
	return items
}

// PendingInteractions is the one interaction read over both paths. Each
// underlying read keeps its own ownership checks.
func (s *HubService) PendingInteractions(ctx context.Context, id string) ([]Interaction, error) {
	approvals, err := s.PendingApprovals(ctx, id)
	if err != nil {
		return nil, err
	}
	questions, err := s.PendingQuestions(ctx, id)
	if err != nil {
		return nil, err
	}
	var native []EngineInteraction
	if s.Runtime != nil {
		if native, err = s.PendingEngineInteractions(ctx, id); err != nil {
			return nil, err
		}
	}
	return CombineInteractions(approvals, questions, native), nil
}

// InteractionResponse decides a permission (Allow) or answers a question
// (Answers by question ID; nil or empty skips when the request allows it).
type InteractionResponse struct {
	Allow   bool
	Answers map[string]InteractionAnswer
}

// RespondInteraction answers one pending request through the same product
// method its own path's panel uses, so consent stays once-only, frozen to
// its owner and rejected when late: a second response fails explicitly.
func (s *HubService) RespondInteraction(ctx context.Context, id string, item Interaction, response InteractionResponse) error {
	switch item.path {
	case pathApproval:
		return s.RespondApproval(ctx, id, item.request, response.Allow)
	case pathQuestion:
		indexes, err := item.choiceIndexes(item.question.ID, response.Answers[item.question.ID])
		if err != nil {
			return err
		}
		answer, err := questionAnswer(indexes, response.Answers[item.question.ID])
		if err != nil {
			return err
		}
		return s.RespondQuestion(ctx, id, item.request, answer)
	case pathEngine:
		if item.View.Kind == InteractionKindPermission {
			return s.RespondEnginePermission(ctx, id, item.handle, response.Allow)
		}
		answers := make(map[string][]string, len(response.Answers))
		for question, answer := range response.Answers {
			indexes, err := item.choiceIndexes(question, answer)
			if err != nil {
				return err
			}
			switch {
			case len(indexes) > 0:
				for _, index := range indexes {
					answers[question] = append(answers[question], item.labels[question][index])
				}
			case answer.Text != "":
				answers[question] = []string{answer.Text}
			}
		}
		return s.RespondEngineUserInput(ctx, id, item.handle, engine.UserInputResponse{Answers: answers})
	}
	return errors.New("interaction is not a pending request")
}

// offered is a question's whole option labels, in the view's order.
func (i Interaction) offered(question string) []string {
	if i.path == pathQuestion {
		if question != i.question.ID {
			return nil
		}
		labels := make([]string, 0, len(i.question.Options))
		for _, option := range i.question.Options {
			labels = append(labels, option.Label)
		}
		return labels
	}
	return i.labels[question]
}

// choiceIndexes is the 0-based offered options an answer chose: Options
// when given, otherwise each displayed label's position. Every index must
// name an option the view shows.
func (i Interaction) choiceIndexes(question string, answer InteractionAnswer) ([]int, error) {
	var shown []string
	for _, q := range i.View.Questions {
		if q.ID == question {
			shown = q.Options
		}
	}
	full := i.offered(question)
	limit := min(len(shown), len(full))
	indexes := append([]int(nil), answer.Options...)
	if len(indexes) == 0 {
		for _, label := range answer.Choices {
			index := -1
			for j := 0; j < limit && index < 0; j++ {
				if shown[j] == label {
					index = j
				}
			}
			indexes = append(indexes, index)
		}
	}
	for _, index := range indexes {
		if index < 0 || index >= limit {
			return nil, errors.New("answer is not an offered choice")
		}
	}
	return indexes, nil
}

// questionAnswer turns chosen options into the broker's 1-based selections,
// or free text into its text answer.
func questionAnswer(indexes []int, answer InteractionAnswer) (interaction.Answer, error) {
	if len(indexes) == 0 {
		if strings.TrimSpace(answer.Text) == "" {
			return interaction.Answer{}, errors.New("answer requires a choice or text")
		}
		text := answer.Text
		return interaction.Answer{Text: &text}, nil
	}
	selected := make([]int, 0, len(indexes))
	for _, index := range indexes {
		selected = append(selected, index+1)
	}
	return interaction.Answer{Selected: selected}, nil
}

// pendingBlockID is a request's block when its history does not carry it.
func pendingBlockID(item Interaction) string { return "pending/" + item.Key() }

// BindInteractions places pending requests among the window's blocks and
// returns the blocks plus the actionable request of each waiting block.
// A request whose history block is shown makes that block actionable; one
// asked by a shown tool call follows that call, which then shows waiting;
// any other request floats after the last block. A waiting history block
// without a pending request stays waiting but not actionable (its response
// may be on its way). Unchanged blocks keep their shared values.
func BindInteractions(blocks []Block, pending []Interaction) ([]Block, map[string]Interaction) {
	actions := map[string]Interaction{}
	if len(pending) == 0 {
		return blocks, actions
	}
	index := make(map[string]int, len(blocks))
	tools := map[string]int{}
	for i, b := range blocks {
		index[b.ID] = i
		if b.Kind == BlockKindTool {
			tools[toolCallKey(b.NativeSessionID, strings.TrimPrefix(b.ID, b.AttemptID+"/"))] = i
		}
	}
	after := map[int][]Block{}
	var floating []Block
	out := blocks
	copied := false
	write := func(i int, b Block) {
		if !copied {
			out = append([]Block(nil), blocks...)
			copied = true
		}
		out[i] = b
	}
	for _, item := range pending {
		if i, shown := index[item.BlockID]; item.BlockID != "" && shown {
			// Its own history block: actionable while that block waits. A
			// settled block wins over a stale pending read.
			if out[i].Kind == BlockKindInteraction && out[i].Interaction != nil && out[i].Interaction.State == InteractionWaiting {
				actions[item.BlockID] = item
			}
			continue
		}
		view := item.View
		b := Block{ID: pendingBlockID(item), Kind: BlockKindInteraction, Status: BlockStatusWaiting, Interaction: &view}
		if i, asked := tools[toolCallKey(item.NativeSessionID, item.CallID)]; item.CallID != "" && asked {
			tool := out[i]
			b.SessionID, b.TurnID, b.AttemptID, b.Turn, b.NativeSessionID, b.History = tool.SessionID, tool.TurnID, tool.AttemptID, tool.Turn, tool.NativeSessionID, tool.History
			if view.Kind == InteractionKindPermission && view.Tool == nil && tool.Tool != nil {
				view.Tool = engine.CloneToolDisplay(&tool.Tool.ToolDisplay)
			}
			if tool.Status != BlockStatusWaiting {
				tool.Status = BlockStatusWaiting
				write(i, tool)
			}
			after[i] = append(after[i], b)
		} else {
			floating = append(floating, b)
		}
		actions[b.ID] = item
	}
	if len(after) == 0 && len(floating) == 0 {
		return out, actions
	}
	merged := make([]Block, 0, len(out)+len(floating)+len(after))
	for i, b := range out {
		merged = append(merged, b)
		merged = append(merged, after[i]...)
	}
	return append(merged, floating...), actions
}

func toolCallKey(native, call string) string { return native + "\x00" + call }
