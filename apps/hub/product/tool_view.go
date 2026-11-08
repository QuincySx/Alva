// INPUT: strconv, strings, unicode/utf8 and the engine's normalized tool displays on committed tool events
// OUTPUT: ToolView (display folded across a call's events), ToolBody/ToolJump, ToolTail and ToolLineSummary for Message Stream blocks
// POS: [L7 Hub product] Turns adapter-normalized tool displays into block views and owns cross-block rules; knows no tool or engine names
package product

import (
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
)

// ToolBody names where a tool block's expanded content comes from: the
// block's own committed Text, or a bounded read of Output through the
// block's native branch (the trace reader's Raw pages).
type ToolBody string

const (
	ToolBodyText      ToolBody = "text"
	ToolBodyReference ToolBody = "reference"
)

// ToolJump is a block's type-specific jump. Every tool block can also open
// its trace step; ToolJumpTrace means that is the only jump.
type ToolJump string

const (
	ToolJumpTrace ToolJump = "trace" // full output or record: the trace step
	ToolJumpFile  ToolJump = "file"  // the file at Path in the resource panel
	ToolJumpChild ToolJump = "child" // the child session record of a sub-Agent call
)

// ToolView is a tool block's presentation: the engine adapter's display
// folded across the call's events (later fields win, earlier ones fill the
// gaps) plus product facts. An event without a display contributes nothing,
// so an old call renders as ToolOther with its name and text. A published
// value is never mutated: the window replaces the pointer on change.
type ToolView struct {
	engine.ToolDisplay
	// Name is the engine-reported tool name, shown only for ToolOther.
	Name string
	// Superseded marks a todo block a later todo call of the same Turn replaced.
	Superseded bool
	Body       ToolBody
	Jump       ToolJump
}

// foldToolView folds one tool event into its call's view and returns a new
// value; previous is never modified.
func foldToolView(previous *ToolView, name string, event engine.Event) *ToolView {
	view := ToolView{ToolDisplay: engine.ToolDisplay{Kind: engine.ToolOther}}
	if previous != nil {
		view = cloneToolView(*previous)
	}
	if name != "" {
		view.Name = name
	}
	if event.Display != nil {
		view.ToolDisplay = overlayDisplay(view.ToolDisplay, *event.Display)
	}
	view.settle()
	return &view
}

func cloneToolView(view ToolView) ToolView {
	view.ToolDisplay = *engine.CloneToolDisplay(&view.ToolDisplay)
	return view
}

// overlayDisplay returns base with every field top sets; a different kind
// (or a base without one) takes top whole.
func overlayDisplay(base, top engine.ToolDisplay) engine.ToolDisplay {
	top = *engine.CloneToolDisplay(&top)
	if base.Kind != top.Kind || base.Kind == engine.ToolOther {
		return top
	}
	merged := *engine.CloneToolDisplay(&base)
	text := func(dst *string, src string) {
		if src != "" {
			*dst = src
		}
	}
	number := func(dst **int, src *int) {
		if src != nil {
			*dst = src
		}
	}
	text(&merged.Command, top.Command)
	text(&merged.Path, top.Path)
	text(&merged.Query, top.Query)
	text(&merged.Task, top.Task)
	text(&merged.ChildSessionID, top.ChildSessionID)
	text(&merged.Status, top.Status)
	text(&merged.Plan, top.Plan)
	text(&merged.Output, top.Output)
	number(&merged.ExitCode, top.ExitCode)
	number(&merged.Added, top.Added)
	number(&merged.Removed, top.Removed)
	number(&merged.Matches, top.Matches)
	if top.Files != 0 {
		merged.Files = top.Files
	}
	if top.OutputInTrace {
		merged.OutputInTrace = true
	}
	if top.Diff != nil {
		merged.Diff = top.Diff
	}
	if top.Matches != nil || len(top.Results) > 0 {
		merged.MoreMatches, merged.Results = top.MoreMatches, top.Results
	}
	if len(top.Todos) > 0 {
		merged.Todos = top.Todos
	}
	return merged
}

// settle derives the body source and jump from the fields present.
func (v *ToolView) settle() {
	v.Body, v.Jump = ToolBodyText, ToolJumpTrace
	if v.Output != "" {
		v.Body = ToolBodyReference
	}
	switch {
	case (v.Kind == engine.ToolRead || v.Kind == engine.ToolEdit) && v.Path != "":
		v.Jump = ToolJumpFile
	case v.Kind == engine.ToolSubagent && v.ChildSessionID != "":
		v.Jump = ToolJumpChild
	}
}

// mergeToolView folds an older part of a call (split by a page edge) under
// its current part: current fields win, older ones fill the gaps.
func mergeToolView(older, current *ToolView) *ToolView {
	if older == nil {
		return current
	}
	if current == nil {
		return older
	}
	merged := cloneToolView(*current)
	if merged.Kind == engine.ToolOther {
		merged.ToolDisplay = *engine.CloneToolDisplay(&older.ToolDisplay)
	} else if older.Kind == merged.Kind {
		merged.ToolDisplay = overlayDisplay(older.ToolDisplay, current.ToolDisplay)
	}
	if merged.Name == "" {
		merged.Name = older.Name
	}
	merged.Superseded = merged.Superseded || older.Superseded
	merged.settle()
	return &merged
}

// toolViewBytes approximates a view's memory for the window's byte bound.
func toolViewBytes(v *ToolView) int {
	if v == nil {
		return 0
	}
	size := 192 + len(v.Name) + len(v.Command) + len(v.Path) + len(v.Query) + len(v.Task) + len(v.ChildSessionID) + len(v.Status) + len(v.Plan) + len(v.Output)
	if v.Diff != nil {
		size += 32 + len(v.Diff.Old) + len(v.Diff.New)
	}
	for _, match := range v.Results {
		size += 48 + len(match.Path) + len(match.Text)
	}
	for _, todo := range v.Todos {
		size += 32 + len(todo.Text) + len(todo.Status)
	}
	return size
}

// ToolTail returns at most the last lines and bytes of text, for a command's
// output tail; it never splits a rune.
func ToolTail(text string, lines, bytes int) string {
	if bytes > 0 && len(text) > bytes {
		text = text[len(text)-bytes:]
		for len(text) > 0 && !utf8.RuneStart(text[0]) {
			text = text[1:]
		}
	}
	text = strings.TrimRight(text, "\n")
	if lines <= 0 {
		return text
	}
	index := len(text)
	for count := 0; count < lines; count++ {
		previous := strings.LastIndexByte(text[:index], '\n')
		if previous < 0 {
			return text
		}
		index = previous
	}
	return text[index+1:]
}

// ToolLineSummary formats added/removed line counts, e.g. "+3 −1"; a
// whole-file write without a known removal shows only "+N".
func ToolLineSummary(v ToolView) string {
	if v.Added == nil {
		return ""
	}
	if v.Removed == nil {
		return "+" + strconv.Itoa(*v.Added)
	}
	return "+" + strconv.Itoa(*v.Added) + " −" + strconv.Itoa(*v.Removed)
}
