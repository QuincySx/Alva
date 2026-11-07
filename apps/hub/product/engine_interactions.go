// INPUT: Committed Runtime interaction snapshots and active product branch/native ownership
// OUTPUT: Opaque frozen interaction handles and separately bound permission/question responses
// POS: [L7 Hub product] Native engine callbacks never borrow Alva brokers or rediscover a replacement owner on a late click
package product

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// EngineInteractionHandle is obtained only from a validated pending snapshot.
// Its private fields cannot be constructed from client-selected native IDs.
type EngineInteractionHandle struct {
	session string
	branch  string
	request string
	kind    engine.Kind
	binding hub.PermissionBinding
}

func (h EngineInteractionHandle) ID() string {
	data, _ := json.Marshal([]string{h.session, h.branch, h.request, string(h.kind), h.binding.ExecutionID, h.binding.AttemptID, h.binding.EngineID, h.binding.NativeSessionID})
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

type EngineInteraction struct {
	Handle   EngineInteractionHandle
	Event    engine.Event
	BranchID string
	EngineID string
}

func (s *HubService) PendingEngineInteractions(ctx context.Context, id string) ([]EngineInteraction, error) {
	if s.Runtime == nil {
		return nil, engine.ErrUnsupported
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return nil, err
	}
	if detail.Session.Status != hub.Running {
		return nil, nil
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return nil, err
	}
	if branch.Target.EngineID == "alva" || branch.NativeSessionID == "" {
		return nil, nil
	}
	var result []EngineInteraction
	for _, pending := range s.Runtime.PendingEngineInteractions(id) {
		binding, event := pending.Binding, pending.Event
		if binding.EngineID != branch.Target.EngineID || binding.NativeSessionID != branch.NativeSessionID || event.SessionID != "" && event.SessionID != branch.NativeSessionID || binding.ExecutionID == "" || binding.AttemptID == "" || event.ID == "" {
			continue
		}
		switch event.Kind {
		case engine.PermissionRequest:
			if event.Name == "" {
				return nil, errors.New("invalid native engine permission")
			}
		case engine.UserInputRequested:
			if event.Question == nil || event.Question.Validate() != nil {
				return nil, errors.New("invalid native engine question")
			}
		default:
			continue
		}
		result = append(result, EngineInteraction{Handle: EngineInteractionHandle{session: id, branch: branch.ID, request: event.ID, kind: event.Kind, binding: binding}, Event: event, BranchID: branch.ID, EngineID: binding.EngineID})
	}
	return result, nil
}

func (s *HubService) validateEngineInteraction(ctx context.Context, id string, handle EngineInteractionHandle, kind engine.Kind) error {
	if handle.session != id || handle.branch == "" || handle.request == "" || handle.kind != kind || handle.binding.EngineID == "" || handle.binding.EngineID == "alva" || handle.binding.ExecutionID == "" || handle.binding.AttemptID == "" || handle.binding.NativeSessionID == "" {
		return engine.ErrSessionNotFound
	}
	// Re-read product ownership, then compare the original opaque handle against
	// the live cache. The supplied handle is never replaced by this observation.
	pending, err := s.PendingEngineInteractions(ctx, id)
	if err != nil {
		return err
	}
	for _, candidate := range pending {
		if candidate.Handle == handle {
			return nil
		}
	}
	return engine.ErrSessionNotFound
}

func (s *HubService) RespondEnginePermission(ctx context.Context, id string, handle EngineInteractionHandle, allow bool) error {
	if err := s.validateEngineInteraction(ctx, id, handle, engine.PermissionRequest); err != nil {
		return err
	}
	return s.Runtime.RespondPermissionBound(ctx, id, handle.binding, handle.request, engine.PermissionDecision{Allow: allow})
}

func (s *HubService) RespondEngineUserInput(ctx context.Context, id string, handle EngineInteractionHandle, response engine.UserInputResponse) error {
	if err := s.validateEngineInteraction(ctx, id, handle, engine.UserInputRequested); err != nil {
		return err
	}
	return s.Runtime.RespondUserInputBound(ctx, id, handle.binding, handle.request, response)
}
