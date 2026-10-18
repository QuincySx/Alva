// INPUT: context, errors, testing, time, agent/security.
// OUTPUT: Explicit approval, session ownership, cancellation and bounded pending regressions.
// POS: [platform / native execution host] Headless HITL lifecycle tests without executing host capabilities.
package host

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"strings"
	"testing"
	"time"
)

func TestApprovalOversizeMetadataFailsBeforePublishing(t *testing.T) {
	b := NewApprovalBroker(1)
	_, err := b.Approve(context.Background(), security.ApprovalRequest{SessionID: "native", RunID: "run", CallID: "call", ToolName: "tool", Effect: abi.Effect{Description: strings.Repeat("x", 16<<10)}})
	if err == nil || len(b.Pending("native")) != 0 {
		t.Fatal("oversize approval published/authorized")
	}
}

func waitApproval(t *testing.T, b *ApprovalBroker, session string) security.ApprovalRequest {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		requests := b.Pending(session)
		if len(requests) > 0 {
			return requests[0]
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("approval was not published")
	return security.ApprovalRequest{}
}
func TestApprovalExplicitSessionBoundDecision(t *testing.T) {
	b := NewApprovalBroker(2)
	defer b.Close()
	result := make(chan error, 1)
	go func() {
		decision, err := b.Approve(context.Background(), security.ApprovalRequest{SessionID: "native", RunID: "run", CallID: "call", ToolName: "shell"})
		if err == nil && !decision.Allowed {
			err = errors.New("decision lost")
		}
		result <- err
	}()
	request := waitApproval(t, b, "native")
	if len(b.Pending("foreign")) != 0 {
		t.Fatal("foreign pending disclosure")
	}
	if err := b.Respond(context.Background(), "foreign", request.ID, security.ApprovalDecision{Allowed: true, Scope: security.Once}); !errors.Is(err, ErrApprovalExpired) {
		t.Fatal("foreign approval allowed")
	}
	if err := b.Respond(context.Background(), "native", request.ID, security.ApprovalDecision{Allowed: true, Scope: security.Once}); err != nil {
		t.Fatal(err)
	}
	if err := <-result; err != nil {
		t.Fatal(err)
	}
	if err := b.Respond(context.Background(), "native", request.ID, security.ApprovalDecision{Allowed: true, Scope: security.Once}); !errors.Is(err, ErrApprovalExpired) {
		t.Fatal("approval replayed")
	}
}
func TestApprovalCancellationCloseAndBudgetFailClosed(t *testing.T) {
	b := NewApprovalBroker(1)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	request := security.ApprovalRequest{SessionID: "native", RunID: "run", CallID: "call", ToolName: "shell"}
	go func() { _, err := b.Approve(ctx, request); done <- err }()
	pending := waitApproval(t, b, "native")
	if _, err := b.Approve(context.Background(), request); err == nil {
		t.Fatal("pending limit ignored")
	}
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if err := b.Respond(context.Background(), "native", pending.ID, security.ApprovalDecision{Allowed: true, Scope: security.Session}); !errors.Is(err, ErrApprovalExpired) {
		t.Fatal("expired approval cached")
	}
	go func() { _, err := b.Approve(context.Background(), request); done <- err }()
	waitApproval(t, b, "native")
	b.Close()
	if err := <-done; err == nil {
		t.Fatal("Close approved pending request")
	}
}

func TestTrustedPendingSnapshotKeepsActualChildIDsAndDetachedMetadata(t *testing.T) {
	b := NewApprovalBroker(2)
	defer b.Close()
	done := make(chan error, 2)
	for _, id := range []string{"root", "child"} {
		id := id
		go func() {
			_, err := b.Approve(context.Background(), security.ApprovalRequest{SessionID: id, RunID: "run", CallID: "call", ToolName: "actual", Effect: abi.Effect{Resources: []abi.ResourceClaim{{Resource: "original", Access: abi.AccessWrite}}}})
			done <- err
		}()
		waitApproval(t, b, id)
	}
	snapshot := b.PendingSnapshot()
	if len(snapshot) != 2 {
		t.Fatal("bounded host snapshot missing child", snapshot)
	}
	snapshot[0].Effect.Resources[0].Resource = "mutated"
	for _, request := range b.PendingSnapshot() {
		if request.Effect.Resources[0].Resource != "original" {
			t.Fatal("snapshot borrowed internal metadata")
		}
		if err := b.Respond(context.Background(), request.SessionID, request.ID, security.ApprovalDecision{Allowed: false, Scope: security.Once}); err != nil {
			t.Fatal(err)
		}
	}
	for range 2 {
		if err := <-done; err != nil {
			t.Fatal(err)
		}
	}
}
