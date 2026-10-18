// INPUT: context, errors, sync, sync/atomic, testing, kernel/abi
// OUTPUT: Invocation-scoped approval single-flight, effect isolation and invalidation regressions
// POS: [L3 capability library] Coalesces duplicate trusted authorization layers without turning Once into a cross-call grant.
package security

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"sync"
	"sync/atomic"
	"testing"
)

func approvalFixture() ApprovalRequest {
	return ApprovalRequest{SessionID: "session", RunID: "run", CallID: "call", ToolName: "mcp", Effect: abi.Effect{Kind: abi.EffectExecute, Description: "execute one operation", Resources: []abi.ResourceClaim{{Resource: "one", Access: abi.AccessWrite}, {Resource: "two", Access: abi.AccessRead}}}}
}
func TestApprovalOnceCoalescesExactInvocationOnly(t *testing.T) {
	var calls atomic.Int32
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		calls.Add(1)
		return ApprovalDecision{true, Once}, nil
	}))
	request := approvalFixture()
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	reordered := request
	reordered.Effect.Resources = append([]abi.ResourceClaim(nil), request.Effect.Resources...)
	reordered.Effect.Resources[0], reordered.Effect.Resources[1] = reordered.Effect.Resources[1], reordered.Effect.Resources[0]
	if err := manager.Authorize(context.Background(), reordered); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 {
		t.Fatal("duplicate layer reprompt", calls.Load())
	}
	for _, change := range []func(*ApprovalRequest){func(r *ApprovalRequest) { r.SessionID = "other" }, func(r *ApprovalRequest) { r.RunID = "other" }, func(r *ApprovalRequest) { r.CallID = "other" }, func(r *ApprovalRequest) { r.ToolName = "other" }, func(r *ApprovalRequest) { r.Effect.Description = "other" }, func(r *ApprovalRequest) {
		r.Effect.Resources = []abi.ResourceClaim{{Resource: "one", Access: abi.AccessRead}}
	}} {
		copy := request
		change(&copy)
		if err := manager.Authorize(context.Background(), copy); err != nil {
			t.Fatal(err)
		}
	}
	if calls.Load() != 7 {
		t.Fatal("Once grant escaped invocation/effect", calls.Load())
	}
}
func TestApprovalSingleFlightAndWaiterCancellation(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		if calls.Add(1) == 1 {
			close(started)
		}
		<-release
		return ApprovalDecision{true, Once}, nil
	}))
	request := approvalFixture()
	var wait sync.WaitGroup
	failures := make(chan error, 16)
	for i := 0; i < 16; i++ {
		wait.Add(1)
		go func() { defer wait.Done(); failures <- manager.Authorize(context.Background(), request) }()
	}
	<-started
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := manager.Authorize(ctx, request); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	close(release)
	wait.Wait()
	close(failures)
	for err := range failures {
		if err != nil {
			t.Fatal(err)
		}
	}
	if calls.Load() != 1 {
		t.Fatal(calls.Load())
	}
}
func TestApprovalInvalidatedPendingAndInfrastructureFailureNotMemoized(t *testing.T) {
	for _, invalidate := range []func(*PermissionManager){func(manager *PermissionManager) { manager.ClearSession("session") }, func(manager *PermissionManager) { _ = manager.SetMode(Plan) }} {
		started, release := make(chan struct{}), make(chan struct{})
		manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
			close(started)
			<-release
			return ApprovalDecision{true, Session}, nil
		}))
		result := make(chan error, 1)
		go func() { result <- manager.Authorize(context.Background(), approvalFixture()) }()
		<-started
		invalidate(manager)
		close(release)
		if err := <-result; !errors.Is(err, ErrPermissionDenied) {
			t.Fatal("late grant after invalidate", err)
		}
		if len(manager.cache) != 0 {
			t.Fatal("invalidated approval cached")
		}
	}
	calls := 0
	fixture := errors.New("approval transport failure")
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		calls++
		return ApprovalDecision{}, fixture
	}))
	for i := 0; i < 2; i++ {
		if err := manager.Authorize(context.Background(), approvalFixture()); !errors.Is(err, fixture) {
			t.Fatal(err)
		}
	}
	if calls != 2 {
		t.Fatal("infrastructure failure cached", calls)
	}
}

func TestClearSessionInvalidatesStandaloneLateGrantOnlyForThatSession(t *testing.T) {
	started := make(chan string, 2)
	release := make(chan struct{})
	manager := NewPermissionManager(Ask, ApprovalFunc(func(_ context.Context, request ApprovalRequest) (ApprovalDecision, error) {
		started <- request.SessionID
		<-release
		return ApprovalDecision{Allowed: true, Scope: Session}, nil
	}))
	first, second := make(chan error, 1), make(chan error, 1)
	go func() {
		first <- manager.Authorize(context.Background(), ApprovalRequest{SessionID: "one", ToolName: "exec", Effect: abi.Effect{Kind: abi.EffectExecute}})
	}()
	go func() {
		second <- manager.Authorize(context.Background(), ApprovalRequest{SessionID: "two", ToolName: "exec", Effect: abi.Effect{Kind: abi.EffectExecute}})
	}()
	<-started
	<-started
	manager.ClearSession("one")
	close(release)
	if err := <-first; !errors.Is(err, ErrPermissionDenied) {
		t.Fatal("standalone late grant survived ClearSession", err)
	}
	if err := <-second; err != nil {
		t.Fatal("unrelated session invalidated", err)
	}
	manager.mu.RLock()
	defer manager.mu.RUnlock()
	if len(manager.cache) != 1 {
		t.Fatal("late standalone cache repopulated", manager.cache)
	}
}
