// INPUT: context, encoding/json, errors, os/filepath, strings, testing/time, SDK/security, owned shell process and native SQLite facts.
// OUTPUT: Real foreground/background/stdin/output/kill/timeout/secret/lease/identity and risk-before-launch regressions.
// POS: [L4 extension] Temporary explicit shell jobs prove actual physical cleanup rather than detached PID acknowledgements.
package shell

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(c context.Context, r abi.ModelRequest, e func(wire.StreamChunk) error) error {
	return f(c, r, e)
}

type fixture struct {
	repo    *session.Repository
	log     *session.Session
	cwd     string
	p       *Plugin
	runtime abi.RuntimeContext
	count   int
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	if _, err := os.Stat("/bin/sh"); err != nil {
		t.Skip("explicit local shell unavailable")
	}
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{repo: repo, log: log, cwd: t.TempDir()}
	workspace, err := workspacefs.OpenWorkspace(f.cwd)
	if err != nil {
		t.Fatal(err)
	}
	guard, err := security.NewGuard(f.cwd, nil, workspace, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	locks, _ := core.NewToolLockRegistry(16)
	f.p, err = New(ctx, Config{Repository: repo, Owner: "owner", SessionID: "native", Cwd: f.cwd, Executable: "/bin/sh", Guard: guard, Locks: locks, ApprovedHostAccess: true, Report: func(context.Context, Activity) error { return nil }})
	if err != nil {
		t.Fatal(err)
	}
	m := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
	state, err := abi.NewAgentState(m, log, f.p.Tools())
	if err != nil {
		t.Fatal(err)
	}
	f.runtime = abi.RuntimeContext{RunID: "run", State: state, Workspace: f.cwd}
	if err = f.p.OnRunStart(ctx, f.runtime); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 6*time.Second)
		defer cancel()
		err := f.p.Close(cleanup)
		if err != nil && !abi.OnlyCancellation(err) {
			t.Error(err)
		}
		if !f.p.CleanupComplete() {
			t.Error("owned process leaked")
		}
		workspace.Close()
		if err = repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return f
}

func TestConfiguredEnvironmentValueIsNeverRetainedAsUnlabelledOutput(t *testing.T) {
	f := newFixture(t)
	const credential = "opaque-configured-environment-credential"
	f.p.config.Env = map[string]string{"API_KEY": credential}
	if _, err := f.call(t, "execute_shell", map[string]any{"command": `printf '%s\n' "$API_KEY"`}); err != nil {
		t.Fatal(err)
	}
	page, err := f.repo.PageArtifacts(t.Context(), "owner", "native", "shell-stdout", "", 0, 0, 8)
	if err != nil || len(page.Entries) == 0 {
		t.Fatal("no actual stdout evidence", err)
	}
	for _, entry := range page.Entries {
		_, data, err := f.repo.ReadArtifactFull(t.Context(), "owner", "native", entry.Info.Reference().URI, 2<<20)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(data, []byte(credential)) {
			t.Fatal("configured credential reached stored stdout without a field marker")
		}
	}
}
func (f *fixture) call(t *testing.T, name string, args any) (wire.ToolResult, error) {
	t.Helper()
	f.count++
	id := strings.Repeat("c", f.count)
	data, _ := json.Marshal(args)
	for _, tool := range f.p.Tools() {
		if tool.Name() == name {
			return tool.Execute(context.Background(), abi.ToolExecutionContext{ActualTool: tool, Session: f.log, SessionID: "native", RunID: "run", CallID: id, Workspace: f.cwd}, wire.ToolCall{ID: id, Name: name, Arguments: data})
		}
	}
	t.Fatal("missing tool")
	return wire.ToolResult{}, nil
}
func jobID(t *testing.T, result wire.ToolResult) string {
	t.Helper()
	var data struct {
		ID string `json:"job_id"`
	}
	if err := json.Unmarshal([]byte(result.Content[0].Text), &data); err != nil {
		t.Fatal(err)
	}
	if data.ID == "" {
		t.Fatal("missing job ID")
	}
	return data.ID
}
func TestForegroundRealOutputExitAndCompleteReferences(t *testing.T) {
	f := newFixture(t)
	t.Setenv("ALVA_HOST_SECRET", "must-not-inherit")
	result, err := f.call(t, "execute_shell", map[string]any{"command": `test -z "$ALVA_HOST_SECRET" || exit 91; printf 'complete🙂\n'; printf '{"api_key":"fixture-secret","safe":"kept"}\n' >&2; exit 3`})
	if err != nil {
		t.Fatal(err)
	}
	if !result.IsError {
		t.Fatal("nonzero exit hidden")
	}
	id := jobID(t, result)
	p := f.p
	p.mu.Lock()
	j := p.jobs[id]
	p.mu.Unlock()
	s := j.snapshot()
	if !s.Reaped || !s.OutputComplete || s.ExitCode == nil || *s.ExitCode != 3 {
		t.Fatalf("not actual complete exit: %+v", s)
	}
	output, err := f.call(t, "read_shell_output", map[string]any{"job_id": id, "limit": 8})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(output.Content[0].Text, "complete🙂") {
		t.Fatal("raw output inline")
	}
	j.mu.Lock()
	frames := append([]Frame(nil), j.frames...)
	j.mu.Unlock()
	var all string
	for _, frame := range frames {
		_, data, err := f.repo.ReadArtifactFull(context.Background(), "owner", "native", frame.Reference.URI, 2<<20)
		if err != nil {
			t.Fatal(err)
		}
		all += string(data)
	}
	if !strings.Contains(all, "complete🙂") || strings.Contains(all, "fixture-secret") {
		t.Fatalf("wrong full/redacted evidence: %s", all)
	}
	inputs, err := f.repo.PageArtifacts(context.Background(), "owner", "native", "shell-input", "", 0, 0, 10)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range inputs.Entries {
		_, data, err := f.repo.ReadArtifactFull(context.Background(), "owner", "native", entry.Info.Reference().URI, 1<<20)
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(data), "fixture-secret") {
			t.Fatal("nested command credentials retained in input artifact")
		}
	}
}
func TestBackgroundStdinFullOutputAndGlobalLease(t *testing.T) {
	f := newFixture(t)
	result, err := f.call(t, "execute_shell", map[string]any{"command": "cat", "run_in_background": true})
	if err != nil {
		t.Fatal(err)
	}
	id := jobID(t, result)
	blocked, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	if release, err := f.p.config.Locks.Acquire(blocked, abi.Scheduling{ExecutionMode: abi.SerialGlobal}, f.cwd); err == nil {
		release()
		t.Fatal("background dropped global execution lease")
	}
	if _, err = f.call(t, "write_shell_stdin", map[string]any{"job_id": id, "stdin": "actual-input🙂\n", "close": true}); err != nil {
		t.Fatal(err)
	}
	f.p.mu.Lock()
	j := f.p.jobs[id]
	f.p.mu.Unlock()
	select {
	case <-j.done:
	case <-time.After(3 * time.Second):
		t.Fatal("cat not reaped")
	}
	if s := j.snapshot(); !s.OutputComplete || !s.Reaped || s.ExitCode == nil || *s.ExitCode != 0 {
		t.Fatalf("wrong cat status: %+v", s)
	}
	release, err := f.p.config.Locks.Acquire(context.Background(), abi.Scheduling{ExecutionMode: abi.SerialGlobal}, f.cwd)
	if err != nil {
		t.Fatal(err)
	}
	release()
}
func TestKillRunEndTimeoutAndOverflowReallyClean(t *testing.T) {
	for _, mode := range []string{"kill", "run_end", "timeout", "overflow"} {
		t.Run(mode, func(t *testing.T) {
			f := newFixture(t)
			args := map[string]any{"command": "sleep 20 & wait", "run_in_background": true}
			if mode == "timeout" {
				args["timeout_ms"] = 30
			}
			if mode == "overflow" {
				args["command"] = "head -c 1048577 /dev/zero"
			}
			result, err := f.call(t, "execute_shell", args)
			if err != nil {
				t.Fatal(err)
			}
			id := jobID(t, result)
			f.p.mu.Lock()
			j := f.p.jobs[id]
			f.p.mu.Unlock()
			switch mode {
			case "kill":
				if _, err = f.call(t, "kill_shell", map[string]any{"job_id": id}); err != nil {
					t.Fatal(err)
				}
			case "run_end":
				if err = f.p.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
					t.Fatal(err)
				}
			}
			select {
			case <-j.done:
			case <-time.After(3 * time.Second):
				t.Fatal("owned shell not stopped")
			}
			if !j.process.CleanupComplete() {
				t.Fatal("not physically reaped")
			}
			if mode == "overflow" {
				j.mu.Lock()
				err = j.err
				j.mu.Unlock()
				if err == nil {
					t.Fatal("overflow success")
				}
				j.mu.Lock()
				j.err = nil
				j.mu.Unlock()
			}
		})
	}
}
func TestPriorRiskFailureAndForgedToolNeverStart(t *testing.T) {
	f := newFixture(t)
	marker := filepath.Join(f.cwd, "marker")
	f.p.config.Report = func(context.Context, Activity) error { return errors.New("risk journal unavailable") }
	if _, err := f.call(t, "execute_shell", map[string]any{"command": "touch " + marker}); err == nil {
		t.Fatal("failed risk accepted")
	}
	if _, err := os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("process preceded durable risk")
	}
	tool := f.p.tools[0]
	if _, err := tool.Execute(context.Background(), abi.ToolExecutionContext{ActualTool: &shellTool{plugin: f.p, name: tool.name}, Session: f.log, SessionID: "native", RunID: "run", CallID: "forged", Workspace: f.cwd}, wire.ToolCall{ID: "forged", Name: tool.name, Arguments: json.RawMessage(`{"command":"true"}`)}); err == nil {
		t.Fatal("forged instance granted authority")
	}
}

type lockPlugin struct{ registry *core.ToolLockRegistry }

func (lockPlugin) Name() string { return "locks" }
func (p lockPlugin) Register(_ context.Context, r *core.Registrar) error {
	return bus.Provide[*core.ToolLockRegistry](r.BusWriter(), p.registry)
}
func TestSharedLockIdentityValidatedAfterAllRegistrations(t *testing.T) {
	for _, same := range []bool{false, true} {
		t.Run(map[bool]string{false: "different", true: "same"}[same], func(t *testing.T) {
			f := newFixture(t)
			registry := f.p.config.Locks
			if !same {
				registry, _ = core.NewToolLockRegistry(16)
			}
			m := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
			_, err := core.NewBuilder(m, f.log).Workspace(f.cwd).Plugin(f.p).Plugin(lockPlugin{registry}).Build(context.Background())
			if same && err != nil {
				t.Fatal(err)
			}
			if !same && err == nil {
				t.Fatal("late registry replaced shell lock authority")
			}
		})
	}
}

func TestLatePhysicalCleanupStillReleasesJobOwnedLease(t *testing.T) {
	locks, _ := core.NewToolLockRegistry(8)
	release, err := locks.Acquire(context.Background(), abi.Scheduling{ExecutionMode: abi.SerialGlobal}, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	proc, err := process.Start(process.Config{Command: "/bin/sh", Args: []string{"-c", "sleep 20"}, Cwd: t.TempDir()})
	if err != nil {
		release()
		t.Fatal(err)
	}
	cleanup, end := context.WithTimeout(context.Background(), 3*time.Second)
	defer end()
	defer proc.Close(cleanup)
	j := &job{process: proc, release: release, leaseDone: make(chan struct{})}
	go j.watchLease()
	blocked, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if r, err := locks.Acquire(blocked, abi.Scheduling{ExecutionMode: abi.SerialGlobal}, ""); err == nil {
		r()
		t.Fatal("lease released before real process cleanup")
	}
	short, cancelShort := context.WithCancel(context.Background())
	cancelShort()
	_ = proc.Close(short)
	select {
	case <-j.leaseDone:
	case <-cleanup.Done():
		t.Fatal("late physical reap lost lease owner")
	}
	if !proc.CleanupComplete() {
		t.Fatal("watcher reported a failed reap as clean")
	}
	r, err := locks.Acquire(cleanup, abi.Scheduling{ExecutionMode: abi.SerialGlobal}, "")
	if err != nil {
		t.Fatal("late reap left permanent global exclusion", err)
	}
	r()
}

func TestForegroundInternalTimeoutIsNotRootUserCancellation(t *testing.T) {
	f := newFixture(t)
	_, err := f.call(t, "execute_shell", map[string]any{"command": "sleep 20", "timeout_ms": 20})
	if err == nil || !errors.Is(err, ErrExecution) || abi.OnlyCancellation(err) {
		t.Fatalf("internal command timeout mislabeled user cancellation: %v", err)
	}
}
