// INPUT: Deterministic physical-cleanup fault injection and real product/native SQLite application stores
// OUTPUT: Cleanup retry retains facts, preserves strong errors and closes stores only after physical completion
// POS: [L7 Hub product] Owner fault regression complements actual AEP subprocess early-observer cleanup tests
package bootstrap

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/extension/hooks"
	owned "github.com/QuincySx/Alva/go/platform/system/process"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type cleanupFixture struct {
	complete atomic.Bool
	failure  error
}

func (f *cleanupFixture) Close(ctx context.Context) error {
	if f.complete.Load() {
		return nil
	}
	return errors.Join(f.failure, ctx.Err())
}
func (f *cleanupFixture) CleanupComplete() bool { return f.complete.Load() }
func TestApplicationCloseKeepsFactsUntilOwnedCleanupPhysicallyCompletes(t *testing.T) {
	ctx := context.Background()
	a, err := Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = a.Close(ctx) })
	session, err := a.Service.Create(ctx, "resource fault", "")
	if err != nil {
		t.Fatal(err)
	}
	failure := errors.New("fixture journal failure with incomplete native cleanup")
	resource := &cleanupFixture{failure: failure}
	runner := &ownedRunner{ownerID: session.ID, nativeID: "fault-native", resources: []attemptResource{resource}}
	a.pending = map[*ownedRunner]struct{}{runner: {}}
	runner.onClosed = func() { a.pendingMu.Lock(); delete(a.pending, runner); a.pendingMu.Unlock() }
	err = a.Close(ctx)
	var strong *abi.SessionError
	if !errors.Is(err, failure) || !errors.As(err, &strong) || runner.CleanupComplete() || a.closed {
		t.Fatal("resource/facts lifecycle falsely completed", err)
	}
	if _, err = a.Store.Detail(ctx, session.ID); err != nil {
		t.Fatal("product facts closed under live cleanup", err)
	}
	if _, err = a.Repository.List(ctx, session.ID, "", 1); err != nil {
		t.Fatal("native facts closed under live cleanup", err)
	}
	resource.complete.Store(true)
	err = a.Close(ctx)
	if !errors.Is(err, failure) || !a.closed || !runner.CleanupComplete() || len(a.pending) != 0 {
		t.Fatal("physical retry hid original failure or retained resource", err)
	}
	if _, err = a.Store.Detail(ctx, session.ID); err == nil {
		t.Fatal("completed cleanup did not close stores")
	}
}

type unusedHookModel struct{}

func (unusedHookModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return errors.New("unexpected model execution")
}
func TestApplicationCloseJoinsActualHookNotificationBeforeFacts(t *testing.T) {
	ctx := context.Background()
	a, err := Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close(ctx)
	product, err := a.Service.Create(ctx, "hook cleanup", "")
	if err != nil {
		t.Fatal(err)
	}
	log, err := a.Repository.Create(ctx, product.ID, "hook-native", "")
	if err != nil {
		t.Fatal(err)
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	entered, release := make(chan struct{}), make(chan struct{})
	plugin, err := hooks.New(ctx, hooks.Config{Repository: a.Repository, Owner: product.ID, SessionID: log.ID(), Cwd: product.Workspace, Entries: []hooks.Entry{{ID: "notification", Event: hooks.Notification, ApprovedHostAccess: true, Process: owned.Config{Command: executable, Args: []string{"-test.run=^$"}, Cwd: product.Workspace}}}, Report: func(ctx context.Context, _ hooks.Activity) error { close(entered); <-release; return ctx.Err() }})
	if err != nil {
		t.Fatal(err)
	}
	resource, ok := any(plugin).(attemptResource)
	if !ok {
		t.Fatal("actual Hook plugin has no owned cleanup contract")
	}
	state, err := abi.NewAgentState(unusedHookModel{}, log, nil)
	if err != nil {
		t.Fatal(err)
	}
	runtime := abi.RuntimeContext{State: state, RunID: "actual-hook-run", Workspace: product.Workspace}
	if err := plugin.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- plugin.Notify(ctx, runtime, "fixture") }()
	<-entered
	runner := &ownedRunner{ownerID: product.ID, nativeID: log.ID(), resources: []attemptResource{resource}}
	a.pending = map[*ownedRunner]struct{}{runner: {}}
	runner.onClosed = func() { a.pendingMu.Lock(); delete(a.pending, runner); a.pendingMu.Unlock() }
	deadline, cancel := context.WithTimeout(ctx, 40*time.Millisecond)
	closeErr := a.Close(deadline)
	cancel()
	if !errors.Is(closeErr, context.DeadlineExceeded) || a.closed || resource.CleanupComplete() {
		t.Error("Hook callback owner/facts closed early", closeErr)
	}
	if _, err := a.Repository.OpenSession(ctx, product.ID, log.ID()); err != nil {
		t.Error("native facts closed under Hook callback", err)
	}
	late, stopLate := context.WithTimeout(ctx, 40*time.Millisecond)
	if err := plugin.OnRunEnd(late, runtime, nil); err != nil {
		t.Error("terminal Hook owner admitted a late finalizer", err)
	}
	stopLate()
	close(release)
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Error("Hook callback did not observe owner cancellation", err)
	}
	if err := a.Close(ctx); !errors.Is(err, context.DeadlineExceeded) || !a.closed || !resource.CleanupComplete() {
		t.Error("retry did not settle physical cleanup and retain earlier error", err)
	}
	if err := plugin.OnRunStart(ctx, runtime); err == nil {
		t.Error("explicitly closed Hook plugin reopened")
	}
}
