// INPUT: Product-private evidence index and explicit proposed original/new/patch byte count
// OUTPUT: Bounded retained evidence admission without deleting or rewriting any existing snapshot
// POS: [L7 Hub product] Native disk admission cap, not an OS quota or permission to silently discard recovery evidence
package worktree

import (
	"encoding/json"
	"errors"
	"io"
	"path/filepath"
)

func (m *Manager) evidenceBudget(additional int) error {
	parent, err := m.root.OpenRoot("evidence")
	if err != nil {
		return err
	}
	defer parent.Close()
	directory, err := parent.Open(".")
	if err != nil {
		return err
	}
	defer directory.Close()
	names, err := directory.Readdirnames(34)
	if err != nil && !errors.Is(err, io.EOF) {
		return err
	}
	count, total := 0, additional
	for _, name := range names {
		if !validEvidence(name) {
			continue
		} // Only our current temporary build is expected here.
		count++
		if count >= 32 {
			return errors.New("32 retained worktree evidence snapshots reached; explicit archive management is required, nothing was deleted")
		}
		root, base, err := pinParent(parent, filepath.Join(name, "index.json"), false)
		if err != nil {
			return err
		}
		info, err := root.Lstat(base)
		if err != nil || !info.Mode().IsRegular() || info.Size() > 128<<10 {
			root.Close()
			return errors.New("private evidence index is invalid")
		}
		file, err := root.Open(base)
		if err != nil {
			root.Close()
			return err
		}
		data, readErr := io.ReadAll(io.LimitReader(file, 128<<10+1))
		err = errors.Join(readErr, file.Close(), root.Close())
		if err != nil || len(data) > 128<<10 {
			return errors.New("private evidence index exceeds budget")
		}
		var budget struct {
			SnapshotBytes int `json:"snapshot_bytes"`
			PatchBytes    int `json:"patch_bytes"`
		}
		if json.Unmarshal(data, &budget) != nil || budget.SnapshotBytes < 0 || budget.SnapshotBytes > 64<<20 || budget.PatchBytes < 1 || budget.PatchBytes > 16<<20 {
			return errors.New("private evidence byte accounting invalid")
		}
		total += budget.SnapshotBytes + budget.PatchBytes
	}
	if len(names) >= 34 || total > 256<<20 {
		return errors.New("retained worktree evidence exceeds 32 snapshots/256MiB; explicit archive management required, no cleanup was performed")
	}
	return nil
}
