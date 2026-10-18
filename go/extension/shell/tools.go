// INPUT: bytes, context, crypto/sha256, encoding/hex/json, errors, io, strings/time/utf8, Task, ABI/wire and native/process, platform/system/httpjson.
// OUTPUT: Actual execute_shell/read_shell_output/write_shell_stdin/kill_shell Tools with owned job references.
// POS: [L4 extension] Coordinator tools retain shared global execution leases across background work without deadlocking job management.
package shell

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	"github.com/QuincySx/Alva/go/platform/system/process"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type shellTool struct {
	plugin *Plugin
	name   string
}

var ErrExecution = errors.New("owned shell command execution failed")

func (t *shellTool) Name() string { return t.name }
func (t *shellTool) Description() string {
	return map[string]string{"execute_shell": "Execute an explicitly host-enabled shell command. Foreground waits for actual cleanup. Background lifetime follows the configured explicit run/session owner and holds global host-execution exclusion until reap. Full input/output uses same-session refs; no OS sandbox claim.", "read_shell_output": "Read bounded output references and actual status of this native session's owned shell job; no arbitrary PID/log access.", "write_shell_stdin": "Write bounded UTF-8 stdin and/or EOF to this native session's actual owned job; arbitrary host effects require approval.", "kill_shell": "Cancel and actually reap this native session's owned process group; cleanup acknowledgement is not assumed."}[t.name]
}
func (*shellTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.Coordinator, ManagesOwnTimeout: true}
}
func (t *shellTool) IsReadOnly(wire.ToolCall) bool { return t.name == "read_shell_output" }
func (t *shellTool) DescribeEffect(wire.ToolCall) abi.Effect {
	kind := abi.EffectUnknown
	if t.name == "read_shell_output" {
		kind = abi.EffectRead
	}
	return abi.Effect{Kind: kind, Description: "Execute or manage explicitly owned host shell; command/stdin may affect host files/network; no OS sandbox"}
}
func (t *shellTool) ParametersSchema() json.RawMessage {
	switch t.name {
	case "execute_shell":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["command"],"properties":{"command":{"type":"string","maxLength":49152},"timeout_ms":{"type":"integer","minimum":1,"maximum":600000},"run_in_background":{"type":"boolean"},"stdin":{"type":"string","maxLength":131072}}}`)
	case "write_shell_stdin":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["job_id"],"properties":{"job_id":{"type":"string"},"stdin":{"type":"string","maxLength":131072},"close":{"type":"boolean"}}}`)
	case "read_shell_output":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["job_id"],"properties":{"job_id":{"type":"string"},"after":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":8}}}`)
	default:
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["job_id"],"properties":{"job_id":{"type":"string"}}}`)
	}
}

type args struct {
	Command      string
	TimeoutMS    int64 `json:"timeout_ms"`
	Background   bool  `json:"run_in_background"`
	Stdin        *string
	JobID        string `json:"job_id"`
	After, Limit int
	Close        bool
}

func (t *shellTool) decode(raw json.RawMessage) (args, error) {
	var a args
	if len(raw) > 256<<10 || !utf8.Valid(raw) {
		return a, errors.New("shell input exceeds 256 KiB UTF-8")
	}
	if err := httpjson.Validate(raw); err != nil {
		return a, err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return a, errors.New("shell input requires an object")
	}
	var schema struct{ Properties map[string]any }
	json.Unmarshal(t.ParametersSchema(), &schema)
	for key, value := range fields {
		if _, ok := schema.Properties[key]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return a, errors.New("unknown/null shell field")
		}
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err := d.Decode(&a); err != nil {
		return a, err
	}
	if d.Decode(new(any)) != io.EOF {
		return a, errors.New("shell requires one object")
	}
	if a.Stdin != nil && (len(*a.Stdin) > 128<<10 || strings.ContainsRune(*a.Stdin, 0)) {
		return a, errors.New("bounded UTF-8 stdin without NUL required")
	}
	if t.name == "execute_shell" {
		if a.Command == "" || len(a.Command) > 48<<10 || strings.ContainsRune(a.Command, 0) {
			return a, errors.New("bounded explicit shell command required")
		}
		if a.TimeoutMS == 0 {
			a.TimeoutMS = 120000
		}
		if a.TimeoutMS < 1 || a.TimeoutMS > 600000 {
			return a, errors.New("shell timeout must be 1..600000 ms")
		}
	} else if a.JobID == "" || len(a.JobID) > 64 {
		return a, errors.New("opaque owned session job ID required")
	}
	if t.name == "read_shell_output" {
		if a.Limit == 0 {
			a.Limit = 4
		}
		if a.After < 0 || a.Limit < 1 || a.Limit > 8 {
			return a, errors.New("bounded output cursor/limit required")
		}
	}
	if t.name == "write_shell_stdin" && a.Stdin == nil && !a.Close {
		return a, errors.New("explicit stdin or EOF required")
	}
	return a, nil
}
func (t *shellTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	if e.ActualTool != t || call.ID == "" || e.CallID != call.ID || call.Name != t.name {
		return zero, errors.New("shell requires its actual dispatched tool/call identity")
	}
	a, err := t.decode(call.Arguments)
	if err != nil {
		return zero, err
	}
	p := t.plugin
	life, runCtx, finishCall, err := p.beginCall(e)
	if err != nil {
		return zero, err
	}
	defer finishCall()
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	stopLife, stopRun := context.AfterFunc(life, cancel), context.AfterFunc(runCtx, cancel)
	defer stopLife()
	defer stopRun()
	if life.Err() != nil || runCtx.Err() != nil {
		cancel()
	}
	ctx = operation
	if err = ctx.Err(); err != nil {
		return zero, err
	}
	if err = p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return zero, err
	}
	if t.name == "execute_shell" {
		return t.launch(ctx, e, call, a)
	}
	if err = p.check(e); err != nil {
		return zero, err
	}
	p.mu.Lock()
	j := p.jobs[a.JobID]
	p.mu.Unlock()
	if j == nil || !p.persistent && j.runID != e.RunID {
		return zero, errors.New("job is not owned by this actual run/session")
	}
	var body any
	switch t.name {
	case "read_shell_output":
		j.mu.Lock()
		if a.After > len(j.frames) {
			j.mu.Unlock()
			return zero, errors.New("shell output cursor exceeds high water")
		}
		end := min(len(j.frames), a.After+a.Limit)
		frames := append([]Frame(nil), j.frames[a.After:end]...)
		total := len(j.frames)
		j.mu.Unlock()
		body = map[string]any{"status": j.snapshot(), "frames": frames, "next": end, "high_water": total, "has_more": end < total}
	case "write_shell_stdin":
		if err = p.admit(ctx, e, call); err != nil {
			return zero, err
		}
		text := ""
		if a.Stdin != nil {
			text = *a.Stdin
		}
		deadline, cancel := context.WithTimeout(ctx, 15*time.Second)
		defer cancel()
		if err = p.write(deadline, j, text, a.Close); err != nil {
			return zero, err
		}
		body = map[string]any{"status": j.snapshot(), "stdin_bytes": len(text), "stdin_closed": a.Close}
	case "kill_shell":
		if err = p.admit(ctx, e, call); err != nil {
			return zero, err
		}
		j.cancel()
		j.process.BeginClose()
		select {
		case <-j.done:
		case <-ctx.Done():
			return zero, ctx.Err()
		}
		j.mu.Lock()
		err = j.err
		j.mu.Unlock()
		if err != nil && !abi.OnlyCancellation(err) {
			return zero, err
		}
		body = j.snapshot()
	}
	return output(call.ID, body, false)
}
func output(id string, body any, isError bool) (wire.ToolResult, error) {
	data, err := json.Marshal(body)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) > 16<<10 {
		return wire.ToolResult{}, errors.New("shell receipt metadata exceeds 16 KiB; select fewer output frames")
	}
	return wire.ToolResult{CallID: id, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}, IsError: isError}, nil
}
func (p *Plugin) admit(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) error {
	if err := p.check(e); err != nil {
		return err
	}
	h := sha256.Sum256([]byte(e.RunID + "\x00" + call.ID))
	id := "shell-" + hex.EncodeToString(h[:16])
	if err := p.config.Report(ctx, Activity{ID: id, Name: call.Name, SessionID: e.SessionID, RunID: e.RunID, CallID: e.CallID, Effect: abi.EffectUnknown}); err != nil {
		return err
	}
	return p.append(ctx, "operation_intent", map[string]any{"run_id": e.RunID, "call_id": call.ID, "tool": call.Name, "arguments": json.RawMessage(call.Arguments), "state": "operation_attempted", "notice": "No replay; actual process outcome is recorded separately"})
}
func (t *shellTool) launch(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall, a args) (wire.ToolResult, error) {
	var zero wire.ToolResult
	p := t.plugin
	// Deliberate Coordinator: acquire exactly the host's shared global lease,
	// then transfer ownership to the job until physical group cleanup.
	release, err := p.config.Locks.Acquire(ctx, abi.Scheduling{ExecutionMode: abi.SerialGlobal}, p.config.Cwd)
	if err != nil {
		return zero, err
	}
	transferred := false
	var releaseWorkspace func()
	defer func() {
		if !transferred {
			release()
			if releaseWorkspace != nil {
				releaseWorkspace()
			}
		}
	}()
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return zero, ctx.Err()
	}
	defer func() { <-p.gate }()
	if err = p.check(e); err != nil {
		return zero, err
	}
	p.mu.Lock()
	budget := p.started < 8 && len(p.jobs) < 64
	life := p.life
	p.mu.Unlock()
	if !budget {
		return zero, errors.New("shell job budget exceeds 8 per run/64 retained per session owner; explicitly stop/reset the session shell owner")
	}
	if _, err = p.config.Guard.CheckPath(ctx, p.config.Cwd, abi.AccessWrite); err != nil {
		return zero, err
	}
	if err = p.admit(ctx, e, call); err != nil {
		return zero, err
	}
	input, _ := json.Marshal(map[string]any{"command": a.Command, "stdin": a.Stdin, "explicit_environment": p.config.Env, "background": a.Background, "timeout_ms": a.TimeoutMS})
	inputRef, err := p.retain(ctx, "shell-input", call.ID, input)
	if err != nil {
		return zero, err
	}
	task, err := agentabi.NewTask(agentabi.TaskLocalBash, "Explicit host shell job", call.ID, "")
	if err != nil {
		return zero, err
	}
	if err = p.append(ctx, "job_intent", map[string]any{"task": task, "input": inputRef, "run_id": e.RunID, "call_id": call.ID}); err != nil {
		return zero, err
	}
	if err = p.check(e); err != nil {
		return zero, err
	}
	if err = ctx.Err(); err != nil {
		return zero, err
	}
	if a.Background && p.persistent && p.config.RetainWorkspace != nil {
		releaseWorkspace, err = p.config.RetainWorkspace()
		if err != nil {
			return zero, err
		}
		if releaseWorkspace == nil {
			return zero, errors.New("host workspace retention returned no owned release")
		}
	}
	jobCtx, cancel := context.WithTimeout(life, time.Duration(a.TimeoutMS)*time.Millisecond)
	proc, err := process.Start(process.Config{Command: p.config.Executable, Args: []string{"-c", a.Command}, Cwd: p.config.Cwd, Env: p.config.Env})
	if err != nil {
		cancel()
		cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer end()
		return zero, errors.Join(err, p.append(cleanup, "job_start_failed", map[string]any{"job_id": task.ID, "run_id": e.RunID, "call_id": call.ID, "started": false}))
	}
	task.Transition(agentabi.TaskRunning)
	releaseOwned := func() {
		release()
		if releaseWorkspace != nil {
			releaseWorkspace()
		}
	}
	j := &job{task: task, process: proc, cancel: cancel, done: make(chan struct{}), leaseDone: make(chan struct{}), release: releaseOwned, runID: e.RunID, callID: call.ID, status: Status{Task: task, SourceNotice: notice}}
	p.mu.Lock()
	p.jobs[task.ID] = j
	p.started++
	p.mu.Unlock()
	transferred = true
	go j.watchLease()
	go p.runJob(jobCtx, j)
	if a.Stdin != nil || !a.Background {
		text := ""
		if a.Stdin != nil {
			text = *a.Stdin
		}
		if err = p.write(ctx, j, text, !a.Background); err != nil {
			cancel()
			proc.BeginClose()
			return zero, err
		}
	}
	if a.Background {
		lifetime := "this_run_only"
		if p.persistent {
			lifetime = "this_application_native_session; normal turn completion keeps ownership; Stop/failure/timeout/application Close reaps; never restart/replay"
		}
		return output(call.ID, map[string]any{"job_id": task.ID, "status": j.snapshot(), "input": inputRef, "lifetime": lifetime}, false)
	}
	select {
	case <-j.done:
	case <-ctx.Done():
		cancel()
		proc.BeginClose()
		cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer end()
		select {
		case <-j.done:
		case <-cleanup.Done():
			return zero, errors.Join(ctx.Err(), cleanup.Err())
		}
	}
	j.mu.Lock()
	runErr := j.err
	j.mu.Unlock()
	if runErr != nil {
		if ctx.Err() == nil && abi.OnlyCancellation(runErr) {
			runErr = errors.Join(ErrExecution, runErr)
		}
		return zero, runErr
	}
	return output(call.ID, map[string]any{"job_id": task.ID, "status": j.snapshot(), "input": inputRef, "output": "Use read_shell_output with this job_id for complete captured frame references"}, j.snapshot().Task.Status == agentabi.TaskFailed)
}
