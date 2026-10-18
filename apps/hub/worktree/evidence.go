// INPUT: Managed Worker/base identity, literal file hashes and explicitly requested fixed Git diff command
// OUTPUT: Bounded private patch evidence and permission-preserving checked adoption without commits or resets
// POS: [L7 Hub product] Product owner must hold both workspace-exclusive leases and durable adopting status before Apply
package worktree

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

func validEvidence(id string) bool {
	return strings.HasPrefix(id, "diff-") && len(id) == 37 && !strings.ContainsAny(id, "/\\\x00")
}
func (m *Manager) operation(ctx context.Context) (context.Context, func(), error) {
	end, err := m.begin()
	if err != nil {
		return nil, nil, err
	}
	owned, cancel := context.WithTimeout(m.life, 2*time.Minute)
	stop := context.AfterFunc(ctx, cancel)
	if ctx.Err() != nil {
		cancel()
	}
	select {
	case m.operationGate <- struct{}{}:
	case <-owned.Done():
		stop()
		cancel()
		end()
		return nil, nil, owned.Err()
	}
	if err = m.checkRoot(); err != nil {
		<-m.operationGate
		stop()
		cancel()
		end()
		return nil, nil, err
	}
	return owned, func() { <-m.operationGate; stop(); cancel(); end() }, nil
}
func writeSnapshot(directory string, snapshot fileSnapshot, names []string) error {
	root, err := os.OpenRoot(directory)
	if err != nil {
		return err
	}
	defer root.Close()
	for _, name := range names {
		state, ok := snapshot[name]
		if !ok {
			continue
		}
		if parent := filepath.Dir(name); parent != "." {
			if err = root.MkdirAll(parent, 0700); err != nil {
				return err
			}
		}
		mode := os.FileMode(0600)
		if state.Mode == "100755" {
			mode = 0700
		}
		file, e := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
		if e != nil {
			return e
		}
		_, e = file.Write(state.Data)
		e = errors.Join(e, file.Sync(), file.Close())
		if e != nil {
			return e
		}
	}
	return nil
}

// Preview creates no model prompt and modifies neither shared nor Worker files.
// Patch bytes remain private on disk; the small returned hashes/index are not
// execution success, test evidence, an automatic merge or an adoption approval.
func (m *Manager) Preview(ctx context.Context, h hub.WorktreeBinding) (hub.WorktreeEvidence, error) {
	var evidence hub.WorktreeEvidence
	owned, end, err := m.operation(ctx)
	if err != nil {
		return evidence, err
	}
	defer end()
	entries, err := m.checkedHandle(owned, h)
	if err != nil {
		return evidence, err
	}
	base, err := m.baseSnapshot(owned, h.Workspace, entries)
	if err != nil {
		return evidence, err
	}
	shared, err := m.cleanShared(owned, h, entries, base)
	if err != nil {
		return evidence, err
	}
	worker, err := m.currentSnapshot(owned, h.Path, entries)
	if err != nil {
		return evidence, err
	}
	for _, name := range paths(base, worker) {
		before, a := base[name]
		after, b := worker[name]
		if !sameFile(before, a, after, b) {
			evidence.Changes = append(evidence.Changes, hub.WorktreeChange{Path: name, BeforeSHA256: before.SHA256, AfterSHA256: after.SHA256, BeforeMode: before.Mode, AfterMode: after.Mode})
		}
	}
	if len(evidence.Changes) == 0 {
		return evidence, errors.New("Worker has no literal file changes from its saved base")
	}
	if len(evidence.Changes) > 128 {
		return evidence, errors.New("preview exceeds 128 changed paths; split the task")
	}
	for _, change := range evidence.Changes {
		evidence.SnapshotBytes += len(base[change.Path].Data) + len(worker[change.Path].Data)
	}
	if evidence.SnapshotBytes > 64<<20 {
		return evidence, errors.New("private original/proposed evidence exceeds 64MiB; split the task")
	}
	id, err := hub.NewID("diff-")
	if err != nil {
		return evidence, err
	}
	directory, err := os.MkdirTemp(filepath.Join(m.config.Root, "evidence"), "build-")
	if err != nil {
		return evidence, err
	}
	persisted := false
	defer func() {
		if !persisted {
			_ = os.RemoveAll(directory)
		}
	}()
	for _, name := range []string{"old", "new"} {
		if err = os.Mkdir(filepath.Join(directory, name), 0700); err != nil {
			return evidence, err
		}
	}
	names := make([]string, len(evidence.Changes))
	for i, change := range evidence.Changes {
		names[i] = change.Path
	}
	if err = writeSnapshot(filepath.Join(directory, "old"), base, names); err != nil {
		return evidence, err
	}
	if err = writeSnapshot(filepath.Join(directory, "new"), worker, names); err != nil {
		return evidence, err
	}
	patch, err := m.runExit(owned, directory, []string{"diff", "--no-index", "--binary", "--full-index", "--no-ext-diff", "--no-textconv", "--src-prefix=a/", "--dst-prefix=b/", "--", "old", "new"}, nil, nil, 16<<20, true)
	if err != nil {
		return evidence, err
	}
	if len(patch) == 0 {
		return evidence, errors.New("Git generated no patch for changed literal files")
	}
	if err = m.evidenceBudget(evidence.SnapshotBytes + len(patch)); err != nil {
		return evidence, err
	}
	// Evidence must describe files that were still current when generation ended.
	again, err := m.currentSnapshot(owned, h.Path, entries)
	if err != nil || again.digest() != worker.digest() {
		return evidence, errors.New("Worker changed while preview was being generated; retry explicit Preview")
	}
	latestShared, err := m.cleanShared(owned, h, entries, base)
	if err != nil {
		return evidence, err
	}
	if latestShared.digestWithPermissions() != shared.digestWithPermissions() {
		return evidence, errors.New("shared workspace permissions changed during preview; generate new evidence")
	}
	evidence.ID, evidence.WorkerID, evidence.BaseCommit = id, h.WorkerID, h.BaseCommit
	evidence.WorkerSHA256, evidence.SharedSHA256, evidence.PatchSHA256 = worker.digest(), shared.digestWithPermissions(), sha(patch)
	evidence.PatchBytes, evidence.CreatedAt = len(patch), time.Now().UTC().UnixMilli()
	root, err := os.OpenRoot(directory)
	if err != nil {
		return evidence, err
	}
	defer root.Close()
	for name, data := range map[string][]byte{"diff.patch": patch} {
		file, e := root.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if e != nil {
			return evidence, e
		}
		_, e = file.Write(data)
		e = errors.Join(e, file.Sync(), file.Close())
		if e != nil {
			return evidence, e
		}
	}
	metadata, err := json.Marshal(evidence)
	if err != nil {
		return evidence, err
	}
	file, err := root.OpenFile("index.json", os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return evidence, err
	}
	_, err = file.Write(metadata)
	if err = errors.Join(err, file.Sync(), file.Close()); err != nil {
		return evidence, err
	}
	if err = os.Rename(directory, filepath.Join(m.config.Root, "evidence", id)); err != nil {
		return evidence, err
	}
	persisted = true
	return evidence, nil
}
func (m *Manager) readPatch(e hub.WorktreeEvidence) ([]byte, error) {
	if !validEvidence(e.ID) || !validWorker(e.WorkerID) || e.PatchBytes < 1 || e.PatchBytes > 16<<20 || len(e.Changes) < 1 || len(e.Changes) > 128 {
		return nil, errors.New("invalid saved diff evidence")
	}
	for _, change := range e.Changes {
		if !validPath(change.Path) {
			return nil, errors.New("invalid evidence path")
		}
	}
	directory := filepath.Join(m.config.Root, "evidence", e.ID)
	info, err := os.Lstat(directory)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return nil, errors.New("private evidence directory missing or replaced")
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	for _, name := range []string{"index.json", "diff.patch"} {
		info, err := root.Lstat(name)
		if err != nil || !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("private diff evidence missing or replaced")
		}
	}
	meta, err := root.Open("index.json")
	if err != nil {
		return nil, err
	}
	metadata, err := io.ReadAll(io.LimitReader(meta, 128<<10+1))
	err = errors.Join(err, meta.Close())
	if err != nil || len(metadata) > 128<<10 {
		return nil, errors.New("private diff metadata exceeds budget")
	}
	expected, err := json.Marshal(e)
	if err != nil || string(metadata) != string(expected) {
		return nil, errors.New("saved diff metadata changed")
	}
	file, err := root.Open("diff.patch")
	if err != nil {
		return nil, err
	}
	defer file.Close()
	patch, err := io.ReadAll(io.LimitReader(file, 16<<20+1))
	if err != nil || len(patch) != e.PatchBytes || sha(patch) != e.PatchSHA256 {
		return nil, errors.New("private patch bytes/hash changed")
	}
	return patch, nil
}

// ReadPatch returns a small explicit raw preview page. It does not feed the
// model, execute Git or interpret its bytes as verification/adoption success.
func (m *Manager) ReadPatch(ctx context.Context, e hub.WorktreeEvidence, offset, maximum int) ([]byte, bool, error) {
	_, finish, err := m.operation(ctx)
	if err != nil {
		return nil, false, err
	}
	defer finish()
	if offset < 0 || maximum < 1 || maximum > 16<<10 {
		return nil, false, errors.New("raw patch page must be <=16KiB")
	}
	patch, err := m.readPatch(e)
	if err != nil {
		return nil, false, err
	}
	if offset > len(patch) {
		return nil, false, errors.New("patch offset outside evidence")
	}
	end := min(len(patch), offset+maximum)
	return append([]byte(nil), patch[offset:end]...), end < len(patch), nil
}

// Apply MUST be called only after the product has durably entered adopting and
// acquired exclusive leases for BOTH saved workspaces. Any error after apply
// starts is an unknown/partial outcome: no automatic retry, reset or rollback.
func (m *Manager) Apply(ctx context.Context, h hub.WorktreeBinding, e hub.WorktreeEvidence) error {
	owned, end, err := m.operation(ctx)
	if err != nil {
		return err
	}
	defer end()
	if e.WorkerID != h.WorkerID || e.BaseCommit != h.BaseCommit {
		return errors.New("diff belongs to another Worker/base")
	}
	_, err = m.readPatch(e)
	if err != nil {
		return err
	}
	entries, err := m.checkedHandle(owned, h)
	if err != nil {
		return err
	}
	base, err := m.baseSnapshot(owned, h.Workspace, entries)
	if err != nil {
		return err
	}
	shared, err := m.cleanShared(owned, h, entries, base)
	if err != nil {
		return err
	}
	worker, err := m.currentSnapshot(owned, h.Path, entries)
	if err != nil {
		return err
	}
	if worker.digest() != e.WorkerSHA256 || shared.digestWithPermissions() != e.SharedSHA256 {
		return errors.New("Worker/shared snapshot differs from the explicitly reviewed evidence")
	}
	root, err := os.OpenRoot(h.Workspace)
	if err != nil {
		return err
	}
	defer root.Close()
	for _, change := range e.Changes {
		state, exists, err := readLiteral(root, change.Path)
		if err != nil {
			return err
		}
		if exists != (change.BeforeSHA256 != "") || (exists && (state.SHA256 != change.BeforeSHA256 || state.Mode != change.BeforeMode || state.Perm != shared[change.Path].Perm)) {
			return errors.New("affected shared path changed or already exists; no adoption")
		}
	}
	// Git apply can execute repository clean/smudge filters. Apply the exact
	// literal snapshot instead: prepare all output bytes before touching any
	// target, retain original bytes in private evidence, and never interpret
	// the human-readable patch as executable instructions.
	frozen, err := os.OpenRoot(filepath.Join(m.config.Root, "evidence", e.ID))
	if err != nil {
		return err
	}
	defer frozen.Close()
	type staged struct {
		parent                *os.Root
		name, temp            string
		beforePerm, afterPerm os.FileMode
	}
	prepared := map[string]staged{}
	defer func() {
		for _, file := range prepared {
			if file.temp != "" {
				_ = file.parent.Remove(file.temp)
			}
			_ = file.parent.Close()
		}
	}()
	for i, change := range e.Changes {
		if err = owned.Err(); err != nil {
			return err
		}
		before, exists, err := readLiteral(frozen, filepath.Join("old", change.Path))
		if err != nil || exists != (change.BeforeSHA256 != "") || (exists && (before.SHA256 != change.BeforeSHA256 || before.Mode != change.BeforeMode)) {
			return errors.New("private original backup differs from reviewed evidence")
		}
		after, exists, err := readLiteral(frozen, filepath.Join("new", change.Path))
		if err != nil || exists != (change.AfterSHA256 != "") || (exists && (after.SHA256 != change.AfterSHA256 || after.Mode != change.AfterMode)) {
			return errors.New("private proposed bytes differ from reviewed evidence")
		}
		parent, basename, err := pinParent(root, change.Path, exists)
		if err != nil {
			return err
		}
		staging := staged{parent: parent, name: basename}
		prepared[change.Path] = staging
		current, currentExists, err := readPinned(parent, basename)
		if err != nil || currentExists != (change.BeforeSHA256 != "") || (currentExists && (current.SHA256 != change.BeforeSHA256 || current.Mode != change.BeforeMode || current.Perm != shared[change.Path].Perm)) {
			return errors.New("shared parent/target changed before staging; no adoption")
		}
		staging.beforePerm = current.Perm
		prepared[change.Path] = staging
		if !exists {
			continue
		}
		name := ".alva-adopt-" + e.ID + "-" + strconv.Itoa(i)
		mode := adoptionPermissions(current, currentExists, after.Mode)
		file, err := parent.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
		if err != nil {
			return err
		}
		staging.temp = name
		staging.afterPerm = mode
		prepared[change.Path] = staging
		_, err = file.Write(after.Data)
		err = errors.Join(err, file.Chmod(mode), file.Sync(), file.Close())
		if err != nil {
			return err
		}
	}
	for _, change := range e.Changes {
		if err = owned.Err(); err != nil {
			return err
		}
		file := prepared[change.Path]
		state, exists, err := readPinned(file.parent, file.name)
		if err != nil || exists != (change.BeforeSHA256 != "") || (exists && (state.SHA256 != change.BeforeSHA256 || state.Mode != change.BeforeMode || state.Perm != file.beforePerm)) {
			return errors.New("shared path changed during apply; inspect files, no automatic retry")
		}
		if change.AfterSHA256 == "" {
			err = file.parent.Remove(file.name)
		} else {
			if !exists {
				err = file.parent.Link(file.temp, file.name)
				if err == nil {
					err = file.parent.Remove(file.temp)
				}
			} else {
				err = file.parent.Rename(file.temp, file.name)
			}
		}
		if err != nil {
			return errors.Join(errors.New("literal adoption failed; private original backup retained, do not automatically retry"), err)
		}
		file.temp = ""
		prepared[change.Path] = file
		parent, err := file.parent.Open(".")
		if err != nil {
			return err
		}
		err = errors.Join(parent.Sync(), parent.Close())
		if err != nil {
			return err
		}
	}
	for _, change := range e.Changes {
		file := prepared[change.Path]
		state, exists, err := readPinned(file.parent, file.name)
		if err != nil || exists != (change.AfterSHA256 != "") || (exists && (state.SHA256 != change.AfterSHA256 || state.Mode != change.AfterMode || state.Perm != file.afterPerm)) {
			return errors.New("patch application could not prove every final path/hash; inspect actual files, no automatic retry")
		}
	}
	return nil
}

// Preserve actual Lead permissions, not the Worker's deliberately owner-only
// snapshot mode. Git only expresses executable/not executable: when that
// changes add owner execute only, or clear all execute bits. New files never
// gain group/other permissions. ACL/xattrs and OS atomic hash-CAS are not claimed.
func adoptionPermissions(current fileState, exists bool, afterGitMode string) os.FileMode {
	if !exists {
		if afterGitMode == "100755" {
			return 0700
		}
		return 0600
	}
	mode := current.Perm.Perm()
	if current.Mode != afterGitMode {
		if afterGitMode == "100755" {
			mode |= 0100
		} else {
			mode &^= 0111
		}
	}
	return mode
}
