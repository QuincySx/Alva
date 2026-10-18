// INPUT: os, syscall
// OUTPUT: nativeOpenFlags
// POS: [platform / WASM host or guest] Nonblocking confined open flags prevent FIFO substitution from wedging the native job.
//go:build unix

package host

import (
	"os"
	"syscall"
)

func nativeOpenFlags(flags int, noFollow bool) (int, error) {
	if noFollow {
		flags |= syscall.O_NOFOLLOW
	}
	return flags | syscall.O_NONBLOCK, nil
}
func nativeOpen(root *os.Root, name string, flags int, mode os.FileMode, noFollow bool) (*os.File, error) {
	flags, err := nativeOpenFlags(flags, noFollow)
	if err != nil {
		return nil, err
	}
	return root.OpenFile(name, flags, mode)
}
