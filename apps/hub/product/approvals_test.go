// INPUT: Real private stores, two distinct engine branches sharing a native identity string and an actual approval broker.
// OUTPUT: Native approval observation/response cannot borrow foreign-engine ownership or permission callbacks.
// POS: [L7 product approval tests] Cold routing and local callback fixtures grant no external execution or account authority.
package product

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

type foreignPermissionEngine struct {
	native  string
	started chan struct{}
	calls   atomic.Int32
}

func (e *foreignPermissionEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: e.native}); err != nil {
		return err
	}
	close(e.started)
	<-ctx.Done()
	return ctx.Err()
}
func (*foreignPermissionEngine) Cancel(context.Context, string) error { return nil }
func (e *foreignPermissionEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	e.calls.Add(1)
	return nil
}
func (*foreignPermissionEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Cancel: true, PermissionCallback: true}
}

func TestNativeApprovalCannotBorrowForeignEngineWithSameNativeID(t *testing.T) {
	s, requests := fixture(t)
	s.Approvals = host.NewApprovalBroker(8)
	t.Cleanup(func() { _ = s.Approvals.Close() })
	native := "same-string-different-authority"
	owner, err := s.Create(t.Context(), "Alva owner", "")
	if err != nil {
		t.Fatal(err)
	}
	detail, err := s.Store.Detail(t.Context(), owner.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Repository.Create(t.Context(), owner.ID, native, ""); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.BindNative(t.Context(), owner.ID, detail.Session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.BeginTurn(t.Context(), owner.ID, "actual native approval fixture"); err != nil {
		t.Fatal(err)
	}
	decided := make(chan security.ApprovalDecision, 1)
	go func() {
		decision, _ := s.Approvals.Approve(t.Context(), security.ApprovalRequest{SessionID: native, RunID: "native-run", CallID: "native-call", ToolName: "write_file"})
		decided <- decision
	}()
	deadline := time.Now().Add(time.Second)
	for len(s.Approvals.PendingSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	pending, err := s.PendingApprovals(t.Context(), owner.ID)
	if err != nil || len(pending) != 1 {
		t.Fatal("actual native permission did not become visible", err)
	}
	other, err := s.Create(t.Context(), "Foreign owner", "")
	if err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "foreign-permission"
	s.Targets = append(s.Targets, target)
	if _, err = s.Store.Switch(t.Context(), other.ID, target, "explicit foreign engine"); err != nil {
		t.Fatal(err)
	}
	foreign := &foreignPermissionEngine{native: native, started: make(chan struct{})}
	if err = s.Runtime.Register(target.EngineID, foreign); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(t.Context(), other.ID, "explicit local engine fixture", nil); err != nil {
		t.Fatal(err)
	}
	select {
	case <-foreign.started:
	case <-time.After(time.Second):
		t.Fatal("foreign local owner did not bind")
	}
	if shown, err := s.PendingApprovals(t.Context(), other.ID); err != nil || len(shown) != 0 {
		t.Error("foreign engine displayed native approval authority", shown, err)
	}
	if err = s.RespondApproval(t.Context(), other.ID, pending[0].ID, true); !errors.Is(err, host.ErrApprovalExpired) || foreign.calls.Load() != 0 {
		t.Error("native approval was sent to a foreign permission callback", foreign.calls.Load(), err)
	}
	if err = s.RespondNativeApproval(t.Context(), target.EngineID, native, pending[0].ID, true); !errors.Is(err, host.ErrApprovalExpired) {
		t.Error("foreign native string borrowed the Alva broker", err)
	}
	if len(s.Approvals.PendingSnapshot()) != 1 || len(requests) != 0 {
		t.Fatal("foreign observation or response consumed native work or ran the native model")
	}
	if err = s.RespondNativeApproval(t.Context(), "alva", native, pending[0].ID, false); err != nil {
		t.Fatal("actual native owner lost its separate permission", err)
	}
	select {
	case decision := <-decided:
		if decision.Allowed || decision.Scope != security.Once {
			t.Fatal("native denial was changed", decision)
		}
	case <-time.After(time.Second):
		t.Fatal("native denial was not delivered")
	}
}

type approvalReadTransitionStore struct {
	ServiceStore
	afterRead func()
}

func (s *approvalReadTransitionStore) Detail(ctx context.Context, id string) (hub.Detail, error) {
	detail, err := s.ServiceStore.Detail(ctx, id)
	if err == nil && s.afterRead != nil {
		transition := s.afterRead
		s.afterRead = nil
		transition()
	}
	return detail, err
}

func TestNativeApprovalReadDispatchRejectsReplacementOwner(t *testing.T) {
	s, _ := fixture(t)
	s.Runtime = hub.New(s.Store)
	t.Cleanup(func() { _ = s.Runtime.Close(context.Background()) })
	s.Approvals = host.NewApprovalBroker(8)
	t.Cleanup(func() { _ = s.Approvals.Close() })
	owner, err := s.Create(t.Context(), "actual native owner replacement", "")
	if err != nil {
		t.Fatal(err)
	}
	const native = "native-before-replacement"
	if _, err = s.Repository.Create(t.Context(), owner.ID, native, ""); err != nil {
		t.Fatal(err)
	}
	first := &foreignPermissionEngine{native: native, started: make(chan struct{})}
	if err = s.Runtime.Register("alva", first); err != nil {
		t.Fatal(err)
	}
	if _, _, err = s.Runtime.Start(t.Context(), owner.ID, "first actual owner"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-first.started:
	case <-time.After(time.Second):
		t.Fatal("initial native owner did not bind")
	}
	// Leave the actual broker request independently pending after replacement,
	// so dispatch must reject its original owner binding rather than rely on TTL.
	go func() {
		_, _ = s.Approvals.Approve(t.Context(), security.ApprovalRequest{SessionID: native, RunID: "native-run", CallID: "native-call", ToolName: "write_file"})
	}()
	deadline := time.Now().Add(time.Second)
	for len(s.Approvals.PendingSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	pending := s.Approvals.PendingSnapshot()
	if len(pending) != 1 {
		t.Fatal("native broker request did not become pending")
	}
	foreign := &foreignPermissionEngine{native: "replacement-native", started: make(chan struct{})}
	target := s.DefaultTarget
	target.EngineID = "replacement-foreign"
	if err = s.Runtime.Register(target.EngineID, foreign); err != nil {
		t.Fatal(err)
	}
	original := s.Store
	transitioned := false
	s.Store = &approvalReadTransitionStore{ServiceStore: original, afterRead: func() {
		if err := s.Runtime.CancelAndWait(t.Context(), owner.ID); err != nil {
			t.Fatal(err)
		}
		if _, err := original.Switch(t.Context(), owner.ID, target, "explicit replacement after detail snapshot"); err != nil {
			t.Fatal(err)
		}
		if _, _, err := s.Runtime.Start(t.Context(), owner.ID, "replacement actual owner"); err != nil {
			t.Fatal(err)
		}
		select {
		case <-foreign.started:
		case <-time.After(time.Second):
			t.Fatal("replacement owner did not bind")
		}
		transitioned = true
	}}
	err = s.RespondApproval(t.Context(), owner.ID, pending[0].ID, true)
	if !transitioned || !errors.Is(err, host.ErrApprovalExpired) || first.calls.Load() != 0 || foreign.calls.Load() != 0 || len(s.Approvals.PendingSnapshot()) != 1 {
		t.Fatal("stale native permission crossed its observed execution owner", transitioned, first.calls.Load(), foreign.calls.Load(), err)
	}
}
