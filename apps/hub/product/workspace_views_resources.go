// INPUT: Saved product branches, pinned workspace search selections and shared native evidence readers.
// OUTPUT: Bounded actual file/artifact/task reads, cold original indexes and tool-path file tab pinning for workspace tabs.
// POS: [L7 Hub product] Revalidates resource ownership on every read without restoring or replaying execution.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

const WorkspaceResourcePageBytes = 4096

type WorkspaceResourceBody struct {
	Title, MediaType, Processing, SHA256 string
	Data                                 []byte
	Offset, NextOffset, TotalBytes       int64
	HasMore                              bool
}

type WorkspaceResourceEntry struct {
	Location hub.WorkspaceTab
	Detail   string
}

type WorkspaceResourcePage struct {
	Entries         []WorkspaceResourceEntry
	Body            WorkspaceResourceBody
	Next, HighWater uint64
	After           string
	HasMore         bool
	Processing      string
}

func workspaceResourceLabel(value string) string {
	if len(value) <= 160 {
		return value
	}
	value = value[:160]
	for !utf8.ValidString(value) {
		value = value[:len(value)-1]
	}
	return value + "…"
}

// WorkspaceResource is a cold, bounded observation. The independent view owner
// tracks every actual read through completion so application shutdown can join
// it before releasing the supplied repositories.
func (s *HubService) WorkspaceResource(ctx context.Context, id string, tab hub.WorkspaceTab) (WorkspaceResourcePage, error) {
	var result WorkspaceResourcePage
	if s.WorkspaceViews == nil {
		return result, errors.New("workspace view service is unavailable")
	}
	ctx, end, err := s.WorkspaceViews.begin(ctx)
	if err != nil {
		return result, err
	}
	defer end()
	if err = (hub.WorkspaceView{SessionID: id, Tabs: []hub.WorkspaceTab{tab}, Active: tab.ID}).Validate(); err != nil {
		return result, err
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return result, err
	}
	owned := false
	for _, branch := range detail.Branches {
		owned = owned || branch.ID == tab.BranchID
	}
	if !owned {
		return result, hub.ErrNotFound
	}
	switch tab.Kind {
	case "file":
		f := tab.File
		result.Body, err = s.ReadWorkspaceFile(ctx, id, WorkspaceReferenceSelection{ID: f.ID, BranchID: f.BranchID, WorkspaceID: f.WorkspaceID, Path: f.Path, Name: f.Name, Kind: f.Kind, Revision: f.Revision, Bytes: f.Bytes}, tab.Offset)
	case "artifact":
		result.Body, err = s.ReadWorkspaceArtifact(ctx, id, tab.BranchID, tab.URI, tab.Offset)
	case "record":
		page, e := s.InspectorRead(ctx, id, tab.BranchID, tab.URI, tab.Offset)
		err = e
		result.Body = WorkspaceResourceBody{Title: tab.Title, MediaType: "text/plain", Data: []byte(page.Text), Offset: page.Offset, NextOffset: page.NextOffset, TotalBytes: page.TotalBytes, HasMore: page.Truncated, Processing: "Explicit bounded committed record range; no Agent restore or execution."}
	case "task":
		result.Body, err = s.WorkspaceTask(ctx, id, tab.TeamID, tab.WorkerID, tab.TaskID, tab.TaskSequence, tab.Offset)
	case "files":
		if detail.Session.ActiveBranchID != tab.BranchID {
			return result, ErrWorkspaceReferenceChanged
		}
		page, e := s.SearchWorkspaceReferences(ctx, id, tab.Query, tab.After, 16)
		err = e
		result.After, result.HasMore = page.Next, page.HasMore
		result.Processing = "Project names and metadata only. Click a file to explicitly read its selected 4 KiB range."
		if page.Truncated {
			result.Processing += " The bounded metadata scan is partial; refine the query."
		}
		for _, f := range page.Entries {
			if f.Kind == "file" {
				file := hub.WorkspaceFileSource{ID: f.ID, BranchID: f.BranchID, WorkspaceID: f.WorkspaceID, Path: f.Path, Name: f.Name, Kind: f.Kind, Revision: f.Revision, Bytes: f.Bytes}
				result.Entries = append(result.Entries, WorkspaceResourceEntry{Location: hub.WorkspaceTab{Kind: "file", Title: f.Name, BranchID: tab.BranchID, File: &file}, Detail: f.Path})
			}
		}
	case "artifacts", "media":
		var page storage.ArtifactIndexPage
		if tab.Kind == "media" {
			if _, err = s.workspaceArtifactBranch(ctx, id, tab.BranchID); err != nil {
				return result, err
			}
			page, err = s.MediaSources(ctx, id, tab.BranchID, tab.Cursor, tab.HighWater)
		} else {
			page, err = s.WorkspaceArtifacts(ctx, id, tab.BranchID, tab.Cursor, tab.HighWater)
		}
		result.Next, result.HighWater, result.HasMore = page.Next, page.HighWater, page.HasMore
		result.Processing = "Committed immutable originals and evidence. Click to read an actual bounded range; binary content is shown as bytes."
		for _, entry := range page.Entries {
			result.Entries = append(result.Entries, WorkspaceResourceEntry{Location: hub.WorkspaceTab{Kind: "artifact", Title: entry.Info.Name, BranchID: tab.BranchID, URI: entry.Info.Reference().URI}, Detail: entry.Info.MediaType + " · " + entry.Info.Kind})
		}
	case "inspector":
		page, e := s.Inspector(ctx, id, tab.BranchID, tab.Query, tab.Cursor, tab.HighWater)
		err = e
		result.Next, result.HighWater, result.HasMore = page.Next, page.HighWater, page.HasMore
		result.Processing = page.Processing
		for _, entry := range page.Entries {
			if entry.Value.URI != "" {
				title := strings.TrimSpace(entry.Namespace + " / " + entry.Type)
				result.Entries = append(result.Entries, WorkspaceResourceEntry{Location: hub.WorkspaceTab{Kind: "record", Title: workspaceResourceLabel(title), BranchID: tab.BranchID, URI: entry.Value.URI}, Detail: workspaceResourceLabel(entry.Summary)})
			}
		}
	case "tasks":
		if s.Teams == nil {
			return result, errors.New("task service is unavailable")
		}
		team, e := s.Teams.Store.TeamForLead(ctx, id)
		if e != nil {
			return result, e
		}
		result.Processing = "Committed collaboration tasks. Opening, switching and closing tabs never resumes or stops a worker."
		if tab.WorkerID == "" {
			for _, worker := range team.Workers {
				result.Entries = append(result.Entries, WorkspaceResourceEntry{Location: hub.WorkspaceTab{Kind: "tasks", Title: workspaceResourceLabel(worker.Name), BranchID: tab.BranchID, TeamID: team.Team.ID, WorkerID: worker.ID}, Detail: string(worker.Lifecycle)})
			}
		} else {
			if tab.TeamID != team.Team.ID {
				return result, hub.ErrNotFound
			}
			page, e := s.Teams.Store.WorkerTasks(ctx, id, tab.TeamID, tab.WorkerID, tab.Cursor, 16)
			err = e
			result.Next, result.HasMore = page.Next, page.HasMore
			for _, task := range page.Tasks {
				result.Entries = append(result.Entries, WorkspaceResourceEntry{Location: hub.WorkspaceTab{Kind: "task", Title: workspaceResourceLabel(task.Scope), BranchID: tab.BranchID, TeamID: tab.TeamID, WorkerID: tab.WorkerID, TaskID: task.ID, TaskSequence: task.Sequence}, Detail: string(task.Status)})
			}
		}
	}
	if err == nil {
		err = ctx.Err()
	}
	if err != nil {
		return WorkspaceResourcePage{}, err
	}
	return result, nil
}

func (s *HubService) ReadWorkspaceFile(ctx context.Context, id string, selection WorkspaceReferenceSelection, offset int64) (WorkspaceResourceBody, error) {
	if !workspaceReferencePath(selection.Path) || selection.Kind != "file" || offset < 0 {
		return WorkspaceResourceBody{}, errors.New("pinned project file and nonnegative offset required")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	var branch hub.Branch
	for _, b := range detail.Branches {
		if b.ID == selection.BranchID {
			branch = b
		}
	}
	if branch.ID == "" || selection.WorkspaceID != workspaceReferenceScope(id, detail.Session.Workspace) {
		return WorkspaceResourceBody{}, ErrWorkspaceReferenceChanged
	}
	w, guard, root, err := openReferenceWorkspace(detail.Session.Workspace)
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	defer w.Close()
	defer root.Close()
	if _, err = guard.CheckPath(ctx, selection.Path, abi.AccessRead); err != nil {
		return WorkspaceResourceBody{}, err
	}
	before, err := root.Lstat(filepath.FromSlash(selection.Path))
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	if !before.Mode().IsRegular() || referenceSelection(id, branch, detail.Session.Workspace, selection.Path, before) != selection {
		return WorkspaceResourceBody{}, ErrWorkspaceReferenceChanged
	}
	file, err := root.Open(filepath.FromSlash(selection.Path))
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(before, opened) {
		return WorkspaceResourceBody{}, ErrWorkspaceReferenceChanged
	}
	if offset > before.Size() {
		return WorkspaceResourceBody{}, errors.New("file position exceeds current source length")
	}
	data := make([]byte, WorkspaceResourcePageBytes)
	n, err := file.ReadAt(data, offset)
	if err != nil && !errors.Is(err, io.EOF) {
		return WorkspaceResourceBody{}, err
	}
	if err = ctx.Err(); err != nil {
		return WorkspaceResourceBody{}, err
	}
	after, err := root.Lstat(filepath.FromSlash(selection.Path))
	if err != nil || !os.SameFile(before, after) || referenceSelection(id, branch, detail.Session.Workspace, selection.Path, after) != selection {
		return WorkspaceResourceBody{}, ErrWorkspaceReferenceChanged
	}
	if _, err = guard.CheckPath(ctx, selection.Path, abi.AccessRead); err != nil {
		return WorkspaceResourceBody{}, err
	}
	return WorkspaceResourceBody{Title: selection.Path, Data: data[:n], Offset: offset, NextOffset: offset + int64(n), TotalBytes: before.Size(), HasMore: offset+int64(n) < before.Size(), Processing: "Explicit 4 KiB source range; saved project and observed source identity revalidated. No model delivery, command or browser replay."}, nil
}

// WorkspaceFileTab pins the file a tool block names as a resource-panel file
// tab of the session's active branch. The path may be absolute inside the
// saved workspace or relative to it; anything outside it, missing or not a
// regular file is ErrWorkspaceReferenceChanged. Pinning reads metadata only
// and grants nothing: opening the tab revalidates the source as usual.
func (s *HubService) WorkspaceFileTab(ctx context.Context, id, path string) (hub.WorkspaceTab, error) {
	if path == "" || len(path) > 4096 || !utf8.ValidString(path) || strings.ContainsRune(path, 0) {
		return hub.WorkspaceTab{}, errors.New("tool file path must be bounded UTF-8")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return hub.WorkspaceTab{}, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return hub.WorkspaceTab{}, err
	}
	directory := detail.Session.Workspace
	relative := filepath.Clean(path)
	if filepath.IsAbs(relative) {
		// An engine may report a symlink-resolved path (/private/tmp for
		// /tmp) or an unresolved one; compare both sides resolved, falling
		// back to the given spelling when resolution fails.
		root, target := resolvedPath(directory), resolvedPath(relative)
		if relative, err = filepath.Rel(root, target); err != nil {
			return hub.WorkspaceTab{}, ErrWorkspaceReferenceChanged
		}
	}
	relative = filepath.ToSlash(relative)
	if !workspaceReferencePath(relative) {
		return hub.WorkspaceTab{}, ErrWorkspaceReferenceChanged
	}
	w, guard, root, err := openReferenceWorkspace(directory)
	if err != nil {
		return hub.WorkspaceTab{}, err
	}
	defer w.Close()
	defer root.Close()
	if _, err = guard.CheckPath(ctx, relative, abi.AccessRead); err != nil {
		return hub.WorkspaceTab{}, err
	}
	info, err := root.Lstat(filepath.FromSlash(relative))
	if err != nil || !info.Mode().IsRegular() {
		return hub.WorkspaceTab{}, ErrWorkspaceReferenceChanged
	}
	selection := referenceSelection(id, branch, directory, relative, info)
	file := hub.WorkspaceFileSource{ID: selection.ID, BranchID: selection.BranchID, WorkspaceID: selection.WorkspaceID, Path: selection.Path, Name: selection.Name, Kind: selection.Kind, Revision: selection.Revision, Bytes: selection.Bytes}
	return hub.WorkspaceTab{Kind: "file", Title: workspaceResourceLabel(selection.Name), BranchID: branch.ID, File: &file}, ctx.Err()
}

// resolvedPath is path with symlinks resolved, or path itself when it
// cannot be resolved (for example a file that no longer exists).
func resolvedPath(path string) string {
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		return resolved
	}
	return path
}

func (s *HubService) workspaceArtifactBranch(ctx context.Context, id, branchID string) (hub.Branch, error) {
	branch, err := s.boundBranch(ctx, id, branchID)
	if err == nil && branch.Target.EngineID != "alva" {
		err = errors.New("native artifacts are available only on their Alva branch")
	}
	return branch, err
}

func (s *HubService) WorkspaceArtifacts(ctx context.Context, id, branchID string, after, high uint64) (storage.ArtifactIndexPage, error) {
	branch, err := s.workspaceArtifactBranch(ctx, id, branchID)
	if err != nil {
		return storage.ArtifactIndexPage{}, err
	}
	return s.Repository.PageArtifacts(ctx, id, branch.NativeSessionID, "", "", after, high, 16)
}

func (s *HubService) ReadWorkspaceArtifact(ctx context.Context, id, branchID, uri string, offset int64) (WorkspaceResourceBody, error) {
	branch, err := s.workspaceArtifactBranch(ctx, id, branchID)
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	page, err := s.Repository.ReadArtifact(ctx, id, branch.NativeSessionID, uri, offset, WorkspaceResourcePageBytes)
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	return WorkspaceResourceBody{Title: page.Info.Name, MediaType: page.Info.MediaType, SHA256: page.Info.SHA256, Processing: page.Info.Processing, Data: page.Data, Offset: page.Offset, NextOffset: page.NextOffset, TotalBytes: page.Info.Bytes, HasMore: page.Truncated}, nil
}

// WorkspaceTask reads exactly the selected committed queue identity. Paging to
// a task never dispatches the queue or resumes its worker.
func (s *HubService) WorkspaceTask(ctx context.Context, lead, team, worker, task string, sequence uint64, offset int64) (WorkspaceResourceBody, error) {
	if s.Teams == nil || sequence == 0 || offset < 0 {
		return WorkspaceResourceBody{}, errors.New("owned task identity and nonnegative position required")
	}
	page, err := s.Teams.Store.WorkerTasks(ctx, lead, team, worker, sequence-1, 1)
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	if len(page.Tasks) != 1 || page.Tasks[0].ID != task || page.Tasks[0].Sequence != sequence {
		return WorkspaceResourceBody{}, hub.ErrNotFound
	}
	data, err := json.MarshalIndent(page.Tasks[0], "", "  ")
	if err != nil {
		return WorkspaceResourceBody{}, err
	}
	if offset > int64(len(data)) {
		return WorkspaceResourceBody{}, errors.New("task position exceeds current committed result; refresh from start")
	}
	end := min(offset+WorkspaceResourcePageBytes, int64(len(data)))
	return WorkspaceResourceBody{Title: "Task " + task, MediaType: "application/json", Data: append([]byte(nil), data[offset:end]...), Offset: offset, NextOffset: end, TotalBytes: int64(len(data)), HasMore: end < int64(len(data)), Processing: "Committed worker task and actual outcome. Observation does not dispatch, stop or replay this task."}, nil
}
