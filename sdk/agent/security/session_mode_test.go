// INPUT: context, testing, actual concurrent permission callbacks
// OUTPUT: Session-only mode/cache invalidation and explicit global reset regressions
// POS: [L3 capability library] Preserves shared-manager child/sibling separation without inventing ancestry or model-side elevation authority.
package security

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"testing"
)

func TestSessionModeDoesNotChangeParentOrSiblingAndGlobalResetIsExplicit(t *testing.T) {
	m := NewPermissionManager(Ask, nil)
	if err := m.SetSessionMode("child", AcceptEdits); err != nil {
		t.Fatal(err)
	}
	write := ApprovalRequest{SessionID: "child", ToolName: "edit", Effect: abi.Effect{Kind: abi.EffectWrite}}
	if err := m.Authorize(context.Background(), write); err != nil {
		t.Fatal(err)
	}
	write.SessionID = "parent"
	if !errors.Is(m.Authorize(context.Background(), write), ErrApprovalRequired) {
		t.Fatal("child mode changed parent's admission")
	}
	if m.Mode() != Ask || m.ModeForSession("sibling") != Ask {
		t.Fatal("session mode changed base/sibling")
	}
	if err := m.SetSessionMode("child", Plan); err != nil {
		t.Fatal(err)
	}
	write.SessionID = "child"
	if !errors.Is(m.Authorize(context.Background(), write), ErrPermissionDenied) {
		t.Fatal("plan allowed edit")
	}
	m.ClearSession("child")
	if m.ModeForSession("child") != Ask {
		t.Fatal("clear retained override")
	}
	if err := m.SetSessionMode("child", AcceptEdits); err != nil {
		t.Fatal(err)
	}
	if err := m.SetMode(Plan); err != nil {
		t.Fatal(err)
	}
	if m.ModeForSession("child") != Plan || m.ModeForSession("parent") != Plan {
		t.Fatal("global change failed to reset overrides")
	}
}
func TestSessionModeInvalidatesOnlyItsPendingAndCachedApprovals(t *testing.T) {
	entered := make(chan string, 2)
	release := make(chan struct{})
	m := NewPermissionManager(Ask, ApprovalFunc(func(_ context.Context, r ApprovalRequest) (ApprovalDecision, error) {
		entered <- r.SessionID
		<-release
		return ApprovalDecision{Allowed: true, Scope: Session}, nil
	}))
	results := make(chan struct {
		id  string
		err error
	}, 2)
	for _, id := range []string{"changed", "sibling"} {
		go func() {
			err := m.Authorize(context.Background(), ApprovalRequest{SessionID: id, ToolName: "shell", Effect: abi.Effect{Kind: abi.EffectExecute}})
			results <- struct {
				id  string
				err error
			}{id, err}
		}()
	}
	<-entered
	<-entered
	if err := m.SetSessionMode("changed", Plan); err != nil {
		t.Fatal(err)
	}
	close(release)
	for i := 0; i < 2; i++ {
		r := <-results
		if r.id == "changed" && !errors.Is(r.err, ErrPermissionDenied) {
			t.Fatal("late approval restored cleared session", r.err)
		}
		if r.id == "sibling" && r.err != nil {
			t.Fatal("unrelated sibling approval revoked", r.err)
		}
	}
	if err := m.Authorize(context.Background(), ApprovalRequest{SessionID: "sibling", ToolName: "shell", Effect: abi.Effect{Kind: abi.EffectExecute}}); err != nil {
		t.Fatal("sibling cache removed", err)
	}
	if err := m.Authorize(context.Background(), ApprovalRequest{SessionID: "changed", ToolName: "shell", Effect: abi.Effect{Kind: abi.EffectExecute}}); !errors.Is(err, ErrPermissionDenied) {
		t.Fatal("changed session escaped plan", err)
	}
}
