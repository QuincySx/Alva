// INPUT: Pinned Workspace, explicit path Guard and bounded literal query
// OUTPUT: SearchResult with bounded UTF-8 snippets, actual scan counts and truncation
// POS: [host capability] Searches only granted regular files without model references or tool-result policy.
package workspace

import (
	"bytes"
	"context"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type SearchMatch struct {
	Path   string `json:"path"`
	Offset int    `json:"offset"`
	Text   string `json:"text"`
}
type SearchResult struct {
	Matches      []SearchMatch `json:"matches"`
	Truncated    bool          `json:"truncated"`
	ScannedFiles int           `json:"scanned_files"`
	ScannedBytes int           `json:"scanned_bytes"`
	Literal      bool          `json:"literal"`
}

func (w *Workspace) Search(ctx context.Context, path, query string, guard *security.Guard) (SearchResult, error) {
	if query == "" || len(query) > 1024 || !utf8.ValidString(query) {
		return SearchResult{}, errors.New("Search requires a nonempty UTF-8 literal query of at most 1024 bytes")
	}
	root, relative, err := w.authorize(ctx, path, abi.AccessRead, guard)
	if err != nil {
		return SearchResult{}, err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	var matches []SearchMatch
	nodes, scanned, files := 0, 0, 0
	truncated := false
	stop := errors.New("search budget reached")
	err = fs.WalkDir(root.root.FS(), filepath.ToSlash(relative), func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		nodes++
		if nodes > 10000 {
			truncated = true
			return stop
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if entry.IsDir() {
			if entry.Name() == ".git" || entry.Name() == "node_modules" {
				truncated = true
				return fs.SkipDir
			}
			return nil
		}
		absolute := filepath.Join(root.path, filepath.FromSlash(path))
		if _, err := guard.CheckPath(ctx, absolute, abi.AccessRead); err != nil {
			return nil
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() || info.Size() > 1<<20 {
			truncated = true
			return nil
		}
		if scanned+int(info.Size()) > 16<<20 {
			truncated = true
			return stop
		}
		data, err := readFile(ctx, root.root, filepath.FromSlash(path))
		if err != nil {
			return err
		}
		scanned += len(data)
		files++
		if !utf8.Valid(data) {
			return nil
		}
		for offset := 0; offset < len(data); {
			index := bytes.Index(data[offset:], []byte(query))
			if index < 0 {
				break
			}
			index += offset
			end := index + 256
			if end > len(data) {
				end = len(data)
			}
			for end < len(data) && end > index && !utf8.RuneStart(data[end]) {
				end--
			}
			matches = append(matches, SearchMatch{path, index, string(data[index:end])})
			if len(matches) >= 20 {
				truncated = true
				return stop
			}
			offset = index + len(query)
		}
		return nil
	})
	if err != nil && !errors.Is(err, stop) {
		return SearchResult{}, err
	}
	return SearchResult{Matches: matches, Truncated: truncated, ScannedFiles: files, ScannedBytes: scanned, Literal: true}, nil
}
