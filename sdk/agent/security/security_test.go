// INPUT: context, errors, testing, kernel/abi, llm/wire
// OUTPUT: Fail-closed permission, cache isolation and ordered path validation regressions
// POS: [L3 capability library] Proves policy decisions do not imply host sandbox enforcement or inherit authority across sessions.
package security

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"testing"
)

func TestGuardChecksSensitiveAndRootsBeforeResolutionOrApproval(t *testing.T) {
	resolved, approved := 0, 0
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		approved++
		return ApprovalDecision{true, Once}, nil
	}))
	guard, err := NewGuard("/work", nil, PathResolverFunc(func(_ context.Context, path string, _ abi.Access) (string, error) { resolved++; return path, nil }), manager)
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{".env", "../outside", "/work-other/file", ".ssh/id_rsa"} {
		if _, err := guard.CheckPath(context.Background(), path, abi.AccessRead); err == nil {
			t.Fatal("unsafe path allowed", path)
		}
	}
	if resolved != 0 || approved != 0 {
		t.Fatal("unsafe path reached later authority")
	}
	if path, err := guard.CheckPath(context.Background(), "notes.txt", abi.AccessRead); err != nil || path != "/work/notes.txt" {
		t.Fatal(path, err)
	}
}
func TestGuardRejectsResolvedEscapeAndNoResolver(t *testing.T) {
	for _, resolved := range []string{"/outside/file", "/work/.env.local", "relative"} {
		guard, _ := NewGuard("/work", nil, PathResolverFunc(func(context.Context, string, abi.Access) (string, error) { return resolved, nil }), nil)
		if _, err := guard.CheckPath(context.Background(), "safe", abi.AccessRead); err == nil {
			t.Fatal("resolver escape allowed", resolved)
		}
	}
	guard, _ := NewGuard("/work", nil, nil, nil)
	if _, err := guard.CheckPath(context.Background(), "safe", abi.AccessRead); err == nil {
		t.Fatal("lexical check pretended to be sandbox")
	}
}
func TestPermissionModesAndSessionCacheIsolation(t *testing.T) {
	calls := 0
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		calls++
		return ApprovalDecision{true, Session}, nil
	}))
	request := ApprovalRequest{SessionID: "one", ToolName: "write", Effect: abi.Effect{Kind: abi.EffectWrite}}
	for i := 0; i < 2; i++ {
		if err := manager.Authorize(context.Background(), request); err != nil {
			t.Fatal(err)
		}
	}
	request.SessionID = "two"
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	if calls != 2 {
		t.Fatal("authority leaked sessions", calls)
	}
	if err := manager.SetMode(Plan); err != nil {
		t.Fatal(err)
	}
	if err := manager.Authorize(context.Background(), request); !errors.Is(err, ErrPermissionDenied) {
		t.Fatal("plan ignored cached write denial", err)
	}
	request.Effect.Kind = abi.EffectRead
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
}
func TestLateApprovalAfterCancellationCannotCache(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		cancel()
		return ApprovalDecision{true, Session}, nil
	}))
	request := ApprovalRequest{SessionID: "one", ToolName: "exec", Effect: abi.Effect{Kind: abi.EffectExecute}}
	if err := manager.Authorize(ctx, request); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if len(manager.cache) != 0 {
		t.Fatal("late approval granted future authority")
	}
}
func TestGuardAuthorizePlanAndReadOnlyExtraRoot(t *testing.T) {
	manager := NewPermissionManager(Plan, nil)
	guard, _ := NewGuard("/work", []AuthorizedRoot{{"/readonly", abi.AccessRead}}, PathResolverFunc(func(_ context.Context, path string, _ abi.Access) (string, error) { return path, nil }), manager)
	if _, err := guard.CheckPath(context.Background(), "/readonly/a", abi.AccessWrite); err == nil {
		t.Fatal("read-only root writable")
	}
	execution := abi.ToolExecutionContext{SessionID: "s", RunID: "r", CallID: "c"}
	call := wire.ToolCall{ID: "c", Name: "shell"}
	if err := guard.Authorize(context.Background(), execution, call, abi.Effect{Kind: abi.EffectExecute}); !errors.Is(err, ErrPermissionDenied) {
		t.Fatal(err)
	}
}
