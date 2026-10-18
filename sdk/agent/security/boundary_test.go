// INPUT: context, errors, testing, kernel/abi
// OUTPUT: Typed-nil security capabilities, nil adapters and resolver cancellation regressions
// POS: [L3 capability library] Rejects missing authority before invocation and preserves cancellation after host resolution.
package security

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type absentResolver struct{ PathResolver }
type absentApprovalHandler struct{ ApprovalHandler }

func TestGuardRejectsTypedNilResolverButKeepsOptionalNil(t *testing.T) {
	for _, tc := range []struct {
		name     string
		resolver PathResolver
	}{{"function", PathResolverFunc(nil)}, {"pointer", (*absentResolver)(nil)}} {
		t.Run(tc.name, func(t *testing.T) {
			if guard, err := NewGuard("/work", nil, tc.resolver, nil); err == nil || guard != nil {
				t.Fatal("missing host resolver admitted", guard, err)
			}
		})
	}
	guard, err := NewGuard("/work", nil, nil, nil)
	if err != nil {
		t.Fatal("optional absent resolver rejected at construction", err)
	}
	if resolved, err := guard.CheckPath(t.Context(), "file", abi.AccessRead); err == nil || resolved != "" {
		t.Fatal("absent resolver granted a resolved path", resolved, err)
	}
}

func TestPermissionManagerTreatsTypedNilHandlerAsUnavailable(t *testing.T) {
	for _, tc := range []struct {
		name    string
		handler ApprovalHandler
	}{{"function", ApprovalFunc(nil)}, {"pointer", (*absentApprovalHandler)(nil)}} {
		t.Run(tc.name, func(t *testing.T) {
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("missing approval handler panicked: %v", value)
				}
			}()
			manager := NewPermissionManager(Ask, tc.handler)
			request := ApprovalRequest{SessionID: "session", RunID: "run", CallID: "call", ToolName: "execute", Effect: abi.Effect{Kind: abi.EffectExecute}}
			if err := manager.Authorize(t.Context(), request); !errors.Is(err, ErrApprovalRequired) {
				t.Fatal("missing handler did not fail closed", err)
			}
			request.Effect.Kind = abi.EffectRead
			if err := manager.Authorize(t.Context(), request); err != nil {
				t.Fatal("missing optional handler changed automatic read policy", err)
			}
		})
	}
}

func TestNilSecurityFunctionAdaptersReturnErrors(t *testing.T) {
	t.Run("resolver", func(t *testing.T) {
		defer func() {
			if value := recover(); value != nil {
				t.Errorf("nil resolver adapter panicked: %v", value)
			}
		}()
		if resolved, err := PathResolverFunc(nil).Resolve(t.Context(), "/work/file", abi.AccessRead); err == nil || resolved != "" {
			t.Fatal("nil resolver adapter returned a path", resolved, err)
		}
	})
	t.Run("approval", func(t *testing.T) {
		defer func() {
			if value := recover(); value != nil {
				t.Errorf("nil approval adapter panicked: %v", value)
			}
		}()
		if decision, err := ApprovalFunc(nil).Approve(t.Context(), ApprovalRequest{}); err == nil || decision.Allowed {
			t.Fatal("nil approval adapter returned a decision", decision, err)
		}
	})
}

func TestGuardRejectsPathReturnedAfterResolverCancellation(t *testing.T) {
	fixture := errors.New("host resolver failed")
	for _, failure := range []error{nil, fixture} {
		t.Run(map[bool]string{false: "success", true: "failure"}[failure != nil], func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			guard, err := NewGuard("/work", nil, PathResolverFunc(func(context.Context, string, abi.Access) (string, error) {
				cancel()
				return "/work/file", failure
			}), nil)
			if err != nil {
				t.Fatal(err)
			}
			resolved, err := guard.CheckPath(ctx, "file", abi.AccessRead)
			if resolved != "" || !errors.Is(err, context.Canceled) {
				t.Error("resolver cancellation was lost", resolved, err)
			}
			if failure != nil && !errors.Is(err, failure) {
				t.Error("resolver failure was lost", err)
			}
		})
	}
}
