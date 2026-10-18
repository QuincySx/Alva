// INPUT: errors, io, io/fs, os, path, path/filepath, strings, sync/atomic, wazero experimental/sys and sys
// OUTPUT: confined WASI filesystem implementation (private)
// POS: [platform / WASM host or guest] Descriptor-rooted regular-file WASI bridge with read-only and per-job IO/handle ceilings.
package host

import (
	"errors"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"strings"
	"sync/atomic"

	expsys "github.com/tetratelabs/wazero/experimental/sys"
	"github.com/tetratelabs/wazero/experimental/sysfs"
	"github.com/tetratelabs/wazero/sys"
)

const maxFileBytes int64 = 20 << 20
const maxWriteBytes int64 = 64 << 20

type fsBudget struct {
	open    atomic.Int64
	written atomic.Int64
}
type rootedFS struct {
	expsys.UnimplementedFS
	root     *os.Root
	readOnly bool
	budget   *fsBudget
}

func relative(raw string) (string, error) {
	if strings.IndexByte(raw, 0) >= 0 || strings.Contains(raw, "\\") || strings.HasPrefix(raw, "/") {
		return "", fs.ErrPermission
	}
	if raw == "" {
		raw = "."
	}
	clean := path.Clean(raw)
	if clean == ".." || strings.HasPrefix(clean, "../") {
		return "", fs.ErrPermission
	}
	return filepath.FromSlash(clean), nil
}
func (filesystem *rootedFS) OpenFile(raw string, flags expsys.Oflag, perm fs.FileMode) (expsys.File, expsys.Errno) {
	name, err := relative(raw)
	if err != nil {
		return nil, expsys.EPERM
	}
	known := expsys.O_APPEND | expsys.O_CREAT | expsys.O_DIRECTORY | expsys.O_DSYNC | expsys.O_EXCL | expsys.O_NOFOLLOW | expsys.O_NONBLOCK | expsys.O_RSYNC | expsys.O_SYNC | expsys.O_TRUNC | 3
	if flags & ^known != 0 || flags&3 == 3 {
		return nil, expsys.EINVAL
	}
	write := flags&3 != expsys.O_RDONLY || flags&(expsys.O_APPEND|expsys.O_CREAT|expsys.O_TRUNC) != 0
	if filesystem.readOnly && write {
		return nil, expsys.EROFS
	}
	if filesystem.budget.open.Add(1) > 256 {
		filesystem.budget.open.Add(-1)
		return nil, expsys.EAGAIN
	}
	success := false
	defer func() {
		if !success {
			filesystem.budget.open.Add(-1)
		}
	}()
	flag := os.O_RDONLY
	if flags&3 == expsys.O_WRONLY {
		flag = os.O_WRONLY
	}
	if flags&3 == expsys.O_RDWR {
		flag = os.O_RDWR
	}
	for _, pair := range []struct {
		in  expsys.Oflag
		out int
	}{{expsys.O_APPEND, os.O_APPEND}, {expsys.O_CREAT, os.O_CREATE}, {expsys.O_EXCL, os.O_EXCL}, {expsys.O_SYNC, os.O_SYNC}, {expsys.O_DSYNC, os.O_SYNC}, {expsys.O_RSYNC, os.O_SYNC}, {expsys.O_TRUNC, os.O_TRUNC}} {
		if flags&pair.in != 0 {
			flag |= pair.out
		}
	}
	// O_NOFOLLOW is fail-closed; os.Root remains the actual traversal/race
	// boundary, even if a symlink changes after this optional inspection.
	if flags&expsys.O_NOFOLLOW != 0 {
		if st, e := filesystem.root.Lstat(name); e == nil && st.Mode()&fs.ModeSymlink != 0 {
			return nil, expsys.ELOOP
		}
	}
	file, err := nativeOpen(filesystem.root, name, flag, perm.Perm()&0700, flags&expsys.O_NOFOLLOW != 0)
	if err != nil {
		return nil, expsys.UnwrapOSError(err)
	}
	st, err := file.Stat()
	if err != nil {
		file.Close()
		return nil, expsys.UnwrapOSError(err)
	}
	if (!st.IsDir() && !st.Mode().IsRegular()) || (flags&expsys.O_DIRECTORY != 0 && !st.IsDir()) || (st.IsDir() && write) {
		file.Close()
		return nil, expsys.EPERM
	}
	if st.Size() > maxFileBytes && !st.IsDir() {
		file.Close()
		return nil, expsys.ERANGE
	}
	success = true
	// Reuse wazero's descriptor adapter for the non-io.Seeker syscall shape.
	// It receives an already-authorized handle, never a host path or DirFS.
	adapted, errno := (&sysfs.AdaptFS{FS: openedFS{file: file}}).OpenFile(".", flags, perm)
	if errno != 0 {
		file.Close()
		success = false
		return nil, errno
	}
	return &rootedFile{File: adapted, file: file, budget: filesystem.budget, writable: flags&3 != expsys.O_RDONLY, appendMode: flags&expsys.O_APPEND != 0}, 0
}
func (filesystem *rootedFS) stat(raw string, follow bool) (sys.Stat_t, expsys.Errno) {
	name, err := relative(raw)
	if err != nil {
		return sys.Stat_t{}, expsys.EPERM
	}
	var st fs.FileInfo
	if follow {
		st, err = filesystem.root.Stat(name)
	} else {
		st, err = filesystem.root.Lstat(name)
	}
	if err != nil {
		return sys.Stat_t{}, expsys.UnwrapOSError(err)
	}
	return sys.NewStat_t(st), 0
}
func (filesystem *rootedFS) Stat(raw string) (sys.Stat_t, expsys.Errno) {
	return filesystem.stat(raw, true)
}
func (filesystem *rootedFS) Lstat(raw string) (sys.Stat_t, expsys.Errno) {
	return filesystem.stat(raw, false)
}
func (filesystem *rootedFS) Mkdir(raw string, perm fs.FileMode) expsys.Errno {
	if filesystem.readOnly {
		return expsys.EROFS
	}
	name, err := relative(raw)
	if err != nil {
		return expsys.EPERM
	}
	return expsys.UnwrapOSError(filesystem.root.Mkdir(name, perm.Perm()&0700))
}
func (filesystem *rootedFS) Rename(from, to string) expsys.Errno {
	if filesystem.readOnly {
		return expsys.EROFS
	}
	a, err := relative(from)
	if err != nil {
		return expsys.EPERM
	}
	b, err := relative(to)
	if err != nil {
		return expsys.EPERM
	}
	return expsys.UnwrapOSError(filesystem.root.Rename(a, b))
}
func (filesystem *rootedFS) remove(raw string, directory bool) expsys.Errno {
	if filesystem.readOnly {
		return expsys.EROFS
	}
	name, err := relative(raw)
	if err != nil {
		return expsys.EPERM
	}
	st, err := filesystem.root.Lstat(name)
	if err != nil {
		return expsys.UnwrapOSError(err)
	}
	if directory != st.IsDir() {
		if directory {
			return expsys.ENOTDIR
		}
		return expsys.EISDIR
	}
	return expsys.UnwrapOSError(filesystem.root.Remove(name))
}
func (filesystem *rootedFS) Unlink(raw string) expsys.Errno { return filesystem.remove(raw, false) }
func (filesystem *rootedFS) Rmdir(raw string) expsys.Errno  { return filesystem.remove(raw, true) }

type rootedFile struct {
	expsys.File
	file       *os.File
	budget     *fsBudget
	writable   bool
	appendMode bool
	closed     atomic.Bool
}
type openedFS struct{ file *os.File }

func (filesystem openedFS) Open(string) (fs.File, error) { return filesystem.file, nil }

func (file *rootedFile) Stat() (sys.Stat_t, expsys.Errno) {
	st, err := file.file.Stat()
	if err != nil {
		return sys.Stat_t{}, expsys.UnwrapOSError(err)
	}
	return sys.NewStat_t(st), 0
}
func (file *rootedFile) Dev() (uint64, expsys.Errno) { st, e := file.Stat(); return st.Dev, e }
func (file *rootedFile) Ino() (uint64, expsys.Errno) { st, e := file.Stat(); return st.Ino, e }
func (file *rootedFile) IsDir() (bool, expsys.Errno) { st, e := file.Stat(); return st.Mode.IsDir(), e }
func (file *rootedFile) IsAppend() bool              { return file.appendMode }
func (file *rootedFile) Read(data []byte) (int, expsys.Errno) {
	n, e := file.file.Read(data)
	return n, expsys.UnwrapOSError(e)
}
func (file *rootedFile) Pread(data []byte, offset int64) (int, expsys.Errno) {
	n, e := file.file.ReadAt(data, offset)
	return n, expsys.UnwrapOSError(e)
}
func (*rootedFile) SetAppend(bool) expsys.Errno       { return expsys.ENOTSUP }
func (*rootedFile) Utimens(int64, int64) expsys.Errno { return expsys.ENOTSUP }
func (file *rootedFile) Readdir(n int) ([]expsys.Dirent, expsys.Errno) {
	if n <= 0 || n > 4096 {
		n = 4096
	}
	entries, err := file.file.Readdir(n)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, expsys.UnwrapOSError(err)
	}
	out := make([]expsys.Dirent, 0, len(entries))
	for _, entry := range entries {
		st := sys.NewStat_t(entry)
		out = append(out, expsys.Dirent{Name: entry.Name(), Ino: st.Ino, Type: entry.Mode().Type()})
	}
	return out, 0
}
func (file *rootedFile) reserveWrite(size int, offset int64) expsys.Errno {
	if !file.writable {
		return expsys.EBADF
	}
	if offset < 0 || offset > maxFileBytes || int64(size) > maxFileBytes-offset {
		return expsys.ERANGE
	}
	for {
		used := file.budget.written.Load()
		if int64(size) > maxWriteBytes-used {
			return expsys.ERANGE
		}
		if file.budget.written.CompareAndSwap(used, used+int64(size)) {
			return 0
		}
	}
}
func (file *rootedFile) Write(data []byte) (int, expsys.Errno) {
	offset, err := file.file.Seek(0, io.SeekCurrent)
	if err != nil {
		return 0, expsys.UnwrapOSError(err)
	}
	if file.appendMode {
		st, e := file.Stat()
		if e != 0 {
			return 0, e
		}
		offset = st.Size
	}
	if e := file.reserveWrite(len(data), offset); e != 0 {
		return 0, e
	}
	n, e := file.file.Write(data)
	return n, expsys.UnwrapOSError(e)
}
func (file *rootedFile) Pwrite(data []byte, offset int64) (int, expsys.Errno) {
	if !file.writable {
		return 0, expsys.EBADF
	}
	if file.appendMode {
		return 0, expsys.EINVAL
	}
	if e := file.reserveWrite(len(data), offset); e != 0 {
		return 0, e
	}
	n, e := file.file.WriteAt(data, offset)
	return n, expsys.UnwrapOSError(e)
}
func (file *rootedFile) Truncate(size int64) expsys.Errno {
	if !file.writable {
		return expsys.EBADF
	}
	if size < 0 || size > maxFileBytes {
		return expsys.ERANGE
	}
	return expsys.UnwrapOSError(file.file.Truncate(size))
}
func (file *rootedFile) Sync() expsys.Errno     { return expsys.UnwrapOSError(file.file.Sync()) }
func (file *rootedFile) Datasync() expsys.Errno { return file.Sync() }
func (file *rootedFile) Close() expsys.Errno {
	if file.closed.Swap(true) {
		return 0
	}
	file.budget.open.Add(-1)
	return expsys.UnwrapOSError(file.file.Close())
}
