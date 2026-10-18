// INPUT: context, errors, io, os, time, native/process.
// OUTPUT: Result, Run, ErrOutputLimit and ErrCleanup for bounded explicitly configured foreground commands.
// POS: [platform / native execution host] Shared native command lifetime drains bounded streams before reap and never claims cancelled or partial output is complete.
package command

import (
	"context"
	"errors"
	"io"
	"os"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
)

var ErrOutputLimit = errors.New("command output exceeded its retained byte budget; process stopped")
var ErrCleanup = errors.New("command process cleanup did not complete")

type Result struct {
	Stdout, Stderr                         []byte
	InputBytes                             int64
	ExitCode                               int
	Started, Reaped, Complete, OutputLimit bool
}
type ioResult struct {
	kind  int
	data  []byte
	n     int64
	err   error
	limit bool
}

// Run does not interpret a shell or infer executable/config/environment. The
// caller owns prior authorization and durable start receipts. Nonzero exit is
// a real business result, not transport success or a hidden automatic retry.
func Run(ctx context.Context, config process.Config, input []byte, streamLimit int) (result Result, runErr error) {
	if streamLimit < 1 || streamLimit > 2<<20 || len(input) > 256<<10 {
		return result, errors.New("command stdin or stream limit exceeds host budget")
	}
	if err := ctx.Err(); err != nil {
		return result, err
	}
	child, err := process.Start(config)
	if err != nil {
		return result, err
	}
	result.Started = true
	done := make(chan ioResult, 3)
	go func() { n, err := writeInput(child.Stdin, input); done <- ioResult{kind: 0, n: n, err: err} }()
	read := func(kind int, reader io.Reader) {
		data, err := io.ReadAll(io.LimitReader(reader, int64(streamLimit)+1))
		limited := len(data) > streamLimit
		if limited {
			data = data[:streamLimit]
		}
		done <- ioResult{kind: kind, data: data, err: err, limit: limited}
	}
	go read(1, child.Stdout)
	go read(2, child.Stderr)
	closing := false
	closeChild := func() {
		if !closing {
			closing = true
			child.BeginClose()
		}
	}
	consume := func(item ioResult) {
		switch item.kind {
		case 0:
			result.InputBytes = item.n
		case 1:
			result.Stdout = item.data
		case 2:
			result.Stderr = item.data
		}
		if item.limit {
			result.OutputLimit = true
			runErr = errors.Join(runErr, ErrOutputLimit)
			closeChild()
		}
		if item.err != nil && !(closing && (errors.Is(item.err, os.ErrClosed) || errors.Is(item.err, io.ErrClosedPipe))) {
			runErr = errors.Join(runErr, item.err)
			closeChild()
		}
	}
	pending := 3
	for pending > 0 {
		select {
		case item := <-done:
			pending--
			consume(item)
		case <-ctx.Done():
			runErr = errors.Join(runErr, ctx.Err())
			closeChild()
			// Close has closed all pipes. Still join the real finite I/O tasks,
			// not report success while detached tasks keep using this process.
			for pending > 0 {
				item := <-done
				pending--
				consume(item)
			}
		}
	}
	if !closing {
		runErr = errors.Join(runErr, child.Wait(ctx))
	}
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	if err := child.Close(cleanup); err != nil {
		runErr = errors.Join(runErr, ErrCleanup, err)
	} else {
		result.Reaped = true
	}
	result.ExitCode, _ = child.ExitCode()
	result.Complete = runErr == nil && result.Reaped && result.InputBytes == int64(len(input))
	return result, runErr
}
func writeInput(writer io.WriteCloser, input []byte) (int64, error) {
	var total int64
	for len(input) > 0 {
		n, err := writer.Write(input)
		total += int64(n)
		input = input[n:]
		if err != nil {
			return total, errors.Join(err, writer.Close())
		}
		if n == 0 {
			return total, errors.Join(io.ErrShortWrite, writer.Close())
		}
	}
	return total, writer.Close()
}
