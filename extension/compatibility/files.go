// INPUT: Explicit workspace os.Root and relative bounded source paths.
// OUTPUT: Pinned directory traversal and bounded UTF-8 regular-file snapshots.
// POS: [L4 extension] Confines optional rule/agent/config reads and rejects symlink ancestors.
package compatibility

import (
	"context"
	"errors"
	"io"
	"os"
	"path"
	"strings"
	"unicode/utf8"
)

// directory owns each newly opened descriptor. Every step verifies the opened
// directory identity before proceeding; paths never resolve through a symlink.
func directory(root *os.Root, relative string) (*os.Root, error) {
	if !ValidPath(relative) {
		return nil, errors.New("confined compatibility directory required")
	}
	current, err := root.OpenRoot(".")
	if err != nil {
		return nil, err
	}
	if relative == "." {
		return current, nil
	}
	for _, part := range strings.Split(relative, "/") {
		before, e := current.Lstat(part)
		if e != nil || !before.IsDir() || before.Mode()&os.ModeSymlink != 0 {
			current.Close()
			return nil, errors.New("compatibility directory is missing or is not a real directory")
		}
		next, e := current.OpenRoot(part)
		current.Close()
		if e != nil {
			return nil, e
		}
		actual, e := next.Stat(".")
		if e != nil || !os.SameFile(before, actual) {
			next.Close()
			return nil, errors.New("compatibility directory changed during open")
		}
		current = next
	}
	return current, nil
}

func readFile(ctx context.Context, root *os.Root, relative string, limit int64) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !ValidPath(relative) || relative == "." {
		return nil, errors.New("confined compatibility file required")
	}
	parent, err := directory(root, path.Dir(relative))
	if err != nil {
		return nil, err
	}
	defer parent.Close()
	name := path.Base(relative)
	before, err := parent.Lstat(name)
	if err != nil {
		return nil, err
	}
	if !before.Mode().IsRegular() || before.Size() > limit {
		return nil, errors.New("compatibility source must be a bounded regular file")
	}
	file, err := parent.Open(name)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	actual, err := file.Stat()
	if err != nil || !os.SameFile(before, actual) {
		return nil, errors.New("compatibility source changed during open")
	}
	data, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > limit || !utf8.Valid(data) {
		return nil, errors.New("compatibility source exceeds its byte limit or is not UTF-8")
	}
	return data, ctx.Err()
}
