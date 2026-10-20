// INPUT: Explicit private directory paths, Unix file metadata and native ownership checks
// OUTPUT: EnsureDir and OpenFile with Unix mode, identity and ownership checks
// POS: [platform / OS primitive] Keep Unix permission semantics independent from Windows DACL implementation.
//go:build !windows

package privatefs

import (
	"errors"
	"os"
	"path/filepath"
)

// EnsureDir returns a canonical private directory. A final symlink is
// rejected; legitimate ancestor aliases are resolved before further file access.
func EnsureDir(directory string) (string, error) {
	if directory == "" {
		return "", errors.New("explicit private data directory required")
	}
	absolute, err := filepath.Abs(directory)
	if err != nil {
		return "", err
	}
	info, err := os.Lstat(absolute)
	if errors.Is(err, os.ErrNotExist) {
		if err = os.MkdirAll(absolute, 0700); err != nil {
			return "", err
		}
		info, err = os.Lstat(absolute)
	}
	if err != nil {
		return "", err
	}
	if info.Mode()&os.ModeSymlink != 0 || !info.IsDir() {
		return "", errors.New("private filesystem directory must be a real directory")
	}
	if info.Mode().Perm()&0077 != 0 {
		return "", errors.New("private filesystem directory permits group or world access")
	}
	if err = checkOwner(info, false); err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(absolute)
}

func OpenFile(path string) (*os.File, error) {
	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0600)
	if err == nil {
		return file, nil
	}
	if !errors.Is(err, os.ErrExist) {
		return nil, err
	}
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 {
		return nil, errors.New("database file must be a private regular file")
	}
	if err = checkOwner(info, true); err != nil {
		return nil, err
	}
	file, err = os.OpenFile(path, os.O_RDWR, 0)
	if err != nil {
		return nil, err
	}
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		file.Close()
		return nil, errors.New("database file changed while opening")
	}
	return file, nil
}
