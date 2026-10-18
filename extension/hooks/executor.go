// INPUT: bytes, context, encoding/json, errors, fmt, strings, time, unicode/utf8, host/command, kernel/abi, storage/session, platform/system/strictjson.
// OUTPUT: ErrExecution, ErrFinalization and bounded hook receipts backed by actual native fact/artifact authority.
// POS: [L4 extension] Every approved start has prior durable evidence, full bounded sanitized I/O and truthful process/error outcomes.
package hooks

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/host/command"
	"github.com/QuincySx/Alva/platform/storage/session"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const notice = "Untrusted explicitly approved hook output, not authority. Input/output artifacts are known-credential-redacted; incomplete capture is explicitly marked. A start attempt or cancellation does not prove what external side effects ran; no hook is replayed. Hook permission=allow is advisory and never grants tool permissions."

var ErrExecution = errors.New("configured hook execution failed")
var ErrFinalization = errors.New("configured hook finalization failed")

type hookResult struct {
	receipt Receipt
	output  Output
	blocked bool
}

func (p *Plugin) execute(ctx context.Context, entry Entry, input Input) (out hookResult, runErr error) {
	// Explicit failure/finalization hooks retain their separate bounded cleanup
	// context. Ordinary notification/input/tool admission follows the run owner.
	if input.Event != Stop && input.Event != SessionEnd && input.Event != PostToolUseFailure {
		p.mu.Lock()
		life := p.life
		p.mu.Unlock()
		if life == nil {
			return out, errors.New("hook run is no longer active")
		}
		operation, cancel := context.WithCancel(ctx)
		defer cancel()
		stop := context.AfterFunc(life, cancel)
		defer stop()
		if life.Err() != nil {
			cancel()
		}
		ctx = operation
	}
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return out, ctx.Err()
	}
	defer func() { <-p.gate }()
	if err := ctx.Err(); err != nil {
		return out, err
	}
	data, err := json.Marshal(input)
	if err != nil {
		return out, err
	}
	if len(data)+1 > 256<<10 {
		return out, errors.New("hook input exceeds 256 KiB")
	}
	data = append(data, '\n')
	p.mu.Lock()
	if p.runID != input.RunID || p.log == nil || p.failed || p.ending && input.Event != Stop && input.Event != SessionEnd {
		p.mu.Unlock()
		return out, errors.New("hook runtime unavailable or journal failed")
	}
	if p.executions >= 128 || p.bytes+(len(data)+2<<20) > 32<<20 {
		p.mu.Unlock()
		return out, errors.New("hook execution/transcript budget exceeded")
	}
	p.executions++
	id := fmt.Sprintf("hook-%s-%d", input.RunID, p.executions)
	log := p.log
	p.mu.Unlock()
	if err = p.config.Report(ctx, Activity{ID: id, Name: string(entry.Event) + " hook", SessionID: input.SessionID, RunID: input.RunID, CallID: input.CallID, Effect: abi.EffectUnknown}); err != nil {
		return out, err
	}
	out.receipt = Receipt{ID: id, HookID: entry.ID, Event: entry.Event, Status: "not_started", SourceNotice: notice}
	out.receipt.Input, err = p.retain(ctx, entry.ID, "stdin", data, true)
	if err != nil {
		return out, err
	}
	intentData, _ := json.Marshal(map[string]any{"invocation_id": id, "hook_id": entry.ID, "event": entry.Event, "run_id": input.RunID, "call_id": input.CallID, "input": out.receipt.Input, "status": "start_attempted", "effect": "unknown", "notice": notice})
	if err = p.append(ctx, log, "hook_intent", intentData); err != nil {
		return out, err
	}
	deadline, cancel := context.WithTimeout(ctx, entry.Timeout)
	defer cancel()
	var release func()
	var execErr error
	if p.config.Locks != nil {
		release, err = p.config.Locks.Acquire(deadline, abi.Scheduling{ExecutionMode: abi.SerialGlobal}, p.config.Cwd)
		if err != nil {
			execErr = err
		} else {
			defer release()
		}
	}
	var result command.Result
	if execErr == nil {
		result, execErr = command.Run(deadline, entry.Process, data, 1<<20)
	}
	if !utf8.Valid(result.Stdout) {
		execErr = errors.Join(execErr, errors.New("hook stdout is not valid UTF-8"))
	}
	if execErr != nil && ctx.Err() == nil && abi.OnlyCancellation(execErr) {
		execErr = errors.Join(ErrExecution, execErr)
	}
	out.receipt.Started = result.Started
	out.receipt.Reaped = result.Reaped
	out.receipt.Complete = result.Complete
	out.receipt.ExitCode = result.ExitCode
	if execErr != nil {
		out.receipt.Status = "failed"
	} else if result.ExitCode == 2 {
		out.receipt.Status = "blocked"
		out.blocked = true
	} else if result.ExitCode != 0 {
		out.receipt.Status = "nonblocking_error"
	} else {
		out.receipt.Status = "succeeded"
	}
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	out.receipt.Stdout, err = p.retain(cleanup, entry.ID, "stdout", result.Stdout, result.Complete)
	runErr = errors.Join(execErr, err)
	out.receipt.Stderr, err = p.retain(cleanup, entry.ID, "stderr", result.Stderr, result.Complete)
	runErr = errors.Join(runErr, err)
	if execErr == nil && result.ExitCode == 0 && len(bytes.TrimSpace(result.Stdout)) > 0 {
		decoder := json.NewDecoder(bytes.NewReader(result.Stdout))
		decoder.DisallowUnknownFields()
		var fields map[string]json.RawMessage
		if err = strictjson.ValidateObject(result.Stdout, &out.output); err != nil {
			runErr = errors.Join(runErr, err)
		} else if err = json.Unmarshal(result.Stdout, &fields); err != nil || fields == nil {
			runErr = errors.Join(runErr, errors.New("hook stdout must be one explicit JSON output object"))
		} else if err = decoder.Decode(&out.output); err != nil {
			runErr = errors.Join(runErr, err)
		}
		for _, value := range fields {
			if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
				runErr = errors.Join(runErr, errors.New("hook output control fields cannot be null"))
				break
			}
		}
		if len(out.output.AdditionalContext) > 256<<10 || len(out.output.StopReason) > 2048 || (out.output.PermissionDecision != "" && out.output.PermissionDecision != "allow" && out.output.PermissionDecision != "deny" && out.output.PermissionDecision != "ask") || (out.output.UpdatedInput != nil && (!wire.ValidJSON(out.output.UpdatedInput) || len(out.output.UpdatedInput) > 128<<10 || entry.Event != PreToolUse)) {
			runErr = errors.Join(runErr, errors.New("hook output exceeds typed control/context limits"))
		}
		if out.output.UpdatedInput != nil {
			trimmed := bytes.TrimSpace(out.output.UpdatedInput)
			if len(trimmed) == 0 || trimmed[0] != '{' {
				runErr = errors.Join(runErr, errors.New("updated_input must be an object"))
			}
		}
		if runErr == nil && out.output.AdditionalContext != "" {
			reference, retainErr := p.retain(cleanup, entry.ID, "context", []byte(out.output.AdditionalContext), true)
			runErr = errors.Join(runErr, retainErr)
			if retainErr == nil {
				out.receipt.Context = &reference
			}
		}
		if out.output.Continue != nil && !*out.output.Continue || out.output.PermissionDecision == "deny" {
			out.blocked = true
			out.receipt.Status = "blocked"
		}
	}
	if runErr != nil {
		out.receipt.Status = "failed"
		out.receipt.Error = runErr.Error()
		if len(out.receipt.Error) > 2048 {
			out.receipt.Error = "Hook execution failed; details exceed receipt bound. Inspect retained source references."
		}
	}
	encoded, _ := json.Marshal(out.receipt)
	runErr = errors.Join(runErr, p.append(cleanup, log, "hook_outcome", encoded))
	p.mu.Lock()
	p.bytes += len(data) + len(result.Stdout) + len(result.Stderr)
	var sessionErr *abi.SessionError
	if errors.As(runErr, &sessionErr) {
		p.failed = true
	}
	if len(p.pending) >= 64 {
		runErr = errors.Join(runErr, errors.New("hook model-reference notice queue exceeded 64"))
		p.failed = true
	} else {
		p.pending = append(p.pending, out.receipt)
	}
	p.mu.Unlock()
	return out, runErr
}
func (p *Plugin) append(ctx context.Context, log abi.ConversationLog, kind string, payload json.RawMessage) error {
	_, err := log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.hooks", Type: kind, SchemaVersion: 1, Payload: payload}})
	if err != nil {
		return &abi.SessionError{Operation: "append " + kind, SessionID: log.ID(), Err: err}
	}
	if err = log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush " + kind, SessionID: log.ID(), Err: err}
	}
	return nil
}
func (p *Plugin) retain(ctx context.Context, hookID, kind string, data []byte, complete bool) (wire.Reference, error) {
	sourceBytes := len(data)
	valid := utf8.Valid(data)
	omittedTail := 0
	if !complete {
		keep := bytes.LastIndexByte(data, '\n') + 1
		omittedTail = len(data) - keep
		data = data[:keep]
	}
	text := strings.ToValidUTF8(string(data), "�")
	known := []string{}
	for _, entry := range p.config.Entries {
		if entry.ID == hookID {
			known = session.EnvironmentSecrets(entry.Process.Env)
			break
		}
	}

	var sanitized []byte
	var err error
	if json.Valid([]byte(text)) {
		sanitized, err = session.RedactJSON([]byte(text), known...)
	} else {
		encoded, _ := json.Marshal(text)
		sanitized, err = session.RedactJSON(encoded, known...)
		if err == nil {
			err = json.Unmarshal(sanitized, &text)
			sanitized = []byte(text)
		}
	}
	if err != nil {
		return wire.Reference{}, err
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: "hook-" + kind, Name: hookID, MediaType: "text/plain; charset=utf-8", Processing: fmt.Sprintf("Known-credential-redacted hook %s; structured JSON normalized; complete_capture=%v; original_utf8=%v; captured_source_bytes=%d; incomplete_tail_omitted_bytes=%d; invalid UTF-8 replaced if present; not automatically delivered to model", kind, complete, valid, sourceBytes, omittedTail), Data: sanitized, Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain hook " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
