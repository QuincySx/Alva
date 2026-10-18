// INPUT: Explicit sole-reader direct REPL file/terminal and cancellation context
// OUTPUT: Bounded polled native command line without changing/closing the borrowed descriptor
// POS: [L6 application] Shares the verified Unix poll reader with approval input; no detached blocking goroutine.
//go:build darwin || linux

package main

import (
	"context"
	"errors"
	"os"
)

func readREPLFile(ctx context.Context, file *os.File) (string, error) {
	fd := file.Fd()
	if uint64(fd) > uint64(^uint32(0)>>1) {
		return "", errors.New("REPL descriptor out of range")
	}
	return finiteInputLine(ctx, &approvalFileReader{ctx: ctx, file: file, fd: int32(fd)}, 32<<10)
}
