// INPUT: context, crypto/rand, sync, agent/security, kernel/abi.
// OUTPUT: ApprovalBroker with bounded pending requests and explicit session-bound decisions.
// POS: [platform / native execution host] Native HITL rendezvous owned by execution rather than UI observation.
package host

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sort"
	"sync"

	"github.com/QuincySx/Alva/sdk/agent/security"
)

var ErrApprovalExpired = errors.New("approval is unavailable, expired or belongs to another session")

type approvalReply struct {
	decision security.ApprovalDecision
	err      error
}
type pendingApproval struct {
	request security.ApprovalRequest
	ctx     context.Context
	reply   chan approvalReply
}
type ApprovalBroker struct {
	mu       sync.Mutex
	capacity int
	closed   bool
	pending  map[string]*pendingApproval
}

func NewApprovalBroker(capacity int) *ApprovalBroker {
	if capacity < 1 {
		capacity = 32
	}
	if capacity > 128 {
		capacity = 128
	}
	return &ApprovalBroker{capacity: capacity, pending: make(map[string]*pendingApproval)}
}

func (b *ApprovalBroker) Approve(ctx context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
	if err := ctx.Err(); err != nil {
		return security.ApprovalDecision{}, err
	}
	if request.SessionID == "" || request.RunID == "" || request.CallID == "" || request.ToolName == "" {
		return security.ApprovalDecision{}, errors.New("approval requires actual session/run/call/tool identity")
	}
	var id [16]byte
	if _, err := rand.Read(id[:]); err != nil {
		return security.ApprovalDecision{}, err
	}
	request.ID = hex.EncodeToString(id[:])
	request = cloneApproval(request)
	encoded, encodeErr := json.Marshal(request)
	if encodeErr != nil || len(encoded) > 16<<10 {
		return security.ApprovalDecision{}, errors.New("approval metadata exceeds 16 KiB budget; no permission granted")
	}
	b.mu.Lock()
	if b.closed {
		b.mu.Unlock()
		return security.ApprovalDecision{}, errors.New("approval broker is closed")
	}
	if len(b.pending) >= b.capacity {
		b.mu.Unlock()
		return security.ApprovalDecision{}, errors.New("approval pending budget exceeded")
	}
	pending := &pendingApproval{request: request, ctx: ctx, reply: make(chan approvalReply, 1)}
	b.pending[request.ID] = pending
	b.mu.Unlock()
	defer func() { b.mu.Lock(); delete(b.pending, request.ID); b.mu.Unlock() }()
	select {
	case <-ctx.Done():
		return security.ApprovalDecision{}, ctx.Err()
	case reply := <-pending.reply:
		if err := ctx.Err(); err != nil {
			return security.ApprovalDecision{}, err
		}
		return reply.decision, reply.err
	}
}

// SessionID is injected from the currently bound product/native session, never
// accepted as an independent authorization grant from an untrusted caller.
func (b *ApprovalBroker) Pending(sessionID string) []security.ApprovalRequest {
	b.mu.Lock()
	defer b.mu.Unlock()
	var requests []security.ApprovalRequest
	for _, pending := range b.pending {
		if pending.request.SessionID == sessionID && pending.ctx.Err() == nil {
			requests = append(requests, cloneApproval(pending.request))
		}
	}
	sort.Slice(requests, func(i, j int) bool { return requests[i].ID < requests[j].ID })
	return requests
}

// PendingSnapshot is a bounded trusted-host view, not a UI authorization API.
// Products must independently verify each real child session's typed ancestry
// to the currently claimed active root before showing/responding. It never
// invokes caller code while holding the broker lock or relabels child IDs.
func (b *ApprovalBroker) PendingSnapshot() []security.ApprovalRequest {
	b.mu.Lock()
	defer b.mu.Unlock()
	var requests []security.ApprovalRequest
	for _, pending := range b.pending {
		if pending.ctx.Err() == nil {
			requests = append(requests, cloneApproval(pending.request))
		}
	}
	sort.Slice(requests, func(i, j int) bool { return requests[i].ID < requests[j].ID })
	return requests
}

func (b *ApprovalBroker) Respond(ctx context.Context, sessionID, id string, decision security.ApprovalDecision) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if decision.Scope != security.Once && decision.Scope != security.Session {
		return errors.New("invalid approval scope")
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	pending, exists := b.pending[id]
	if !exists || pending.request.SessionID != sessionID || pending.ctx.Err() != nil || b.closed {
		return ErrApprovalExpired
	}
	delete(b.pending, id)
	pending.reply <- approvalReply{decision: decision}
	return nil
}

func (b *ApprovalBroker) Close() error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.closed {
		return nil
	}
	b.closed = true
	for id, pending := range b.pending {
		delete(b.pending, id)
		pending.reply <- approvalReply{err: errors.New("approval broker closed without a decision")}
	}
	return nil
}

func cloneApproval(request security.ApprovalRequest) security.ApprovalRequest {
	request.Effect.Resources = append(request.Effect.Resources[:0:0], request.Effect.Resources...)
	return request
}
