// INPUT: Actual Runtime owner bindings and bounded engine permission/question events.
// OUTPUT: EngineInteraction snapshots and bound response dispatch with native-accepted suppression.
// POS: [L7 Hub product] Observes committed pending interactions without granting authority from history or native IDs.
package hub

import (
	"context"
	"errors"
	"sort"

	"github.com/QuincySx/Alva/integration/engine"
)

// EngineInteraction freezes the physical owner seen with a pending request.
// Binding grants nothing; responses must still match the live pending request.
type EngineInteraction struct {
	Binding PermissionBinding
	Event   engine.Event
}

type pendingEngineInteraction struct {
	event    engine.Event
	answered bool
}

func interactionKey(kind engine.Kind, id string) string { return string(kind) + "\x00" + id }

// A successful callback means the native owner accepted this response. Keep its
// event until the native resolved fact arrives, but stop offering it to another
// observer. Pointer identity protects a later request that reuses the same ID.
func (r *Runtime) markInteractionAnswered(sessionID string, run *activeRun, expected PermissionBinding, kind engine.Kind, requestID string, pending *pendingEngineInteraction) {
	if pending == nil {
		return // Alva permissions retain their independent broker authority.
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.active[sessionID] == run && permissionBinding(run) == expected && run.interactions[interactionKey(kind, requestID)] == pending {
		pending.answered = true
	}
}

func cloneInteractionEvent(event engine.Event) engine.Event {
	event.Input = append([]byte(nil), event.Input...)
	if event.Question != nil {
		request := *event.Question
		request.Questions = append([]engine.UserInputQuestion(nil), request.Questions...)
		for i := range request.Questions {
			request.Questions[i].Options = append([]engine.UserInputOption(nil), request.Questions[i].Options...)
		}
		event.Question = &request
	}
	return event
}

func (r *Runtime) PendingEngineInteractions(sessionID string) []EngineInteraction {
	r.mu.Lock()
	defer r.mu.Unlock()
	run := r.active[sessionID]
	if run == nil || run.engine == nil || run.nativeID == "" || run.executionID == "" || run.attemptID == "" {
		return nil
	}
	items := make([]EngineInteraction, 0, len(run.interactions))
	for _, pending := range run.interactions {
		if !pending.answered {
			items = append(items, EngineInteraction{Binding: permissionBinding(run), Event: cloneInteractionEvent(pending.event)})
		}
	}
	sort.Slice(items, func(i, j int) bool {
		return interactionKey(items[i].Event.Kind, items[i].Event.ID) < interactionKey(items[j].Event.Kind, items[j].Event.ID)
	})
	return items
}

// RespondUserInputBound answers a human question, never a permission request.
// It uses the same physical-owner lock order as Cancel/RespondPermissionBound.
func (r *Runtime) RespondUserInputBound(ctx context.Context, sessionID string, expected PermissionBinding, requestID string, response engine.UserInputResponse) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if expected.ExecutionID == "" || expected.AttemptID == "" || expected.EngineID == "" || expected.NativeSessionID == "" {
		return errors.New("Hub interaction binding required")
	}
	r.mu.Lock()
	run := r.active[sessionID]
	r.mu.Unlock()
	if run == nil {
		return ErrSessionNotRunning
	}
	run.control.Lock()
	defer run.control.Unlock()
	r.mu.Lock()
	if r.active[sessionID] != run || permissionBinding(run) != expected || run.engine == nil {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	pending := run.interactions[interactionKey(engine.UserInputRequested, requestID)]
	target := run.engine
	r.mu.Unlock()
	if pending == nil || pending.answered || pending.event.Question == nil {
		return engine.ErrSessionNotFound
	}
	if err := response.ValidateFor(*pending.event.Question); err != nil {
		return err
	}
	responder, ok := target.(engine.UserInputResponder)
	if !ok || !target.Capabilities().UserInputCallback {
		return engine.ErrUnsupported
	}
	if err := responder.RespondUserInput(ctx, expected.NativeSessionID, requestID, response); err != nil {
		return err
	}
	r.markInteractionAnswered(sessionID, run, expected, engine.UserInputRequested, requestID, pending)
	return nil
}
