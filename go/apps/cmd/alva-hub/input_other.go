// INPUT: Existing owned non-Windows command input scanner
// OUTPUT: No replacement of the platform's settled scanner/Close cancellation owner
// POS: [L7 CLI entry] Keep non-Windows input ownership independent of the Windows borrowed-file adapter.
//go:build !windows

package main

import (
	"context"
	"io"
)

func nativeFileLine(io.ReadCloser) func(context.Context) (string, error) { return nil }
