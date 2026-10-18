// INPUT: Actual private native facts, real shell processes, per-attempt guards and application Sessions owner
// OUTPUT: Cross-turn ownership/stdin, detached status snapshots, stale/foreign denial, real Stop/Close and unknown-without-replay recovery regressions
// POS: [L4 extension] Fixtures keep jobs alive across successful runs only under an explicit owner and always reap their own groups.
package shell

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/abi"
	kernel "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func sessionFixture(t *testing.T) (*fixture, *Sessions, *Lease) {
	t.Helper()
	f := newFixture(t)
	if err := f.p.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
		t.Fatal(err)
	}
	s := NewSessions()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := s.Close(ctx); err != nil && !kernel.OnlyCancellation(err) {
			t.Error(err)
		}
		if !s.CleanupComplete() {
			t.Error("application session shell ownership leaked")
		}
	})
	l, err := s.Bind(t.Context(), f.p.config)
	if err != nil {
		t.Fatal(err)
	}
	f.p = l.Plugin
	if err := l.OnRunStart(context.Background(), f.runtime); err != nil {
		t.Fatal(err)
	}
	return f, s, l
}
func callInRun(t *testing.T, f *fixture, run, name string, args any) (wire.ToolResult, error) {
	t.Helper()
	f.count++
	id := strings.Repeat("s", f.count)
	data, _ := json.Marshal(args)
	for _, tool := range f.p.Tools() {
		if tool.Name() == name {
			return tool.Execute(t.Context(), kernel.ToolExecutionContext{ActualTool: tool, Session: f.log, SessionID: "native", RunID: run, CallID: id, Workspace: f.cwd}, wire.ToolCall{ID: id, Name: name, Arguments: data})
		}
	}
	t.Fatal("tool unavailable")
	return wire.ToolResult{}, nil
}
func TestApplicationOwnerKeepsJobAcrossSuccessfulTurnsAndUsesCurrentRunAdmission(t *testing.T) {
	f, s, lease := sessionFixture(t)
	result, err := callInRun(t, f, "run", "execute_shell", map[string]any{"command": "cat", "run_in_background": true, "timeout_ms": 60000})
	if err != nil {
		t.Fatal(err)
	}
	id := jobID(t, result)
	if err := lease.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
		t.Fatal(err)
	}
	if err := lease.Close(t.Context()); err != nil || !lease.CleanupComplete() {
		t.Fatal("attempt reservation did not release", err)
	}
	if s.CleanupComplete() {
		t.Fatal("live job lost application ownership")
	}
	if jobs := s.List("foreign", "native"); len(jobs) != 0 {
		t.Fatal("foreign owner saw jobs")
	}
	if err := s.Stop(t.Context(), "foreign", "native", id); err == nil {
		t.Fatal("foreign owner stopped job")
	}
	lease2, err := s.Bind(t.Context(), f.p.config)
	if err != nil {
		t.Fatal(err)
	}
	if lease2.Plugin != lease.Plugin {
		t.Fatal("live process owner was replaced")
	}
	if _, err := s.Bind(t.Context(), f.p.config); err == nil {
		t.Fatal("concurrent attempt stole guard/reporter")
	}
	runtime := f.runtime
	runtime.RunID = "run-two"
	if err := lease2.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	if _, err := callInRun(t, f, "run", "kill_shell", map[string]any{"job_id": id}); err == nil {
		t.Fatal("stale run controlled new attempt")
	}
	if _, err := callInRun(t, f, "run-two", "write_shell_stdin", map[string]any{"job_id": id, "stdin": "cross-turn-input🙂\n", "close": true}); err != nil {
		t.Fatal(err)
	}
	f.p.mu.Lock()
	j := f.p.jobs[id]
	f.p.mu.Unlock()
	select {
	case <-j.done:
	case <-time.After(5 * time.Second):
		t.Fatal("cross-turn cat not reaped")
	}
	if _, err := callInRun(t, f, "run-two", "read_shell_output", map[string]any{"job_id": id}); err != nil {
		t.Fatal(err)
	}
	jobs := s.List("owner", "native")
	if len(jobs) != 1 || jobs[0].RunID != "run" || !jobs[0].Status.Reaped || jobs[0].Status.Task.Status != abi.TaskSucceeded {
		t.Fatal("cross-turn ownership/outcome lost", jobs)
	}
	for _, frame := range j.frames {
		_, data, err := f.repo.ReadArtifactFull(t.Context(), "owner", "native", frame.Reference.URI, 2<<20)
		if err != nil {
			t.Fatal(err)
		}
		if frame.Stream == "stdout" && !strings.Contains(string(data), "cross-turn-input") {
			t.Fatal("full output lost", string(data))
		}
	}
	if err := lease2.OnRunEnd(context.Background(), runtime, nil); err != nil {
		t.Fatal(err)
	}
	if err := lease2.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	// A late old-attempt Close cannot clear a later reservation.
	lease3, err := s.Bind(t.Context(), f.p.config)
	if err != nil {
		t.Fatal(err)
	}
	if err := lease.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if lease3.CleanupComplete() {
		t.Fatal("old lease released a new reservation")
	}
	if err := lease.OnRunStart(context.Background(), runtime); err == nil {
		t.Fatal("expired attempt restarted against the new guard/reservation")
	}
	if err := lease3.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
}
func TestOperatorStopAndApplicationCloseActuallyReapCrossTurnJob(t *testing.T) {
	for _, closeApplication := range []bool{false, true} {
		t.Run(map[bool]string{false: "explicit Stop", true: "application Close"}[closeApplication], func(t *testing.T) {
			f, s, lease := sessionFixture(t)
			result, err := callInRun(t, f, "run", "execute_shell", map[string]any{"command": "cat", "run_in_background": true, "timeout_ms": 60000})
			if err != nil {
				t.Fatal(err)
			}
			id := jobID(t, result)
			if err := lease.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
				t.Fatal(err)
			}
			if err := lease.Close(t.Context()); err != nil {
				t.Fatal(err)
			}
			changed := f.p.config
			changed.Env = map[string]string{"APPROVED_NEW_ENV": "new-value"}
			if _, err := s.Bind(t.Context(), changed); err == nil {
				t.Fatal("live immutable recipe changed")
			}
			if closeApplication {
				err = s.Close(t.Context())
			} else {
				err = s.Stop(t.Context(), "owner", "native", id)
			}
			if err != nil {
				t.Fatal(err)
			}
			if !s.CleanupComplete() {
				t.Fatal("shutdown acknowledgement preceded physical cleanup")
			}
			f.p.mu.Lock()
			j := f.p.jobs[id]
			f.p.mu.Unlock()
			select {
			case <-j.leaseDone:
			default:
				t.Fatal("shared global lease still owned")
			}
			if !closeApplication {
				if err := s.Reset(t.Context(), "owner", "native"); err != nil {
					t.Fatal(err)
				}
				l, err := s.Bind(t.Context(), changed)
				if err != nil {
					t.Fatal(err)
				}
				if err := l.Close(t.Context()); err != nil {
					t.Fatal(err)
				}
			}
		})
	}
}
func TestRestartMarksUnfinishedShellJobUnknownWithoutAdoptingOrReplaying(t *testing.T) {
	f := newFixture(t)
	if err := f.p.OnRunEnd(context.Background(), f.runtime, nil); err != nil {
		t.Fatal(err)
	}
	task, err := abi.NewTask(abi.TaskLocalBash, "interrupted fixture", "old-call", "")
	if err != nil {
		t.Fatal(err)
	}
	if err := f.p.append(t.Context(), "job_intent", map[string]any{"task": task, "run_id": "interrupted-run"}); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		s := NewSessions()
		lease, err := s.Bind(t.Context(), f.p.config)
		if err != nil {
			t.Fatal(err)
		}
		if len(s.List("owner", "native")) != 0 || !s.CleanupComplete() {
			t.Fatal("historical task adopted or restarted")
		}
		if err := s.Stop(t.Context(), "owner", "native", task.ID); err == nil {
			t.Fatal("historical ID became a process grant")
		}
		if err := lease.Close(t.Context()); err != nil {
			t.Fatal(err)
		}
		if err := s.Close(t.Context()); err != nil && !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	}
	records, err := f.log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, r := range records {
		if r.Namespace == "alva.shell" && r.Type == "job_abandoned" {
			count++
			if !strings.Contains(string(r.Payload), "unknown") {
				t.Fatal("fake recovered success")
			}
		}
	}
	if count != 1 {
		t.Fatal("unknown recovery duplicated or missing", count)
	}
}

func TestStatusSnapshotsDetachTaskTimesAndExitCode(t *testing.T) {
	for _, view := range []string{"application list", "job snapshot"} {
		t.Run(view, func(t *testing.T) {
			started := time.Unix(100, 0).UTC()
			ended := time.Unix(200, 0).UTC()
			exit := 7
			j := &job{task: abi.Task{ID: "owned-job", StartedAt: &started, EndedAt: &ended}, status: Status{ExitCode: &exit}}
			owner := &Plugin{jobs: map[string]*job{"owned-job": j}}
			sessions := &Sessions{entries: map[sessionKey]*Plugin{{owner: "owner", native: "native"}: owner}}
			read := func() Status {
				if view == "job snapshot" {
					return j.snapshot()
				}
				values := sessions.List("owner", "native")
				if len(values) != 1 {
					t.Fatal("owned job unavailable", values)
				}
				return values[0].Status
			}
			value := read()
			if value.Task.StartedAt == nil || value.Task.EndedAt == nil || value.ExitCode == nil {
				t.Fatal("snapshot dropped pointer metadata")
			}
			*value.Task.StartedAt = time.Unix(300, 0)
			*value.Task.EndedAt = time.Unix(400, 0)
			*value.ExitCode = 99
			actual := read()
			if !actual.Task.StartedAt.Equal(time.Unix(100, 0)) {
				t.Error("snapshot caller changed owned task start")
			}
			if !actual.Task.EndedAt.Equal(time.Unix(200, 0)) {
				t.Error("snapshot caller changed owned task end")
			}
			if *actual.ExitCode != 7 {
				t.Error("snapshot caller changed owned exit code")
			}
		})
	}
}
