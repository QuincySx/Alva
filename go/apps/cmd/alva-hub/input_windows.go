// INPUT: Owned Hub stdin file/pipe and shared Windows non-readahead line transport
// OUTPUT: Native file line binding with actual cancellation and preserved REPL/approval policies
// POS: [L7 CLI entry] Closing a synchronous Windows console/pipe handle is not used to fake reader completion.
//go:build windows

package main

import (
	"context"
	"io"
	"os"

	"github.com/QuincySx/Alva/go/platform/system/lineinput"
)

func nativeFileLine(source io.ReadCloser) func(context.Context) (string, error) {
	file, ok := source.(*os.File)
	if !ok {
		return nil
	}
	return func(ctx context.Context) (string, error) { return lineinput.ReadLine(ctx, file, 64<<10) }
}
