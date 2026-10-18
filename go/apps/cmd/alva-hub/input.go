// INPUT: Explicit owned stdin, caller/Turn cancellation and bounded UTF-8 command lines
// OUTPUT: Synchronous line input with settled close callback and terminal-safe text
// POS: [L7 CLI entry] No detached reader, arbitrary command evaluation or credential/environment discovery.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/platform/system/terminaltext"
	"io"
	"os"
	"strings"
	"sync"
	"unicode/utf8"
)

type commandInput struct {
	source       io.ReadCloser
	scanner      *bufio.Scanner
	once         sync.Once
	count, bytes int
	nativeLine   func(context.Context) (string, error)
}

func newInput(source io.ReadCloser) *commandInput {
	s := bufio.NewScanner(source)
	s.Buffer(make([]byte, 4096), 64<<10+1)
	return &commandInput{source: source, scanner: s, nativeLine: nativeFileLine(source)}
}
func (i *commandInput) close() { i.once.Do(func() { _ = i.source.Close() }) }
func (i *commandInput) line(ctx context.Context) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if i.nativeLine != nil {
		line, err := i.nativeLine(ctx)
		if err != nil {
			return "", err
		}
		return i.accept(strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r"))
	}
	settled := make(chan struct{})
	stop := context.AfterFunc(ctx, func() { i.close(); close(settled) })
	defer func() {
		if !stop() {
			<-settled
		}
	}()
	ok := i.scanner.Scan()
	if ctx.Err() != nil {
		return "", ctx.Err()
	}
	if !ok {
		if i.scanner.Err() != nil {
			return "", errors.New("command input failed or line exceeded64KiB")
		}
		return "", io.EOF
	}
	return i.accept(i.scanner.Text())
}
func (i *commandInput) accept(line string) (string, error) {
	i.count++
	i.bytes += len(line) + 1
	if i.count > 1024 || i.bytes > 1<<20 || !utf8.ValidString(line) {
		return "", errors.New("command input exceeds1024 lines/1MiB or is not UTF-8")
	}
	return line, nil
}

func writeOutput(ctx context.Context, output io.Writer, text string) error {
	text = terminaltext.Escape(text)
	if len(text) > maxAnswer {
		return errors.New("terminal output exceeds8MiB after control escaping; no task replay")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if file, ok := output.(*os.File); ok {
		settled := make(chan struct{})
		stop := context.AfterFunc(ctx, func() { _ = file.Close(); close(settled) })
		defer func() {
			if !stop() {
				<-settled
			}
		}()
	}
	n, err := io.WriteString(output, text)
	if err == nil && n != len(text) {
		err = io.ErrShortWrite
	}
	if ctx.Err() != nil {
		return errors.Join(ctx.Err(), err)
	}
	return err
}
func writeJSON(ctx context.Context, output io.Writer, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if len(data) > maxAnswer {
		return errors.New("JSON output exceeds8MiB")
	}
	return writeOutput(ctx, output, string(data)+"\n")
}
