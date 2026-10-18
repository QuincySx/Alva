// INPUT: Exclusive stdin file or finite in-memory reader, cancellation context and explicit byte ceiling
// OUTPUT: Read bounded secret bytes without detached reads or an uninterruptible open-pipe shutdown
// POS: [platform / OS primitive] Shared native command boundary; never discovers, persists, logs or parses credentials
package secretinput

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"strings"
)

// Read borrows exclusive read use until return. Native pipes use bounded polls
// or supported deadlines; interactive terminals are rejected before reading.
// Regular redirected files and finite in-memory buffers are read synchronously,
// checking cancellation before and after the bounded read. No read goroutine
// survives return and the caller retains ownership of the file itself.
func Read(ctx context.Context, input io.Reader, maximum int) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if maximum < 1 || maximum > 1<<20 {
		return nil, errors.New("secret input byte ceiling must be 1..1 MiB")
	}
	if file, ok := input.(*os.File); ok {
		if file == nil {
			return nil, errors.New("secret input file required")
		}
		info, err := file.Stat()
		if err != nil {
			return nil, errors.New("secret input file unavailable")
		}
		if !info.Mode().IsRegular() {
			if info.Mode()&os.ModeNamedPipe == 0 {
				return nil, errors.New("secret input requires a pipe or regular file; interactive input is not supported")
			}
			return readNativePipe(ctx, file, maximum)
		}
	} else {
		switch input.(type) {
		case *bytes.Reader, *bytes.Buffer, *strings.Reader:
		default:
			return nil, errors.New("secret input must be a finite buffer or owned stdin file")
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	data, err := io.ReadAll(io.LimitReader(input, int64(maximum)+1))
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	if err != nil {
		return nil, errors.New("secret input read failed")
	}
	if len(data) > maximum {
		return nil, errors.New("secret input exceeds byte ceiling")
	}
	return data, nil
}
