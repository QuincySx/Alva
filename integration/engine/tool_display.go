// INPUT: encoding/json, errors, strings, unicode/utf8
// OUTPUT: ToolDisplay/ToolKind/ToolDiff/ToolMatch/TodoItem, MapToolDisplayStrings, BoundToolDisplay, validation and shared line/diff counting for adapters
// POS: [L4.5 engine contract] Normalized, bounded tool presentation an adapter derives from its own tools; consumers render by kind, never by tool or engine name
package engine

import (
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"
)

// ToolKind is the presentation type of one tool call. The adapter that owns
// the tool chooses it; a tool it does not classify is ToolOther.
type ToolKind string

const (
	ToolCommand  ToolKind = "command"
	ToolRead     ToolKind = "read"
	ToolEdit     ToolKind = "edit"
	ToolSearch   ToolKind = "search"
	ToolSubagent ToolKind = "subagent"
	ToolTodo     ToolKind = "todo"
	ToolOther    ToolKind = "other"
)

// TodoStatus is one plan item's state.
type TodoStatus string

const (
	TodoPending    TodoStatus = "pending"
	TodoInProgress TodoStatus = "in_progress"
	TodoCompleted  TodoStatus = "completed"
)

// TodoItem is one plan item a todo call wrote.
type TodoItem struct {
	Text   string     `json:"text"`
	Status TodoStatus `json:"status"`
}

// ToolMatch is one search result; Line is 0 when the tool gave none.
type ToolMatch struct {
	Path string `json:"path,omitempty"`
	Line int    `json:"line,omitempty"`
	Text string `json:"text,omitempty"`
}

// ToolDiff is a bounded old/new text pair of a file change. Partial marks
// texts that are not whole files: replaced snippets, unified-diff hunks or a
// clipped prefix.
type ToolDiff struct {
	Old     string `json:"old,omitempty"`
	New     string `json:"new,omitempty"`
	Partial bool   `json:"partial,omitempty"`
}

// ToolDisplay is the normalized presentation of a tool call, carried on
// ToolStart and ToolEnd. Only Kind is required; fields belong to the kinds
// noted beside them, and a ToolEnd repeats the start fields it still knows
// together with result fields. Every field is bounded (see BoundToolDisplay)
// and the encoded value stays within ToolDisplayBytes. It is presentation
// only: it grants nothing and is not execution evidence.
type ToolDisplay struct {
	Kind ToolKind `json:"kind"`

	// command: OutputInTrace says the result text is a receipt, not the
	// command's output, which only the native records (the trace) hold.
	Command       string `json:"command,omitempty"`
	ExitCode      *int   `json:"exit_code,omitempty"`
	OutputInTrace bool   `json:"output_in_trace,omitempty"`

	// read, edit; the root of a search
	Path string `json:"path,omitempty"`

	// edit: Files counts the files touched (Path is the first). Added without
	// Removed is a whole-file write whose previous content is unknown.
	Files   int       `json:"files,omitempty"`
	Added   *int      `json:"added,omitempty"`
	Removed *int      `json:"removed,omitempty"`
	Diff    *ToolDiff `json:"diff,omitempty"`

	// search
	Query       string      `json:"query,omitempty"`
	Matches     *int        `json:"matches,omitempty"`
	MoreMatches bool        `json:"more_matches,omitempty"`
	Results     []ToolMatch `json:"results,omitempty"`

	// subagent: ChildSessionID is the child native session the call started.
	// Status (subagent, command) is the engine's own lower-case outcome word,
	// e.g. "completed" or "failed".
	Task           string `json:"task,omitempty"`
	ChildSessionID string `json:"child_session_id,omitempty"`
	Status         string `json:"status,omitempty"`

	// todo: Plan is free-form text when the plan is not a checklist.
	Todos []TodoItem `json:"todos,omitempty"`
	Plan  string     `json:"plan,omitempty"`

	// read, subagent: a session reference whose bounded read is the content
	// (a file snapshot, a child's selected answer), through the engine's Inspector.
	Output string `json:"output,omitempty"`
}

// Display bounds. The encoded display adds at most ToolDisplayBytes to a
// tool event beside its bounded Input and Text.
const (
	ToolDisplayBytes   = 4 << 10
	ToolDisplayResults = 10
	ToolDisplayTodos   = 30
)

const (
	toolDisplayText   = 512  // command, query, task, output reference
	toolDisplayPath   = 1024 // paths
	toolDisplayDiff   = 1024 // each of Old and New
	toolDisplayPlan   = 1024
	toolDisplayItem   = 160 // result and todo text
	toolDisplayStatus = 32
)

// BoundToolDisplay returns a copy clipped to every field bound (UTF-8 safe;
// a clipped diff becomes Partial) and, if the encoding still exceeds
// ToolDisplayBytes, drops results, todos, the diff and the plan in that order.
// An empty or unknown kind becomes ToolOther with no fields.
func BoundToolDisplay(display ToolDisplay) *ToolDisplay {
	d := display
	if !knownToolKind(d.Kind) {
		return &ToolDisplay{Kind: ToolOther}
	}
	d.Command = clip(firstLines(d.Command), toolDisplayText)
	d.Path = clip(d.Path, toolDisplayPath)
	d.Query = clip(d.Query, toolDisplayText)
	d.Task = clip(d.Task, toolDisplayText)
	d.Plan = clip(d.Plan, toolDisplayPlan)
	d.Status = clip(strings.ToLower(d.Status), toolDisplayStatus)
	if len(d.ChildSessionID) > 256 || !utf8.ValidString(d.ChildSessionID) || strings.ContainsRune(d.ChildSessionID, 0) {
		d.ChildSessionID = ""
	}
	if len(d.Output) > toolDisplayText || !utf8.ValidString(d.Output) || strings.ContainsRune(d.Output, 0) {
		d.Output = ""
	}
	if d.Diff != nil {
		diff := *d.Diff
		old, new := clipLines(diff.Old, toolDisplayDiff), clipLines(diff.New, toolDisplayDiff)
		diff.Partial = diff.Partial || old != diff.Old || new != diff.New
		diff.Old, diff.New = old, new
		d.Diff = &diff
	}
	d.Results = append([]ToolMatch(nil), d.Results[:min(len(d.Results), ToolDisplayResults)]...)
	for i := range d.Results {
		d.Results[i].Path, d.Results[i].Text = clip(d.Results[i].Path, toolDisplayPath), clip(firstLines(d.Results[i].Text), toolDisplayItem)
		d.Results[i].Line = max(d.Results[i].Line, 0)
	}
	if len(d.Results) == 0 {
		d.Results = nil
	}
	d.Todos = append([]TodoItem(nil), d.Todos[:min(len(d.Todos), ToolDisplayTodos)]...)
	for i := range d.Todos {
		d.Todos[i].Text = clip(firstLines(d.Todos[i].Text), toolDisplayItem)
		if !knownTodoStatus(d.Todos[i].Status) {
			d.Todos[i].Status = TodoPending
		}
	}
	if len(d.Todos) == 0 {
		d.Todos = nil
	}
	for _, count := range []**int{&d.Added, &d.Removed, &d.Matches} {
		if *count != nil {
			value := max(**count, 0)
			*count = &value
		}
	}
	if d.ExitCode != nil {
		code := *d.ExitCode
		d.ExitCode = &code
	}
	d.Files = max(d.Files, 0)
	d.keepKindFields()
	for encodedSize(d) > ToolDisplayBytes {
		switch {
		case len(d.Results) > 0:
			d.Results = nil
		case len(d.Todos) > 1:
			d.Todos = d.Todos[:len(d.Todos)/2]
		case len(d.Todos) == 1:
			d.Todos = nil
		case d.Diff != nil:
			d.Diff = nil
		case d.Plan != "":
			d.Plan = ""
		default:
			return &ToolDisplay{Kind: d.Kind}
		}
	}
	return &d
}

// keepKindFields clears fields that do not belong to the display's kind.
func (d *ToolDisplay) keepKindFields() {
	kind := d.Kind
	if kind != ToolCommand {
		d.Command, d.ExitCode, d.OutputInTrace = "", nil, false
	}
	if kind != ToolRead && kind != ToolEdit && kind != ToolSearch {
		d.Path = ""
	}
	if kind != ToolEdit {
		d.Files, d.Added, d.Removed, d.Diff = 0, nil, nil, nil
	}
	if kind != ToolSearch {
		d.Query, d.Matches, d.MoreMatches, d.Results = "", nil, false, nil
	}
	if kind != ToolSubagent {
		d.Task, d.ChildSessionID = "", ""
	}
	if kind != ToolSubagent && kind != ToolCommand {
		d.Status = ""
	}
	if kind != ToolTodo {
		d.Todos, d.Plan = nil, ""
	}
	if kind != ToolRead && kind != ToolSubagent {
		d.Output = ""
	}
}

// Validate rejects a display that is malformed or exceeds its bounds, the
// way StreamContract rejects unbounded Text or Input.
func (d *ToolDisplay) Validate() error {
	if d == nil {
		return nil
	}
	if !knownToolKind(d.Kind) {
		return errors.New("tool display kind is unknown")
	}
	text := func(value string, limit int) bool {
		return len(value) <= limit && utf8.ValidString(value) && !strings.ContainsRune(value, 0)
	}
	if !text(d.Command, toolDisplayText) || !text(d.Path, toolDisplayPath) || !text(d.Query, toolDisplayText) || !text(d.Task, toolDisplayText) || !text(d.Plan, toolDisplayPlan) || !text(d.Output, toolDisplayText) || !text(d.Status, toolDisplayStatus) || !text(d.ChildSessionID, 256) {
		return errors.New("tool display text exceeds its bound")
	}
	if d.Status != strings.ToLower(d.Status) {
		return errors.New("tool display status must be a lower-case outcome word")
	}
	if d.Diff != nil && (!text(d.Diff.Old, toolDisplayDiff) || !text(d.Diff.New, toolDisplayDiff)) {
		return errors.New("tool display diff exceeds its bound")
	}
	if len(d.Results) > ToolDisplayResults || len(d.Todos) > ToolDisplayTodos {
		return errors.New("tool display list exceeds its bound")
	}
	for _, match := range d.Results {
		if !text(match.Path, toolDisplayPath) || !text(match.Text, toolDisplayItem) || match.Line < 0 {
			return errors.New("tool display result exceeds its bound")
		}
	}
	for _, todo := range d.Todos {
		if !text(todo.Text, toolDisplayItem) || !knownTodoStatus(todo.Status) {
			return errors.New("tool display todo is malformed")
		}
	}
	for _, count := range []*int{d.Added, d.Removed, d.Matches} {
		if count != nil && *count < 0 {
			return errors.New("tool display counts cannot be negative")
		}
	}
	if d.Files < 0 {
		return errors.New("tool display counts cannot be negative")
	}
	mark := func(set bool, kinds ...ToolKind) bool {
		if !set {
			return true
		}
		for _, kind := range kinds {
			if kind == d.Kind {
				return true
			}
		}
		return false
	}
	if !mark(d.Command != "" || d.ExitCode != nil || d.OutputInTrace, ToolCommand) ||
		!mark(d.Path != "", ToolRead, ToolEdit, ToolSearch) ||
		!mark(d.Files != 0 || d.Added != nil || d.Removed != nil || d.Diff != nil, ToolEdit) ||
		!mark(d.Query != "" || d.Matches != nil || d.MoreMatches || len(d.Results) > 0, ToolSearch) ||
		!mark(d.Task != "" || d.ChildSessionID != "", ToolSubagent) ||
		!mark(d.Status != "", ToolSubagent, ToolCommand) ||
		!mark(len(d.Todos) > 0 || d.Plan != "", ToolTodo) ||
		!mark(d.Output != "", ToolRead, ToolSubagent) {
		return errors.New("tool display field does not belong to its kind")
	}
	if encodedSize(*d) > ToolDisplayBytes {
		return errors.New("tool display exceeds its encoded bound")
	}
	return nil
}

// MapToolDisplayStrings returns a copy with f applied to every string field
// except Kind, e.g. an adapter's redaction before BoundToolDisplay.
func MapToolDisplayStrings(display ToolDisplay, f func(string) string) ToolDisplay {
	d := *CloneToolDisplay(&display)
	for _, field := range []*string{&d.Command, &d.Path, &d.Query, &d.Task, &d.ChildSessionID, &d.Status, &d.Plan, &d.Output} {
		if *field != "" {
			*field = f(*field)
		}
	}
	if d.Diff != nil {
		if d.Diff.Old != "" {
			d.Diff.Old = f(d.Diff.Old)
		}
		if d.Diff.New != "" {
			d.Diff.New = f(d.Diff.New)
		}
	}
	for i := range d.Results {
		if d.Results[i].Path != "" {
			d.Results[i].Path = f(d.Results[i].Path)
		}
		if d.Results[i].Text != "" {
			d.Results[i].Text = f(d.Results[i].Text)
		}
	}
	for i := range d.Todos {
		if d.Todos[i].Text != "" {
			d.Todos[i].Text = f(d.Todos[i].Text)
		}
	}
	return d
}

// CloneToolDisplay returns an independent copy.
func CloneToolDisplay(display *ToolDisplay) *ToolDisplay {
	if display == nil {
		return nil
	}
	d := *display
	for _, count := range []**int{&d.ExitCode, &d.Added, &d.Removed, &d.Matches} {
		if *count != nil {
			value := **count
			*count = &value
		}
	}
	if d.Diff != nil {
		diff := *d.Diff
		d.Diff = &diff
	}
	d.Results = append([]ToolMatch(nil), d.Results...)
	d.Todos = append([]TodoItem(nil), d.Todos...)
	if len(d.Results) == 0 {
		d.Results = nil
	}
	if len(d.Todos) == 0 {
		d.Todos = nil
	}
	return &d
}

func knownToolKind(kind ToolKind) bool {
	switch kind {
	case ToolCommand, ToolRead, ToolEdit, ToolSearch, ToolSubagent, ToolTodo, ToolOther:
		return true
	}
	return false
}

func knownTodoStatus(status TodoStatus) bool {
	return status == TodoPending || status == TodoInProgress || status == TodoCompleted
}

func encodedSize(d ToolDisplay) int {
	data, err := json.Marshal(d)
	if err != nil {
		return ToolDisplayBytes + 1
	}
	return len(data)
}

// clip returns a UTF-8-safe prefix of at most limit bytes without NUL.
func clip(value string, limit int) string {
	value = strings.ReplaceAll(value, "\x00", "")
	if !utf8.ValidString(value) {
		value = strings.ToValidUTF8(value, "")
	}
	if len(value) <= limit {
		return value
	}
	value = value[:limit]
	for len(value) > 0 && !utf8.ValidString(value) {
		value = value[:len(value)-1]
	}
	return value
}

// clipLines clips at the last line break within the limit when there is one.
func clipLines(value string, limit int) string {
	clipped := clip(value, limit)
	if clipped == value {
		return value
	}
	if index := strings.LastIndexByte(clipped, '\n'); index > 0 {
		return clipped[:index+1]
	}
	return clipped
}

// firstLines keeps a one-line label: the first line, with "…" when more follow.
func firstLines(value string) string {
	value = strings.TrimSpace(value)
	if line, _, more := strings.Cut(value, "\n"); more {
		return strings.TrimSpace(line) + " …"
	}
	return value
}

// LineCount counts text lines; a final line without a newline counts.
func LineCount(text string) int {
	if text == "" {
		return 0
	}
	n := strings.Count(text, "\n")
	if !strings.HasSuffix(text, "\n") {
		n++
	}
	return n
}

const lineChangeCells = 1 << 18

// LineChanges counts added and removed lines between two texts: common
// leading and trailing lines are dropped, then a bounded LCS counts the rest
// (whole-middle counts when the middle is too large to compare).
func LineChanges(old, updated string) (added, removed int) {
	a, b := splitLines(old), splitLines(updated)
	for len(a) > 0 && len(b) > 0 && a[0] == b[0] {
		a, b = a[1:], b[1:]
	}
	for len(a) > 0 && len(b) > 0 && a[len(a)-1] == b[len(b)-1] {
		a, b = a[:len(a)-1], b[:len(b)-1]
	}
	if len(a) == 0 || len(b) == 0 || len(a)*len(b) > lineChangeCells {
		return len(b), len(a)
	}
	previous, row := make([]int, len(b)+1), make([]int, len(b)+1)
	for i := range a {
		for j := range b {
			switch {
			case a[i] == b[j]:
				row[j+1] = previous[j] + 1
			case row[j] > previous[j+1]:
				row[j+1] = row[j]
			default:
				row[j+1] = previous[j+1]
			}
		}
		previous, row = row, previous
	}
	common := previous[len(b)]
	return len(b) - common, len(a) - common
}

func splitLines(text string) []string {
	if text == "" {
		return nil
	}
	return strings.Split(strings.TrimSuffix(text, "\n"), "\n")
}

// UnifiedDiff rebuilds a unified diff's hunk lines as old/new texts and
// counts added and removed lines; file and hunk headers are skipped.
func UnifiedDiff(diff string) (old, updated string, added, removed int) {
	var a, b []string
	for _, line := range strings.Split(diff, "\n") {
		switch {
		case strings.HasPrefix(line, "+++"), strings.HasPrefix(line, "---"), strings.HasPrefix(line, "@@"), strings.HasPrefix(line, "diff "), strings.HasPrefix(line, "index "), strings.HasPrefix(line, `\`):
		case strings.HasPrefix(line, "+"):
			b = append(b, line[1:])
			added++
		case strings.HasPrefix(line, "-"):
			a = append(a, line[1:])
			removed++
		case strings.HasPrefix(line, " "):
			a, b = append(a, line[1:]), append(b, line[1:])
		}
	}
	return strings.Join(a, "\n"), strings.Join(b, "\n"), added, removed
}

// Checklist parses Markdown checklist lines ("- [ ]", "- [x]", "- [~]") into
// todo items; text without any checklist line yields none.
func Checklist(content string) []TodoItem {
	var todos []TodoItem
	for _, line := range strings.Split(content, "\n") {
		trimmed := strings.TrimSpace(line)
		marker := ""
		for _, prefix := range []string{"- ", "* ", "+ "} {
			if strings.HasPrefix(trimmed, prefix+"[") && len(trimmed) >= len(prefix)+3 && trimmed[len(prefix)+2] == ']' {
				marker, trimmed = trimmed[len(prefix)+1:len(prefix)+2], strings.TrimSpace(trimmed[len(prefix)+3:])
				break
			}
		}
		if marker == "" {
			continue
		}
		status := TodoPending
		switch marker {
		case "x", "X":
			status = TodoCompleted
		case "~", "-", ">":
			status = TodoInProgress
		}
		todos = append(todos, TodoItem{Text: trimmed, Status: status})
	}
	return todos
}
