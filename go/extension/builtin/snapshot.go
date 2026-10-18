// INPUT: context, crypto/rand/sha256, encoding/hex, errors/io/os/filepath, security Guard and kernel access.
// OUTPUT: FileSnapshot, Workspace.ReadSnapshot and ReplaceCheckedSnapshot using existing pinned os.Root grants.
// POS: [L4 extension] Optimistic checked regular-file replacement never reopens an authorized absolute path or claims OS hash-CAS.
package builtin

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

var ErrSnapshotChanged = errors.New("file changed since snapshot; replacement not attempted")
var ErrReplaceMayHaveOccurred = errors.New("checked replacement may have occurred; inspect before any explicit retry")

type FileSnapshot struct {
	Data      []byte
	SHA256    string
	Mode      os.FileMode
	workspace *Workspace
	path      string
	info      os.FileInfo
}

func snapshotHash(data []byte) string {
	hash := sha256.Sum256(data)
	return hex.EncodeToString(hash[:])
}
func privileged(info os.FileInfo) bool {
	return info.Mode()&(os.ModeSetuid|os.ModeSetgid|os.ModeSticky) != 0
}

// pinParent walks one component at a time, pinning each directory only if
// no symlink was observed before/after opening and its inode did not change.
// Once a parent is pinned all file IO uses basename relative to that handle.
func pinParent(root *os.Root, relative string) (*os.Root, string, error) {
	if !filepath.IsLocal(relative) || relative == "." {
		return nil, "", errors.New("snapshot requires a relative regular-file target")
	}
	parent, err := root.OpenRoot(".")
	if err != nil {
		return nil, "", err
	}
	parts := strings.Split(filepath.ToSlash(filepath.Clean(relative)), "/")
	for _, part := range parts[:len(parts)-1] {
		before, err := parent.Lstat(part)
		if err != nil || !before.IsDir() || before.Mode()&os.ModeSymlink != 0 {
			parent.Close()
			return nil, "", errors.New("snapshot ancestor is not a real directory")
		}
		next, err := parent.OpenRoot(part)
		if err != nil {
			parent.Close()
			return nil, "", err
		}
		file, err := next.Open(".")
		if err != nil {
			next.Close()
			parent.Close()
			return nil, "", err
		}
		actual, statErr := file.Stat()
		closeErr := file.Close()
		after, afterErr := parent.Lstat(part)
		if err = errors.Join(statErr, closeErr, afterErr); err != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(before, actual) || !os.SameFile(before, after) {
			next.Close()
			parent.Close()
			return nil, "", errors.New("snapshot directory changed while pinning")
		}
		if err = parent.Close(); err != nil {
			next.Close()
			return nil, "", err
		}
		parent = next
	}
	return parent, parts[len(parts)-1], nil
}
func readPinned(ctx context.Context, parent *os.Root, name string) ([]byte, os.FileInfo, error) {
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	before, err := parent.Lstat(name)
	if err != nil {
		return nil, nil, err
	}
	if !before.Mode().IsRegular() || before.Mode()&os.ModeSymlink != 0 || privileged(before) || before.Size() > maxFileBytes {
		return nil, nil, errors.New("snapshot target must be unprivileged regular file <=20 MiB")
	}
	f, err := parent.Open(name)
	if err != nil {
		return nil, nil, err
	}
	actual, err := f.Stat()
	if err != nil || !os.SameFile(before, actual) {
		f.Close()
		return nil, nil, ErrSnapshotChanged
	}
	data, readErr := io.ReadAll(io.LimitReader(f, maxFileBytes+1))
	afterRead, statErr := f.Stat()
	closeErr := f.Close()
	afterName, nameErr := parent.Lstat(name)
	if err = errors.Join(readErr, statErr, closeErr, nameErr); err != nil {
		return nil, nil, err
	}
	if len(data) > maxFileBytes {
		return nil, nil, errors.New("snapshot grew beyond 20 MiB")
	}
	if afterName.Mode()&os.ModeSymlink != 0 || !os.SameFile(before, afterRead) || !os.SameFile(before, afterName) || before.Size() != afterRead.Size() || before.Size() != afterName.Size() || !before.ModTime().Equal(afterRead.ModTime()) || !before.ModTime().Equal(afterName.ModTime()) || before.Mode() != afterName.Mode() {
		return nil, nil, ErrSnapshotChanged
	}
	if err = ctx.Err(); err != nil {
		return nil, nil, err
	}
	return data, afterRead, nil
}
func (w *Workspace) ReadSnapshot(ctx context.Context, path string, guard *security.Guard) (FileSnapshot, error) {
	if guard == nil {
		return FileSnapshot{}, errors.New("snapshot requires guard")
	}
	resolved, err := guard.CheckPath(ctx, path, abi.AccessRead)
	if err != nil {
		return FileSnapshot{}, err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	grant, relative, err := w.locate(resolved, abi.AccessRead)
	if err != nil {
		return FileSnapshot{}, err
	}
	parent, name, err := pinParent(grant.root, relative)
	if err != nil {
		return FileSnapshot{}, err
	}
	defer parent.Close()
	data, info, err := readPinned(ctx, parent, name)
	if err != nil {
		return FileSnapshot{}, err
	}
	return FileSnapshot{Data: data, SHA256: snapshotHash(data), Mode: info.Mode().Perm(), workspace: w, path: resolved, info: info}, nil
}

// ReplaceCheckedSnapshot serializes this Workspace's writers and checks the
// complete current bytes/hash/inode before one pinned-parent atomic rename.
// It is NOT an OS atomic hash-CAS against noncooperating external writers:
// an unavoidable final check→rename window remains. Model tools must retain
// the actual shared host execution lease for read/transform/check/replace.
func (w *Workspace) ReplaceCheckedSnapshot(ctx context.Context, path string, before FileSnapshot, data []byte, guard *security.Guard) (FileSnapshot, error) {
	if guard == nil || before.workspace != w || before.info == nil || len(data) > maxFileBytes {
		return FileSnapshot{}, errors.New("actual bounded workspace snapshot/guard required")
	}
	resolved, err := guard.CheckPath(ctx, path, abi.AccessWrite)
	if err != nil {
		return FileSnapshot{}, err
	}
	if resolved != before.path {
		return FileSnapshot{}, errors.New("snapshot target mismatch")
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	grant, relative, err := w.locate(resolved, abi.AccessWrite)
	if err != nil {
		return FileSnapshot{}, err
	}
	parent, name, err := pinParent(grant.root, relative)
	if err != nil {
		return FileSnapshot{}, err
	}
	defer parent.Close()
	current, info, err := readPinned(ctx, parent, name)
	if err != nil {
		return FileSnapshot{}, err
	}
	if snapshotHash(current) != before.SHA256 || !os.SameFile(before.info, info) || before.info.Mode() != info.Mode() {
		return FileSnapshot{}, ErrSnapshotChanged
	}
	var random [12]byte
	if _, err = rand.Read(random[:]); err != nil {
		return FileSnapshot{}, err
	}
	stage := ".alva-snapshot-" + hex.EncodeToString(random[:])
	f, err := parent.OpenFile(stage, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return FileSnapshot{}, err
	}
	defer parent.Remove(stage)
	modeErr := f.Chmod(info.Mode().Perm())
	_, writeErr := f.Write(data)
	syncErr := f.Sync()
	closeErr := f.Close()
	if err = errors.Join(modeErr, writeErr, syncErr, closeErr); err != nil {
		return FileSnapshot{}, err
	}
	latest, latestInfo, err := readPinned(ctx, parent, name)
	if err != nil {
		return FileSnapshot{}, err
	}
	if snapshotHash(latest) != before.SHA256 || !os.SameFile(info, latestInfo) || !info.ModTime().Equal(latestInfo.ModTime()) || info.Mode() != latestInfo.Mode() {
		return FileSnapshot{}, ErrSnapshotChanged
	}
	if err = ctx.Err(); err != nil {
		return FileSnapshot{}, err
	}
	if err = parent.Rename(stage, name); err != nil {
		return FileSnapshot{}, err
	}
	directory, openErr := parent.Open(".")
	if openErr != nil {
		return FileSnapshot{}, errors.Join(ErrReplaceMayHaveOccurred, openErr)
	}
	syncErr = directory.Sync()
	closeErr = directory.Close()
	if err = errors.Join(syncErr, closeErr); err != nil {
		return FileSnapshot{}, errors.Join(ErrReplaceMayHaveOccurred, err)
	}
	actualBytes, actual, err := readPinned(ctx, parent, name)
	if err != nil || snapshotHash(actualBytes) != snapshotHash(data) {
		return FileSnapshot{}, errors.Join(ErrReplaceMayHaveOccurred, err)
	}
	return FileSnapshot{Data: append([]byte(nil), data...), SHA256: snapshotHash(data), Mode: info.Mode().Perm(), workspace: w, path: resolved, info: actual}, nil
}
