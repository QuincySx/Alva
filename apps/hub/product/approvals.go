// INPUT: Active committed product route, typed child ancestry and bounded actual broker snapshot
// OUTPUT: Root/descendant pending approvals and real-native response without identity rewriting
// POS: [L7 Hub product] Trusted product mediation never treats arbitrary child IDs or metadata-only parent links as grants
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

type NativeOwnerResolver interface {
	NativeOwner(context.Context, string, string) (string, error)
}

func (s *HubService) typedDescendant(ctx context.Context, owner, root, child string) (bool, error) {
	seen := make(map[string]bool)
	current := child
	for depth := 0; depth <= 8; depth++ {
		if current == root {
			return true, nil
		}
		if depth == 8 {
			return false, nil
		}
		if current == "" || seen[current] {
			return false, errors.New("invalid child ancestry cycle")
		}
		seen[current] = true
		link, err := s.Repository.ParentLink(ctx, owner, current)
		if errors.Is(err, session.ErrNotFound) {
			return false, nil
		}
		if err != nil {
			return false, err
		}
		if link.ChildID != current || link.ParentID == current {
			return false, errors.New("typed child ancestry mismatch")
		}
		current = link.ParentID
	}
	return false, nil
}
func (s *HubService) pendingAtRoot(ctx context.Context, id, root string) ([]security.ApprovalRequest, error) {
	var result []security.ApprovalRequest
	for _, request := range s.Approvals.PendingSnapshot() {
		allowed, err := s.typedDescendant(ctx, id, root, request.SessionID)
		if err != nil {
			return nil, err
		}
		if allowed {
			result = append(result, request)
		}
	}
	return result, nil
}

// RespondNativeApproval is called only from an actually active Alva adapter,
// after Runtime's generation/permission dispatch checks. UI still supplies only
// product ID + opaque request ID, never a chosen child native identity.
func (s *HubService) RespondNativeApproval(ctx context.Context, engineID, root, requestID string, allow bool) error {
	if s.Approvals == nil {
		return host.ErrApprovalExpired
	}
	resolver, ok := s.Store.(NativeOwnerResolver)
	if !ok {
		return errors.New("product route owner resolver unavailable")
	}
	id, err := resolver.NativeOwner(ctx, engineID, root)
	if err != nil {
		return err
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return err
	}
	if detail.Session.Status != hub.Running || branch.Target.EngineID != engineID || branch.NativeSessionID != root {
		return host.ErrApprovalExpired
	}
	requests, err := s.pendingAtRoot(ctx, id, root)
	if err != nil {
		return err
	}
	for _, request := range requests {
		if request.ID == requestID {
			return s.Approvals.Respond(ctx, request.SessionID, requestID, security.ApprovalDecision{Allowed: allow, Scope: security.Once})
		}
	}
	return host.ErrApprovalExpired
}
