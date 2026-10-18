// INPUT: context, encoding/base64/json, errors, io, sync/time/utf8, agent Task, ABI/wire, native/process, storage/session.
// OUTPUT: Owned process jobs with bounded redacted frame references, real task lifecycle, stdin and physical cleanup evidence.
// POS: [L4 extension] Background execution is run-owned and retains the shared global lease until actual process-group cleanup.
package shell

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"os"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/process"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Frame struct {
	Sequence                int            `json:"sequence"`
	Stream                  string         `json:"stream"`
	Reference               wire.Reference `json:"reference"`
	CompleteLine, Truncated bool
}
type Status struct {
	Task                   agentabi.Task `json:"task"`
	ExitCode               *int          `json:"exit_code,omitempty"`
	Reaped, OutputComplete bool
	SourceNotice           string `json:"source_notice"`
}
type job struct {
	mu            sync.Mutex
	output        sync.Mutex
	input         sync.Mutex
	task          agentabi.Task
	process       *process.Process
	cancel        context.CancelFunc
	done          chan struct{}
	leaseDone     chan struct{}
	release       func()
	releaseOnce   sync.Once
	err           error
	frames        []Frame
	bytes         int
	inputBytes    int
	stdinClosed   bool
	status        Status
	runID, callID string
}

const notice = "Untrusted output from an explicitly host-approved shell, not an OS sandbox. Known credential fields/markers/configured environment values are redacted; binary is explicitly base64 encoded; line/total limits fail explicitly. No automatic model delivery/replay. An explicit run or application-session owner retains the process/global lease until actual reap; opaque IDs are never adopted after restart."

func (p *Plugin) retain(ctx context.Context, kind, name string, raw []byte, framing ...process.StreamLine) (wire.Reference, error) {
	frame := map[string]any{}
	if utf8.Valid(raw) {
		frame["text"] = string(raw)
	} else {
		frame["base64"] = base64.StdEncoding.EncodeToString(raw)
	}
	if len(framing) > 0 {
		frame["truncated"] = framing[0].Truncated
		frame["complete"] = framing[0].Complete
	}
	encoded, _ := json.Marshal(frame)
	clean, err := session.RedactStreamFrame(encoded, session.EnvironmentSecrets(p.config.Env)...)
	if err != nil {
		return wire.Reference{}, err
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: kind, Name: name, MediaType: "application/json", Processing: "Captured frame envelope describes exact framing/redaction/omission; known fields and explicitly configured environment values redacted; binary explicitly base64; no automatic model delivery", Data: clean, Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain shell " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
func (p *Plugin) capture(ctx context.Context, j *job, stream string, r io.Reader) error {
	var consumerErr error
	err := process.ReadLines(r, 1<<20, func(line process.StreamLine) error {
		j.output.Lock()
		defer j.output.Unlock()
		persist, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		j.mu.Lock()
		over := len(j.frames) >= 2048 || j.bytes+len(line.Data) > 8<<20
		j.mu.Unlock()
		p.mu.Lock()
		over = over || p.bytes+len(line.Data) > 32<<20
		if !over {
			p.bytes += len(line.Data)
		}
		p.mu.Unlock()
		if over {
			consumerErr = errors.New("shell complete-output budget exceeded")
			return consumerErr
		}
		ref, err := p.retain(persist, "shell-"+stream, j.task.ID, line.Data, line)
		if err != nil {
			consumerErr = err
			return err
		}
		j.mu.Lock()
		entry := Frame{Sequence: len(j.frames) + 1, Stream: stream, Reference: ref, CompleteLine: line.Complete, Truncated: line.Truncated}
		j.frames = append(j.frames, entry)
		j.bytes += len(line.Data)
		j.task.AdvanceOutput(int64(j.bytes))
		j.mu.Unlock()
		if err = p.append(persist, "output_frame", map[string]any{"job_id": j.task.ID, "run_id": j.runID, "call_id": j.callID, "frame": entry}); err != nil {
			consumerErr = err
			return err
		}
		return nil
	})
	if consumerErr != nil {
		return consumerErr
	}
	if err != nil && !errors.Is(err, os.ErrClosed) {
		return err
	}
	return nil
}
func (p *Plugin) runJob(ctx context.Context, j *job) {
	defer close(j.done)
	defer j.cancel()
	// Each stream is framed before credential redaction; arbitrary Read splits
	// cannot bypass known plaintext markers or UTF-8 decoding.
	reads := make(chan error, 2)
	go func() { reads <- p.capture(ctx, j, "stdout", j.process.Stdout) }()
	go func() { reads <- p.capture(ctx, j, "stderr", j.process.Stderr) }()
	var runErr error
	for count := 0; count < 2; count++ {
		select {
		case err := <-reads:
			runErr = errors.Join(runErr, err)
			if err != nil {
				j.cancel()
				j.process.BeginClose()
			}
		case <-ctx.Done():
			runErr = errors.Join(runErr, ctx.Err())
			j.process.BeginClose()
			runErr = errors.Join(runErr, <-reads)
		}
	}
	if runErr == nil {
		runErr = j.process.Wait(ctx)
	}
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	runErr = errors.Join(runErr, j.process.Close(cleanup))
	reaped := j.process.CleanupComplete()
	if reaped {
		j.releaseIfClean()
	}
	j.mu.Lock()
	code, ok := j.process.ExitCode()
	if ok {
		j.status.ExitCode = &code
	}
	j.status.Reaped = reaped
	j.status.OutputComplete = runErr == nil
	next := agentabi.TaskSucceeded
	if runErr != nil {
		next = agentabi.TaskFailed
		if abi.OnlyCancellation(runErr) {
			next = agentabi.TaskCancelled
		}
	} else if code != 0 {
		next = agentabi.TaskFailed
	}
	runErr = errors.Join(runErr, j.task.Transition(next))
	j.status.Task = j.task
	status := j.status
	j.mu.Unlock()
	commitErr := p.append(cleanup, "job_outcome", map[string]any{"run_id": j.runID, "call_id": j.callID, "status": status, "execution_had_error": runErr != nil})
	j.mu.Lock()
	j.err = errors.Join(runErr, commitErr)
	j.mu.Unlock()
}

// The lease belongs to the job, not a particular timed Close attempt. A late
// successful reap still releases exclusion; a failed physical cleanup never
// grants other host tools access prematurely.
func (j *job) releaseIfClean() {
	if j.process.CleanupComplete() {
		j.releaseOnce.Do(j.release)
	}
}
func (j *job) watchLease() { defer close(j.leaseDone); <-j.process.Done(); j.releaseIfClean() }
func (j *job) snapshot() Status {
	j.mu.Lock()
	defer j.mu.Unlock()
	value := j.status
	value.Task = j.task
	return detachedStatus(value)
}

func (p *Plugin) write(ctx context.Context, j *job, text string, eof bool) error {
	j.input.Lock()
	defer j.input.Unlock()
	if j.stdinClosed {
		return errors.New("owned shell stdin already closed")
	}
	j.mu.Lock()
	over := j.inputBytes+len(text) > 256<<10
	j.mu.Unlock()
	if over {
		return errors.New("shell cumulative stdin exceeds 256 KiB")
	}
	ref, err := p.retain(ctx, "shell-stdin", j.task.ID, []byte(text))
	if err != nil {
		return err
	}
	if err = p.append(ctx, "stdin_intent", map[string]any{"job_id": j.task.ID, "input": ref, "bytes": len(text), "close": eof}); err != nil {
		return err
	}
	finished := make(chan error, 1)
	go func() {
		var err error
		if len(text) > 0 {
			_, err = io.WriteString(j.process.Stdin, text)
		}
		if eof {
			closeErr := j.process.Stdin.Close()
			if len(text) == 0 && errors.Is(closeErr, os.ErrClosed) {
				closeErr = nil
			}
			err = errors.Join(err, closeErr)
		}
		finished <- err
	}()
	select {
	case err = <-finished:
	case <-ctx.Done():
		j.cancel()
		j.process.BeginClose()
		err = errors.Join(ctx.Err(), <-finished)
	}
	j.mu.Lock()
	j.inputBytes += len(text)
	j.mu.Unlock()
	if eof {
		j.stdinClosed = true
	}
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	return errors.Join(err, p.append(cleanup, "stdin_outcome", map[string]any{"job_id": j.task.ID, "input": ref, "succeeded": err == nil, "close": eof}))
}
