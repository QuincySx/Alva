// INPUT: Saved Hub workspace/branch, pinned filesystem reads, security Guard and owner-scoped artifact contracts
// OUTPUT: Bounded project path search, stable source selections and immutable branch-pinned draft references
// POS: [L7 Hub product] References retain selected originals or directory metadata without granting filesystem access or claiming model delivery
package product

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

var ErrWorkspaceReferenceChanged = errors.New("workspace selection changed; search and select it again")

const (
	maxDraftReferenceCount        = 8
	maxDraftReferenceBytes        = 40 << 20
	maxWorkspaceIndexNodes        = 4096
	maxWorkspaceDirectoryNodes    = 256
	maxWorkspaceDirectoryBytes    = 256 << 10
	maxWorkspaceReferencePageSize = 50
)

// Revision identifies observed metadata. Info.SHA256 in a prepared draft binds
// the actual retained file bytes or directory manifest, independently of it.
type WorkspaceReferenceSelection struct {
	ID          string `json:"id"`
	BranchID    string `json:"branch_id"`
	WorkspaceID string `json:"workspace_id"`
	Path        string `json:"path"`
	Name        string `json:"name"`
	Kind        string `json:"kind"`
	Revision    string `json:"revision"`
	Bytes       int64  `json:"bytes"`
}

type WorkspaceReferencePage struct {
	Entries   []WorkspaceReferenceSelection `json:"entries"`
	Next      string                        `json:"next"`
	HasMore   bool                          `json:"has_more"`
	Truncated bool                          `json:"truncated"`
	Scanned   int                           `json:"scanned"`
}

type DraftWorkspaceReference struct {
	DraftID   string                      `json:"draft_id"`
	Selection WorkspaceReferenceSelection `json:"selection"`
	Info      storage.ArtifactInfo        `json:"info"`
	Reference wire.Reference              `json:"reference"`
}

func (d DraftWorkspaceReference) DraftSelection() DraftReferenceSelection {
	return DraftReferenceSelection{Kind: "workspace", ID: d.DraftID, SHA256: d.Info.SHA256}
}

type workspaceReferenceCursor struct {
	WorkspaceID, BranchID, Query, Revision, After string
}

func workspaceReferenceHash(value string) string {
	hash := sha256.Sum256([]byte(value))
	return hex.EncodeToString(hash[:])
}

func workspaceReferenceScope(id, directory string) string {
	return workspaceReferenceHash(id + "\x00" + directory)
}

func workspaceReferencePath(path string) bool {
	return path != "" && len(path) <= 4096 && utf8.ValidString(path) && !strings.ContainsAny(path, "\\\x00") && filepath.IsLocal(path) && filepath.ToSlash(filepath.Clean(path)) == path
}

func referenceSelection(id string, branch hub.Branch, directory, path string, info os.FileInfo) WorkspaceReferenceSelection {
	kind, size := "file", info.Size()
	if info.IsDir() {
		kind, size = "directory", 0
	}
	scope := workspaceReferenceScope(id, directory)
	name := filepath.Base(path)
	if path == "." {
		name = filepath.Base(directory)
	}
	return WorkspaceReferenceSelection{
		ID: workspaceReferenceHash(scope + "\x00" + path), BranchID: branch.ID, WorkspaceID: scope,
		Path: path, Name: name, Kind: kind, Bytes: size,
		Revision: workspaceReferenceHash(fmt.Sprintf("%s\x00%d\x00%d\x00%d", path, info.Size(), info.ModTime().UnixNano(), info.Mode())),
	}
}

// openReferenceWorkspace uses only the saved workspace. A client path never
// creates an additional grant, and symlink ancestors cannot retarget that root.
func openReferenceWorkspace(directory string) (*workspace.Workspace, *security.Guard, *os.Root, error) {
	canonical, err := filepath.EvalSymlinks(directory)
	if err != nil || canonical != directory {
		return nil, nil, nil, errors.New("saved workspace must remain a canonical real directory")
	}
	w, err := workspace.OpenWorkspace(directory)
	if err != nil {
		return nil, nil, nil, err
	}
	guard, err := security.NewGuard(directory, nil, w, nil)
	if err != nil {
		w.Close()
		return nil, nil, nil, err
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		w.Close()
		return nil, nil, nil, err
	}
	return w, guard, root, nil
}

// scanReferencePaths reads names and metadata, never file bodies. Every
// directory read has the remaining node budget, including very wide roots.
func scanReferencePaths(ctx context.Context, id string, branch hub.Branch, directory, start string, root *os.Root, guard *security.Guard, budget int) (WorkspaceReferencePage, error) {
	var page WorkspaceReferencePage
	queue := []string{start}
	for len(queue) > 0 {
		if err := ctx.Err(); err != nil {
			return page, err
		}
		path := queue[0]
		queue = queue[1:]
		if !workspaceReferencePath(path) {
			continue
		}
		if _, err := guard.CheckPath(ctx, path, abi.AccessRead); err != nil {
			if ctx.Err() != nil {
				return page, ctx.Err()
			}
			continue
		}
		info, err := root.Lstat(filepath.FromSlash(path))
		if err != nil {
			return page, err
		}
		if info.Mode()&os.ModeSymlink != 0 || (!info.IsDir() && !info.Mode().IsRegular()) {
			continue
		}
		page.Entries = append(page.Entries, referenceSelection(id, branch, directory, path, info))
		page.Scanned++
		if !info.IsDir() {
			continue
		}
		if path != start && (info.Name() == ".git" || info.Name() == "node_modules") {
			page.Truncated = true
			continue
		}
		remaining := budget - page.Scanned - len(queue)
		if remaining <= 0 {
			page.Truncated = true
			continue
		}
		file, err := root.Open(filepath.FromSlash(path))
		if err != nil {
			return page, err
		}
		entries, readErr := file.ReadDir(remaining + 1)
		closeErr := file.Close()
		if readErr != nil && !errors.Is(readErr, io.EOF) {
			return page, errors.Join(readErr, closeErr)
		}
		if closeErr != nil {
			return page, closeErr
		}
		if len(entries) > remaining {
			entries = entries[:remaining]
			page.Truncated = true
		}
		for _, entry := range entries {
			if entry.Type()&os.ModeSymlink == 0 {
				queue = append(queue, filepath.ToSlash(filepath.Join(path, entry.Name())))
			}
		}
	}
	sort.Slice(page.Entries, func(i, j int) bool { return page.Entries[i].Path < page.Entries[j].Path })
	return page, nil
}

// SearchWorkspaceReferences searches a bounded metadata index of the current
// saved project. Cursors bind the query, branch and observed index revision;
// changed indexes reject pagination rather than silently skip or repeat paths.
func (s *HubService) SearchWorkspaceReferences(ctx context.Context, id, query, after string, limit int) (WorkspaceReferencePage, error) {
	if len(query) > 256 || !utf8.ValidString(query) || strings.ContainsRune(query, 0) || len(after) > 8<<10 || limit < 1 || limit > maxWorkspaceReferencePageSize {
		return WorkspaceReferencePage{}, errors.New("UTF-8 query <=256 bytes, bounded cursor and page size 1..50 required")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return WorkspaceReferencePage{}, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return WorkspaceReferencePage{}, err
	}
	w, guard, root, err := openReferenceWorkspace(detail.Session.Workspace)
	if err != nil {
		return WorkspaceReferencePage{}, err
	}
	defer w.Close()
	defer root.Close()
	index, err := scanReferencePaths(ctx, id, branch, detail.Session.Workspace, ".", root, guard, maxWorkspaceIndexNodes)
	if err != nil {
		return WorkspaceReferencePage{}, err
	}
	encoded, _ := json.Marshal(index.Entries)
	cursor := workspaceReferenceCursor{WorkspaceID: workspaceReferenceScope(id, detail.Session.Workspace), BranchID: branch.ID, Query: query, Revision: workspaceReferenceHash(string(encoded))}
	if after != "" {
		data, err := base64.RawURLEncoding.DecodeString(after)
		var previous workspaceReferenceCursor
		if err != nil || json.Unmarshal(data, &previous) != nil || previous.WorkspaceID != cursor.WorkspaceID || previous.BranchID != cursor.BranchID || previous.Query != cursor.Query || previous.Revision != cursor.Revision || !workspaceReferencePath(previous.After) {
			return WorkspaceReferencePage{}, ErrWorkspaceReferenceChanged
		}
		cursor.After = previous.After
	}
	page := WorkspaceReferencePage{Scanned: index.Scanned, Truncated: index.Truncated}
	for _, entry := range index.Entries {
		if entry.Path <= cursor.After || !strings.Contains(strings.ToLower(entry.Path), strings.ToLower(query)) {
			continue
		}
		if len(page.Entries) == limit {
			page.HasMore = true
			break
		}
		page.Entries = append(page.Entries, entry)
	}
	if page.HasMore {
		cursor.After = page.Entries[len(page.Entries)-1].Path
		encoded, _ := json.Marshal(cursor)
		page.Next = base64.RawURLEncoding.EncodeToString(encoded)
	}
	return page, ctx.Err()
}

// PrepareWorkspaceReference retains a file's checked original bytes, or a
// bounded directory metadata manifest. Neither selection nor preparation grants
// a model filesystem permission or claims its contents were delivered.
func (s *HubService) PrepareWorkspaceReference(ctx context.Context, id string, selection WorkspaceReferenceSelection) (DraftWorkspaceReference, error) {
	var result DraftWorkspaceReference
	if !workspaceReferencePath(selection.Path) {
		return result, errors.New("canonical project-relative reference path required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return result, err
	}
	if hub.BlocksInput(detail.Session.Status) {
		return result, errors.New("finish or stop the Turn before preparing references")
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return result, err
	}
	if branch.Target.EngineID != "alva" {
		return result, errors.New("workspace artifact references are available only on Alva branches")
	}
	if selection.BranchID != branch.ID || selection.WorkspaceID != workspaceReferenceScope(id, detail.Session.Workspace) {
		return result, ErrWorkspaceReferenceChanged
	}
	count, total := s.draftReferenceBudgetLocked(id)
	if count >= maxDraftReferenceCount {
		return result, errors.New("shared session staging allows at most 8 files or project references")
	}
	w, guard, root, err := openReferenceWorkspace(detail.Session.Workspace)
	if err != nil {
		return result, err
	}
	defer w.Close()
	defer root.Close()
	if _, err = guard.CheckPath(ctx, selection.Path, abi.AccessRead); err != nil {
		return result, err
	}
	before, err := root.Lstat(filepath.FromSlash(selection.Path))
	if err != nil {
		return result, err
	}
	if !before.Mode().IsRegular() && !before.IsDir() || before.Mode()&os.ModeSymlink != 0 || referenceSelection(id, branch, detail.Session.Workspace, selection.Path, before) != selection {
		return result, ErrWorkspaceReferenceChanged
	}
	input := storage.ArtifactInput{Kind: "workspace-reference", Name: selection.Name}
	if selection.Kind == "file" {
		snapshot, err := w.ReadSnapshot(ctx, selection.Path, guard)
		if err != nil {
			return result, err
		}
		input.Data = snapshot.Data
		input.MediaType = http.DetectContentType(snapshot.Data)
		input.Text = utf8.Valid(snapshot.Data) && (strings.HasPrefix(input.MediaType, "text/") || strings.HasPrefix(input.MediaType, "application/json"))
		input.Processing = "Checked project file original bytes saved with SHA-256. No parsing, compression, OCR or media understanding performed. Metadata/reference only; original bytes not delivered to the model. Selection grants no filesystem access."
	} else {
		index, err := scanReferencePaths(ctx, id, branch, detail.Session.Workspace, selection.Path, root, guard, maxWorkspaceDirectoryNodes)
		if err != nil {
			return result, err
		}
		input.Data, err = json.Marshal(index)
		if err != nil {
			return result, err
		}
		if len(input.Data) > maxWorkspaceDirectoryBytes {
			return result, errors.New("directory reference metadata exceeds 256 KiB; select a smaller directory")
		}
		input.MediaType, input.Text = "application/json", true
		input.Processing = "Bounded project directory names/metadata manifest saved with SHA-256; not an atomic or complete directory snapshot. File bodies were not read, copied or delivered to the model. Sensitive paths and symlinks excluded. Selection grants no directory read or write permissions."
	}
	after, err := root.Lstat(filepath.FromSlash(selection.Path))
	if err != nil || !os.SameFile(before, after) || referenceSelection(id, branch, detail.Session.Workspace, selection.Path, after) != selection {
		return result, ErrWorkspaceReferenceChanged
	}
	if total+int64(len(input.Data)) > maxDraftReferenceBytes {
		return result, errors.New("files and project references exceed the shared 40 MiB session staging budget")
	}
	// Re-read persisted routing before any artifact/binding write. Concurrent
	// changes can park a prepared draft, but can never submit it on a new branch.
	current, err := s.Store.Detail(ctx, id)
	if err != nil {
		return result, err
	}
	if current.Session.ActiveBranchID != branch.ID || current.Session.Workspace != detail.Session.Workspace || hub.BlocksInput(current.Session.Status) {
		return result, ErrWorkspaceReferenceChanged
	}
	branch, err = ActiveBranch(current)
	if err != nil {
		return result, err
	}
	if branch.NativeSessionID == "" {
		branch.NativeSessionID, err = hub.NewID("native-")
		if err != nil {
			return result, err
		}
		if _, err = s.Repository.Create(ctx, id, branch.NativeSessionID, ""); err != nil {
			return result, err
		}
		if err = s.Store.BindNative(ctx, id, branch.ID, branch.NativeSessionID); err != nil {
			return result, err
		}
	}
	info, err := s.Repository.PutArtifact(ctx, id, branch.NativeSessionID, input)
	if err != nil {
		return result, err
	}
	draftID, err := hub.NewID("draft-")
	if err != nil {
		return result, err
	}
	result = DraftWorkspaceReference{DraftID: draftID, Selection: selection, Info: info, Reference: info.Reference()}
	s.draftMu.Lock()
	if s.workspaceReferences == nil {
		s.workspaceReferences = make(map[string][]DraftWorkspaceReference)
	}
	s.workspaceReferences[id] = append(s.workspaceReferences[id], result)
	s.draftMu.Unlock()
	return result, nil
}

func (s *HubService) DraftWorkspaceReferences(id string) []DraftWorkspaceReference {
	s.draftMu.RLock()
	defer s.draftMu.RUnlock()
	return append([]DraftWorkspaceReference(nil), s.workspaceReferences[id]...)
}

func (s *HubService) RemoveDraftWorkspaceReference(id, draftID string) {
	s.draftMu.Lock()
	defer s.draftMu.Unlock()
	for i, reference := range s.workspaceReferences[id] {
		if reference.DraftID == draftID {
			s.workspaceReferences[id] = append(s.workspaceReferences[id][:i], s.workspaceReferences[id][i+1:]...)
			return
		}
	}
}

func (s *HubService) draftReferenceBudgetLocked(id string) (count int, bytes int64) {
	s.draftMu.RLock()
	defer s.draftMu.RUnlock()
	for _, attachment := range s.attachments[id] {
		count++
		bytes += attachment.Info.Bytes
	}
	for _, reference := range s.workspaceReferences[id] {
		count++
		bytes += reference.Info.Bytes
	}
	return
}

func (s *HubService) workspaceReferencePromptLocked(id string, branch hub.Branch, directory, prompt string, references []DraftWorkspaceReference) (string, error) {
	if len(references) == 0 {
		return prompt, nil
	}
	for _, reference := range references {
		if reference.Selection.BranchID != branch.ID || reference.Selection.WorkspaceID != workspaceReferenceScope(id, directory) || reference.Info.SessionID != branch.NativeSessionID {
			return "", errors.New("project references belong to a different branch or workspace; return to it or remove the draft references")
		}
	}
	manifest, err := json.Marshal(references)
	if err != nil {
		return "", err
	}
	return prompt + "\n\n[User-selected project references: frozen source identity/scope and artifact manifests only. Original file bytes or bounded directory metadata were saved; not delivered to the model. Directory references grant no filesystem permissions. Read only relevant authorized artifact ranges.]\n" + string(manifest), nil
}
