// INPUT: Unverified native direct REPL platform
// OUTPUT: Explicit unsupported file/TTY input result
// POS: [L6 application] Finite buffer tests remain portable; no uncancellable interactive file fallback.
//go:build !darwin && !linux && !windows

package main

import (
	"context"
	"errors"
	"os"
)

func readREPLFile(context.Context, *os.File) (string, error) {
	return "", errors.New("direct REPL file input unsupported on this platform")
}
