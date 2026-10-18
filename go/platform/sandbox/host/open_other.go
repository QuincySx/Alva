// INPUT: errors
// OUTPUT: nativeOpenFlags
// POS: [platform / WASM host or guest] Fail-closed file access on hosts without the verified nonblocking directory adapter.
//go:build !unix && !windows

package host

import (
	"errors"
	"os"
)

func nativeOpenFlags(int, bool) (int, error) {
	return 0, errors.New("secure WASI writable adapter unsupported on this host")
}
func nativeOpen(*os.Root, string, int, os.FileMode, bool) (*os.File, error) {
	return nil, errors.New("secure WASI file adapter unsupported on this host")
}
