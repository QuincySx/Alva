// INPUT: context, errors and bounded UTF-8 product view locations.
// OUTPUT: WorkspaceViewStore, durable resource tabs and shared view validation.
// POS: [L7 Hub product] Stores observation locations separately from execution facts and authority.
package hub

import (
	"context"
	"errors"
	"math"
	"strings"
	"unicode/utf8"
)

var ErrWorkspaceViewConflict = errors.New("workspace view changed in another window; reload before saving")

// WorkspaceFileSource pins a metadata search result. It grants no filesystem
// access; the product reader revalidates its workspace and source identity.
type WorkspaceFileSource struct {
	ID, BranchID, WorkspaceID, Path, Name, Kind, Revision string
	Bytes                                                 int64
}

// WorkspaceTab retains small locations only. Bodies, previews, processes and
// subscriptions never enter the product view store.
type WorkspaceTab struct {
	ID, Kind, Title, BranchID, URI string
	File                           *WorkspaceFileSource `json:",omitempty"`
	TeamID, WorkerID, TaskID       string
	TaskSequence                   uint64
	Offset                         int64
	ScrollX, ScrollY               float32
	Cursor, HighWater              uint64
	Query, After                   string
}

type WorkspaceView struct {
	SessionID          string
	Revision           uint64
	Tabs               []WorkspaceTab
	Active             string
	Collapsed, Focused bool
	// Width is the preferred split-pane width in logical display units;
	// zero uses the Foundation default. Viewport clamping does not alter it.
	Width float32
}

type WorkspaceViewStore interface {
	WorkspaceView(context.Context, string) (WorkspaceView, error)
	SaveWorkspaceView(context.Context, WorkspaceView, uint64) (WorkspaceView, error)
}

func (v WorkspaceView) Validate() error {
	valid := func(s string, max int) bool {
		return len(s) <= max && utf8.ValidString(s) && !strings.ContainsRune(s, 0)
	}
	if v.SessionID == "" || !valid(v.SessionID, 256) || len(v.Tabs) > 12 {
		return errors.New("workspace view needs a session and at most 12 resource tabs")
	}
	if math.IsNaN(float64(v.Width)) || math.IsInf(float64(v.Width), 0) || v.Width != 0 && (v.Width < 320 || v.Width > 960) {
		return errors.New("workspace width must be zero or between 320 and 960 logical display units")
	}
	seen, active := map[string]bool{}, v.Active == ""
	for _, tab := range v.Tabs {
		if tab.ID == "" || seen[tab.ID] || !valid(tab.ID, 256) || tab.BranchID == "" || !valid(tab.BranchID, 256) || !valid(tab.Title, 512) || !valid(tab.URI, 4096) || !valid(tab.Query, 256) || !valid(tab.After, 8<<10) || tab.Offset < 0 || tab.ScrollX < 0 || tab.ScrollY < 0 || tab.ScrollX > 1e7 || tab.ScrollY > 1e7 || math.IsNaN(float64(tab.ScrollX)) || math.IsNaN(float64(tab.ScrollY)) {
			return errors.New("invalid or duplicate workspace resource location")
		}
		seen[tab.ID] = true
		active = active || tab.ID == v.Active
		switch tab.Kind {
		case "files", "artifacts", "inspector", "tasks", "media":
		case "file":
			if tab.File == nil || tab.File.ID == "" || !valid(tab.File.ID, 256) || tab.File.BranchID != tab.BranchID || !valid(tab.File.Path, 4096) || !valid(tab.File.Name, 512) || !valid(tab.File.WorkspaceID, 256) || !valid(tab.File.Revision, 256) || tab.File.Kind != "file" || tab.File.Bytes < 0 {
				return errors.New("file tab needs a pinned file source")
			}
		case "artifact", "record":
			if tab.URI == "" {
				return errors.New("evidence tab needs a reference URI")
			}
		case "task":
			if tab.TeamID == "" || tab.WorkerID == "" || tab.TaskID == "" || tab.TaskSequence == 0 {
				return errors.New("task tab needs owned team/worker/task IDs")
			}
		default:
			return errors.New("unsupported workspace resource kind")
		}
		for _, id := range []string{tab.TeamID, tab.WorkerID, tab.TaskID} {
			if !valid(id, 256) {
				return errors.New("invalid task resource identity")
			}
		}
	}
	if !active || len(v.Tabs) > 0 && v.Active == "" {
		return errors.New("active workspace tab is missing")
	}
	return nil
}

func CloneWorkspaceView(v WorkspaceView) WorkspaceView {
	v.Tabs = append([]WorkspaceTab(nil), v.Tabs...)
	for i := range v.Tabs {
		if v.Tabs[i].File != nil {
			file := *v.Tabs[i].File
			v.Tabs[i].File = &file
		}
	}
	return v
}
