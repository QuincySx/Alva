// INPUT: Synthetic committed tool events carrying adapter-normalized displays, the TranscriptWindow and a real saved workspace
// OUTPUT: Display-to-view conversion per kind, start/end folding, body sources and jumps, display-less legacy events, page-edge merges, superseded todos, byte accounting and file tab pinning
// POS: [L7 Hub product] Pure tool view tests independent of MyGo, native windows, tool names and engine names
package product

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func number(n int) *int { return &n }

// toolBlock projects one call's start and end events and returns its block.
func toolBlock(t *testing.T, name string, start, end *engine.ToolDisplay, text string, failed bool) Block {
	t.Helper()
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(
		hub.HistoryEntry{Sequence: 1, TurnID: "turn", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "call", Name: name, Display: start}},
		hub.HistoryEntry{Sequence: 2, TurnID: "turn", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Name: name, Text: text, IsError: failed, Display: end}},
	))
	block := blockByID(t, w, "attempt/call")
	if block.Kind != BlockKindTool || block.Tool == nil {
		t.Fatalf("tool block lacks its presentation: %+v", block)
	}
	return block
}

func TestToolViewFoldsCommandStartAndEndDisplays(t *testing.T) {
	start := &engine.ToolDisplay{Kind: engine.ToolCommand, Command: "go test ./..."}
	end := &engine.ToolDisplay{Kind: engine.ToolCommand, ExitCode: number(2), Status: "failed", OutputInTrace: true}
	block := toolBlock(t, "runner", start, end, "FAIL\nexit status 2\n", true)
	if v := block.Tool; v.Kind != engine.ToolCommand || v.Command != "go test ./..." || v.ExitCode == nil || *v.ExitCode != 2 || !v.OutputInTrace || v.Status != "failed" || v.Body != ToolBodyText || v.Jump != ToolJumpTrace || v.Name != "runner" || block.Status != BlockStatusFailed {
		t.Fatalf("command view: %+v", v)
	}
	if start.ExitCode != nil {
		t.Fatal("folding mutated an event display")
	}
	if tail := ToolTail("1\n2\n3\n4\n", 2, 1<<10); tail != "3\n4" {
		t.Fatalf("command output tail %q", tail)
	}
	if tail := ToolTail(strings.Repeat("长", 100), 0, 10); tail != strings.Repeat("长", 3) {
		t.Fatalf("byte-bounded tail split a rune %q", tail)
	}
}

func TestToolViewBodiesAndJumpsFollowTheDisplay(t *testing.T) {
	read := toolBlock(t, "r", &engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go"}, &engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go", Output: "alva-artifact://file"}, `{"reference":{}}`, false)
	if v := read.Tool; v.Body != ToolBodyReference || v.Output != "alva-artifact://file" || v.Jump != ToolJumpFile {
		t.Fatalf("read view: %+v", v)
	}
	edit := toolBlock(t, "e", &engine.ToolDisplay{Kind: engine.ToolEdit, Path: "a.go", Files: 2, Added: number(3), Removed: number(1), Diff: &engine.ToolDiff{Old: "b", New: "B", Partial: true}}, nil, "{}", false)
	if v := edit.Tool; v.Jump != ToolJumpFile || v.Diff == nil || ToolLineSummary(*v) != "+3 −1" || v.Files != 2 {
		t.Fatalf("edit view: %+v", v)
	}
	write := toolBlock(t, "w", &engine.ToolDisplay{Kind: engine.ToolEdit, Path: "n.go", Added: number(4)}, nil, "{}", false)
	if ToolLineSummary(*write.Tool) != "+4" {
		t.Fatal("whole-file write summary", ToolLineSummary(*write.Tool))
	}
	search := toolBlock(t, "s", &engine.ToolDisplay{Kind: engine.ToolSearch, Query: "TODO", Path: "."}, &engine.ToolDisplay{Kind: engine.ToolSearch, Query: "TODO", Matches: number(40), MoreMatches: true, Results: []engine.ToolMatch{{Path: "a.go", Text: "TODO"}}}, "{}", false)
	if v := search.Tool; v.Path != "." || *v.Matches != 40 || !v.MoreMatches || len(v.Results) != 1 || v.Jump != ToolJumpTrace {
		t.Fatalf("search view: %+v", v)
	}
	child := toolBlock(t, "c", &engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Review"}, &engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Review", ChildSessionID: "native-child", Status: "completed", Output: "alva-artifact://out"}, "{}", false)
	if v := child.Tool; v.Jump != ToolJumpChild || v.Body != ToolBodyReference || v.Status != "completed" {
		t.Fatalf("subagent view: %+v", v)
	}
	noChild := toolBlock(t, "c", &engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Elsewhere"}, nil, "", true)
	if v := noChild.Tool; v.Jump != ToolJumpTrace || v.Body != ToolBodyText {
		t.Fatalf("subagent without a child: %+v", v)
	}
}

func TestToolViewWithoutDisplayIsOther(t *testing.T) {
	// An old Alva event has neither arguments, a display nor a result.
	legacy := toolBlock(t, "read_file_legacy", nil, nil, "Native tool result retained in session records; open its qualified reference to inspect.", false)
	if v := legacy.Tool; v.Kind != engine.ToolOther || v.Name != "read_file_legacy" || v.Body != ToolBodyText || v.Jump != ToolJumpTrace || legacy.Text == "" {
		t.Fatalf("legacy tool: %+v", v)
	}
	// A display on the start alone survives an end without one.
	kept := toolBlock(t, "x", &engine.ToolDisplay{Kind: engine.ToolRead, Path: "a.go"}, nil, "done", false)
	if v := kept.Tool; v.Kind != engine.ToolRead || v.Path != "a.go" {
		t.Fatalf("start display lost: %+v", v)
	}
	// A truncated result keeps its display and the truncation mark.
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(hub.HistoryEntry{Sequence: 1, AttemptID: "a", Event: engine.Event{Kind: engine.ToolEnd, ID: "c", Name: "x", Text: strings.Repeat("o", 64), Truncated: true, Display: &engine.ToolDisplay{Kind: engine.ToolCommand, Command: "cat big.log"}}}))
	if block := blockByID(t, w, "a/c"); !block.Truncated || block.Tool.Command != "cat big.log" {
		t.Fatalf("truncated result: %+v", block)
	}
}

func TestToolViewTodoBlocksAndSupersededUpdates(t *testing.T) {
	plan := func(id string, todos ...engine.TodoItem) engine.Event {
		return engine.Event{Kind: engine.ToolStart, ID: id, Name: "planner", Display: &engine.ToolDisplay{Kind: engine.ToolTodo, Todos: todos}}
	}
	first := plan("todo-1", engine.TodoItem{Text: "read code", Status: engine.TodoCompleted}, engine.TodoItem{Text: "write", Status: engine.TodoInProgress})
	second := plan("todo-2", engine.TodoItem{Text: "read code", Status: engine.TodoCompleted}, engine.TodoItem{Text: "write", Status: engine.TodoCompleted})
	other := plan("todo-3", engine.TodoItem{Text: "ship", Status: engine.TodoPending})
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(hub.HistoryEntry{Sequence: 1, TurnID: "turn-1", AttemptID: "attempt-1", Event: first}))
	published := blockByID(t, w, "attempt-1/todo-1").Tool
	if published.Kind != engine.ToolTodo || len(published.Todos) != 2 || published.Superseded {
		t.Fatalf("first todo: %+v", published)
	}
	w.ApplyNewer(chunkOf(
		hub.HistoryEntry{Sequence: 2, TurnID: "turn-1", AttemptID: "attempt-1", Event: second},
		hub.HistoryEntry{Sequence: 3, TurnID: "turn-2", AttemptID: "attempt-2", Event: other},
	))
	if old := blockByID(t, w, "attempt-1/todo-1").Tool; !old.Superseded || published.Superseded {
		t.Fatal("earlier todo of the Turn was not superseded, or a published value was mutated", old.Superseded, published.Superseded)
	}
	if latest := blockByID(t, w, "attempt-1/todo-2").Tool; latest.Superseded || latest.Todos[1].Status != engine.TodoCompleted {
		t.Fatalf("latest todo of the Turn: %+v", latest)
	}
	if next := blockByID(t, w, "attempt-2/todo-3").Tool; next.Superseded {
		t.Fatalf("other Turn's todo: %+v", next)
	}
	older := NewTranscriptWindow(0)
	older.Reset(chunkOf(hub.HistoryEntry{Sequence: 2, TurnID: "turn-1", AttemptID: "attempt-1", Event: second}))
	older.ApplyOlder(chunkOf(hub.HistoryEntry{Sequence: 1, TurnID: "turn-1", AttemptID: "attempt-1", Event: first}))
	if !blockByID(t, older, "attempt-1/todo-1").Tool.Superseded || blockByID(t, older, "attempt-1/todo-2").Tool.Superseded {
		t.Fatal("paging back did not supersede the earlier todo")
	}
}

func TestToolViewMergesAcrossPageEdgeAndCountsBytes(t *testing.T) {
	start := hub.HistoryEntry{Sequence: 1, TurnID: "turn", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "call", Name: "r", Display: &engine.ToolDisplay{Kind: engine.ToolRead, Path: "docs/notes.md"}}}
	end := hub.HistoryEntry{Sequence: 2, TurnID: "turn", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Name: "r", Text: "{}", Display: &engine.ToolDisplay{Kind: engine.ToolRead, Output: "alva-artifact://notes"}}}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(end))
	if v := blockByID(t, w, "attempt/call").Tool; v.Path != "" || v.Output != "alva-artifact://notes" || v.Jump != ToolJumpTrace {
		t.Fatalf("end-only part: %+v", v)
	}
	w.ApplyOlder(chunkOf(start))
	if v := blockByID(t, w, "attempt/call").Tool; v.Path != "docs/notes.md" || v.Output != "alva-artifact://notes" || v.Body != ToolBodyReference || v.Jump != ToolJumpFile {
		t.Fatalf("merged call: %+v", v)
	}
	// An older display fills a current part that has none.
	bare := NewTranscriptWindow(0)
	bare.Reset(chunkOf(hub.HistoryEntry{Sequence: 2, AttemptID: "a", Event: engine.Event{Kind: engine.ToolEnd, ID: "c", Name: "x", Text: "ok"}}))
	bare.ApplyOlder(chunkOf(hub.HistoryEntry{Sequence: 1, AttemptID: "a", Event: engine.Event{Kind: engine.ToolStart, ID: "c", Name: "x", Display: &engine.ToolDisplay{Kind: engine.ToolCommand, Command: "ls"}}}))
	if v := blockByID(t, bare, "a/c").Tool; v.Kind != engine.ToolCommand || v.Command != "ls" {
		t.Fatalf("older display did not fill a display-less part: %+v", v)
	}
	// A diff is accounted against the window's byte bound.
	diff := &engine.ToolDisplay{Kind: engine.ToolEdit, Path: "a.go", Diff: &engine.ToolDiff{Old: strings.Repeat("o", 1000), New: strings.Repeat("n", 1000)}}
	small := NewTranscriptWindow(0)
	small.Reset(chunkOf(hub.HistoryEntry{Sequence: 1, AttemptID: "a", Event: engine.Event{Kind: engine.ToolStart, ID: "c", Name: "e"}}))
	large := NewTranscriptWindow(0)
	large.Reset(chunkOf(hub.HistoryEntry{Sequence: 1, AttemptID: "a", Event: engine.Event{Kind: engine.ToolStart, ID: "c", Name: "e", Display: diff}}))
	if large.bytes()-small.bytes() < 2000 {
		t.Fatal("tool view bytes are not counted", small.bytes(), large.bytes())
	}
}

func TestWorkspaceFileTabPinsOnlyRegularFilesInsideTheWorkspace(t *testing.T) {
	s, _ := fixture(t)
	directory, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err = os.MkdirAll(filepath.Join(directory, "src"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(directory, "src", "main.go"), []byte("package main\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "files", directory)
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{"src/main.go", filepath.Join(directory, "src", "main.go"), "./src/../src/main.go"} {
		tab, err := s.WorkspaceFileTab(t.Context(), session.ID, path)
		if err != nil || tab.Kind != "file" || tab.File == nil || tab.File.Path != "src/main.go" || tab.BranchID != session.ActiveBranchID {
			t.Fatal("tool path did not pin the workspace file", path, tab, err)
		}
		tab.ID = "view-1"
		if err = (hub.WorkspaceView{SessionID: session.ID, Tabs: []hub.WorkspaceTab{tab}, Active: tab.ID}).Validate(); err != nil {
			t.Fatal("pinned tab is not a valid saved location", err)
		}
		body, err := s.ReadWorkspaceFile(t.Context(), session.ID, WorkspaceReferenceSelection{ID: tab.File.ID, BranchID: tab.File.BranchID, WorkspaceID: tab.File.WorkspaceID, Path: tab.File.Path, Name: tab.File.Name, Kind: tab.File.Kind, Revision: tab.File.Revision, Bytes: tab.File.Bytes}, 0)
		if err != nil || string(body.Data) != "package main\n" {
			t.Fatal("pinned tab does not read its file", body, err)
		}
	}
	// A tool reporting the workspace through a symlinked spelling (as /tmp is
	// for /private/tmp) still pins the same file.
	link := filepath.Join(t.TempDir(), "workspace-link")
	if err = os.Symlink(directory, link); err != nil {
		t.Fatal(err)
	}
	if tab, err := s.WorkspaceFileTab(t.Context(), session.ID, filepath.Join(link, "src", "main.go")); err != nil || tab.File == nil || tab.File.Path != "src/main.go" {
		t.Fatal("symlinked workspace spelling did not pin the file", tab, err)
	}
	outside := filepath.Join(filepath.Dir(directory), "outside.txt")
	for _, path := range []string{"src", "missing.go", "../outside.txt", outside, ""} {
		if _, err := s.WorkspaceFileTab(t.Context(), session.ID, path); err == nil {
			t.Fatal("pinned a path that is not a regular workspace file", path)
		}
	}
}
