// INPUT: Actual application-owned fixture shell job, committed product branch and MyGo Tester controls
// OUTPUT: Cold native job list, explicit operator Stop and physical cleanup acknowledgement regression
// POS: [L7 Hub product] Real process ownership is exercised without a model request, OS window or ambient shell discovery.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/builtin"
	"github.com/QuincySx/Alva/extension/shell"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/egoist/mygo/ui"
)

type shellControlModel struct{}

func (shellControlModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return errors.New("native job controls must not invoke a model")
}

func TestNativeShellJobStopWaitsForItsActualApplicationOwner(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "job controls", "")
	if err != nil {
		t.Fatal(err)
	}
	log, err := s.Repository.Create(ctx, product.ID, "ui-shell-native", "")
	if err != nil {
		t.Fatal(err)
	}
	if err := s.Store.BindNative(ctx, product.ID, product.ActiveBranchID, log.ID()); err != nil {
		t.Fatal(err)
	}
	files, err := builtin.OpenWorkspace(product.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = files.Close() })
	guard, err := security.NewGuard(product.Workspace, nil, files, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	locks, err := core.NewToolLockRegistry(16)
	if err != nil {
		t.Fatal(err)
	}
	s.Shells = shell.NewSessions()
	t.Cleanup(func() {
		if err := s.Shells.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	lease, err := s.Shells.Bind(ctx, shell.Config{Repository: s.Repository, Owner: product.ID, SessionID: log.ID(), Cwd: product.Workspace, Executable: "/bin/sh", ApprovedHostAccess: true, Guard: guard, Locks: locks, Report: func(context.Context, shell.Activity) error { return nil }})
	if err != nil {
		t.Fatal(err)
	}
	state, err := abi.NewAgentState(shellControlModel{}, log, lease.Tools())
	if err != nil {
		t.Fatal(err)
	}
	runtime := abi.RuntimeContext{State: state, Workspace: product.Workspace, RunID: "ui-shell-run"}
	if err := lease.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	for _, tool := range lease.Tools() {
		if tool.Name() != "execute_shell" {
			continue
		}
		_, err = tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, Session: log, SessionID: log.ID(), Workspace: product.Workspace, RunID: runtime.RunID, CallID: "ui-shell-call"}, wire.ToolCall{ID: "ui-shell-call", Name: tool.Name(), Arguments: json.RawMessage(`{"command":"cat","run_in_background":true,"timeout_ms":60000}`)})
		if err != nil {
			t.Fatal(err)
		}
	}
	if err := lease.OnRunEnd(ctx, runtime, nil); err != nil {
		t.Fatal(err)
	}
	if err := lease.Close(ctx); err != nil {
		t.Fatal(err)
	}
	jobs, err := s.ShellJobs(ctx, product.ID)
	if err != nil || len(jobs) != 1 || jobs[0].Job.Status.Reaped {
		t.Fatal("actual live job missing", jobs, err)
	}
	p := NewNativePage(s)
	p.selected = product.ID
	p.detail, err = s.Store.Detail(ctx, product.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1500, 3000)
	if err := tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if err := tester.Click("Optional tools and human questions"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if len(p.shellJobs) != 1 || p.shellJobs[0].Job.Status.Reaped {
		t.Fatal("cold native job list lost owner")
	}
	if err := tester.Click("Stop owned Shell job " + jobs[0].Job.Status.Task.ID); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || len(p.shellJobs) != 1 || !p.shellJobs[0].Job.Status.Reaped || !s.Shells.CleanupComplete() {
		t.Fatal("native Stop claimed early success", p.problem, p.shellJobs)
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("native process controls invoked model execution")
	}
	// Keep bounded cleanup available even if a future assertion changes.
	closeCtx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	if err := s.Shells.Close(closeCtx); err != nil {
		t.Fatal(err)
	}
}
