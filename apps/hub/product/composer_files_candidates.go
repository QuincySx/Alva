// INPUT: Explicit saved project scope, optional absolute path prefix and bounded metadata-only filesystem reads.
// OUTPUT: Fuzzy local file candidates from the project/recent captures or one explicitly named external directory.
// POS: [L7 Hub product] Suggests paths before a session exists without indexing file contents or scanning the whole computer.
package product

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
)

type ComposerFileCandidate struct {
	Path        string                 `json:"path"`
	Name        string                 `json:"name"`
	Kind        string                 `json:"kind"`
	DisplayPath string                 `json:"display_path"`
	Bytes       int64                  `json:"bytes"`
	Revision    string                 `json:"revision"`
	Captured    *ComposerFileSelection `json:"captured,omitempty"`
}
type ComposerFileCandidatePage struct {
	Entries   []ComposerFileCandidate `json:"entries"`
	Scanned   int                     `json:"scanned"`
	Truncated bool                    `json:"truncated"`
}

func composerFuzzyScore(path, query string) int {
	path, query = strings.ToLower(path), strings.ToLower(query)
	if query == "" {
		return 1
	}
	base := filepath.Base(path)
	if base == query {
		return 10000
	}
	if strings.HasPrefix(base, query) {
		return 8000 - len(base)
	}
	if strings.Contains(base, query) {
		return 6000 - len(base)
	}
	if strings.Contains(path, query) {
		return 4000 - len(path)
	}
	runes := []rune(query)
	matched := 0
	gap := 0
	for _, r := range path {
		if matched < len(runes) && r == runes[matched] {
			matched++
		} else if matched > 0 {
			gap++
		}
	}
	if matched == len(runes) {
		return max(2, 2000-gap-len(path))
	}
	return 0
}

// SearchComposerFileCandidates never reads file bodies. Relative queries use a
// bounded project metadata scan plus recent captures in the exact draft scope.
// An absolute query reads only the named path or up to 256 immediate siblings.
func (s *HubService) SearchComposerFileCandidates(ctx context.Context, scope hub.ComposerDraftScope, workspace, query string, limit int) (ComposerFileCandidatePage, error) {
	var page ComposerFileCandidatePage
	if err := scope.Validate(); err != nil {
		return page, err
	}
	if len(query) > 4096 || !utf8.ValidString(query) || strings.ContainsRune(query, 0) || limit < 1 || limit > 50 {
		return page, errors.New("UTF-8 query up to 4096 bytes and candidate limit 1..50 required")
	}
	type ranked struct {
		entry ComposerFileCandidate
		score int
	}
	var matches []ranked
	seen := map[string]bool{}
	add := func(entry ComposerFileCandidate, match string) {
		if seen[entry.Path] {
			return
		}
		score := composerFuzzyScore(entry.DisplayPath, match)
		if score == 0 {
			return
		}
		seen[entry.Path] = true
		matches = append(matches, ranked{entry, score})
	}
	if query == "~" || strings.HasPrefix(query, "~/") {
		home, err := os.UserHomeDir()
		if err != nil {
			return page, err
		}
		if query == "~" {
			query = home
		} else {
			query = filepath.Join(home, query[2:])
		}
	}
	if filepath.IsAbs(query) {
		cleaned := filepath.Clean(query)
		info, err := os.Stat(cleaned)
		if err == nil && info.Mode().IsRegular() {
			page.Scanned = 1
			add(ComposerFileCandidate{Path: cleaned, Name: info.Name(), Kind: "file", DisplayPath: cleaned, Bytes: info.Size(), Revision: workspaceReferenceHash(info.Name() + info.ModTime().String())}, "")
		} else {
			directory, prefix := filepath.Dir(cleaned), filepath.Base(cleaned)
			if err == nil && info.IsDir() {
				directory, prefix = cleaned, ""
			}
			canonical, e := filepath.EvalSymlinks(directory)
			if e != nil {
				return page, e
			}
			root, e := os.OpenRoot(canonical)
			if e != nil {
				return page, e
			}
			defer root.Close()
			file, e := root.Open(".")
			if e != nil {
				return page, e
			}
			entries, e := file.ReadDir(257)
			closeErr := file.Close()
			if e != nil && !errors.Is(e, io.EOF) {
				return page, errors.Join(e, closeErr)
			}
			if closeErr != nil {
				return page, closeErr
			}
			if len(entries) > 256 {
				entries = entries[:256]
				page.Truncated = true
			}
			for _, entry := range entries {
				if err = ctx.Err(); err != nil {
					return page, err
				}
				page.Scanned++
				if !entry.Type().IsRegular() && !entry.IsDir() {
					continue
				}
				info, e := entry.Info()
				if e != nil {
					return page, e
				}
				path := filepath.Join(canonical, entry.Name())
				kind := "file"
				if entry.IsDir() {
					kind = "directory"
				}
				add(ComposerFileCandidate{Path: path, Name: entry.Name(), Kind: kind, DisplayPath: path, Bytes: info.Size(), Revision: workspaceReferenceHash(info.Name() + info.ModTime().String())}, prefix)
			}
		}
	} else {
		if workspace != "" {
			w, guard, root, err := openReferenceWorkspace(workspace)
			if err != nil {
				return page, err
			}
			index, err := scanReferencePaths(ctx, scope.ID, hub.Branch{ID: scope.ID}, workspace, ".", root, guard, maxWorkspaceIndexNodes)
			closeErr := errors.Join(root.Close(), w.Close())
			if err != nil || closeErr != nil {
				return page, errors.Join(err, closeErr)
			}
			page.Scanned = index.Scanned
			page.Truncated = index.Truncated
			for _, entry := range index.Entries {
				if entry.Kind == "file" || entry.Kind == "directory" {
					add(ComposerFileCandidate{Path: filepath.Join(workspace, filepath.FromSlash(entry.Path)), Name: entry.Name, Kind: entry.Kind, DisplayPath: entry.Path, Bytes: entry.Bytes, Revision: entry.Revision}, query)
				}
			}
		}
		store, err := s.composerFileStore()
		if err != nil {
			return page, err
		}
		recent, err := store.ListComposerFiles(ctx, scope, 100)
		if err != nil {
			return page, err
		}
		for _, f := range recent {
			selection := SelectedComposerFile(f)
			entry := ComposerFileCandidate{Path: f.SourcePath, Name: f.Name, Kind: "file", DisplayPath: f.SourcePath, Bytes: f.Bytes, Revision: f.SHA256, Captured: &selection}
			if seen[entry.Path] { // A current project path must capture its current bytes,
				// rather than silently selecting a possibly older recent copy.
				continue
			}
			add(entry, query)
		}
	}
	sort.Slice(matches, func(i, j int) bool {
		if matches[i].score != matches[j].score {
			return matches[i].score > matches[j].score
		}
		return matches[i].entry.DisplayPath < matches[j].entry.DisplayPath
	})
	if len(matches) > limit {
		page.Truncated = true
		matches = matches[:limit]
	}
	for _, match := range matches {
		page.Entries = append(page.Entries, match.entry)
	}
	return page, ctx.Err()
}
