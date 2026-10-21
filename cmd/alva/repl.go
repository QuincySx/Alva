// INPUT: Explicit direct REPL stdin, same process owner/log and ordinary guarded tool approval requests
// OUTPUT: Bounded sequential direct-agent prompts and exact per-call HITL without EngineRuntime
// POS: [L6 application] Reuses the direct execution owner/session, never routes through Hub, imports legacy settings or auto-runs an empty prompt.
package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

type directREPL struct {
	read         func(context.Context) (string, error)
	diagnostic   io.Writer
	gate         chan struct{}
	mu           sync.Mutex
	lines, bytes int
}

func newDirectREPL(input io.Reader, diagnostic io.Writer) (*directREPL, error) {
	r := &directREPL{diagnostic: diagnostic, gate: make(chan struct{}, 1)}
	switch source := input.(type) {
	case *os.File:
		if source == nil {
			return nil, errors.New("REPL input file unavailable")
		}
		r.read = func(ctx context.Context) (string, error) { return readREPLFile(ctx, source) }
	case *bytes.Buffer, *bytes.Reader, *strings.Reader:
		r.read = func(ctx context.Context) (string, error) { return finiteInputLine(ctx, source, 32<<10) }
	default:
		return nil, errors.New("REPL requires sole-read stdin file/terminal or finite buffer")
	}
	return r, nil
}
func (r *directREPL) line(ctx context.Context) (string, error) {
	value, err := r.read(ctx)
	if err != nil {
		return "", err
	}
	r.mu.Lock()
	r.lines++
	r.bytes += len(value)
	over := r.lines > 1024 || r.bytes > 1<<20
	r.mu.Unlock()
	if over || !utf8.ValidString(value) {
		return "", errors.New("REPL input exceeds1024 lines/1MiB or is not UTF-8")
	}
	return strings.TrimSuffix(strings.TrimSuffix(value, "\n"), "\r"), nil
}
func (r *directREPL) Approve(ctx context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
	select {
	case r.gate <- struct{}{}:
	case <-ctx.Done():
		return security.ApprovalDecision{}, ctx.Err()
	}
	defer func() { <-r.gate }()
	if err := writeString(r.diagnostic, fmt.Sprintf("Permission %s · %s\n%s\nType allow %s to approve only this actual call; any other line denies: ", request.ID, request.ToolName, request.InputSummary, request.ID)); err != nil {
		return security.ApprovalDecision{}, err
	}
	line, err := r.line(ctx)
	if errors.Is(err, io.EOF) {
		return security.ApprovalDecision{Allowed: false, Scope: security.Once}, nil
	}
	if err != nil {
		return security.ApprovalDecision{}, err
	}
	return security.ApprovalDecision{Allowed: line == "allow "+request.ID, Scope: security.Once}, nil
}
func (r *directREPL) run(ctx context.Context, runner harness.Runner, session string, output io.Writer) error {
	if err := writeString(r.diagnostic, "Alva direct REPL. /session /help /quit; each prompt runs only after explicit input.\n"); err != nil {
		return err
	}
	for {
		if err := writeString(r.diagnostic, "alva> "); err != nil {
			return err
		}
		line, err := r.line(ctx)
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
		if strings.TrimSpace(line) == "" {
			continue
		}
		switch strings.TrimSpace(line) {
		case "/quit", "/exit", "/q":
			return nil
		case "/session":
			if err := writeString(r.diagnostic, "Native session: "+session+"\n"); err != nil {
				return err
			}
			continue
		case "/help":
			if err := writeString(r.diagnostic, "/session /help /quit; per-call approvals never grant broader or future permission.\n"); err != nil {
				return err
			}
			continue
		}
		if strings.HasPrefix(strings.TrimSpace(line), "/") {
			return errors.New("unknown direct REPL command; see /help")
		}
		if err := runRunner(ctx, runner, line, output); err != nil {
			return err
		}
		if cliSuspended(runner) {
			return nil
		}
	}
}
