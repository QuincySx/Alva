// INPUT: context, errors, hub PendingStore and Runtime interjection/exit observation, engine outcomes
// OUTPUT: TurnService.Queue/EditQueued/CancelQueued/Queued/SendQueued/Interject/Capabilities and the exit dispatcher that starts the queue head after a successful Turn
// POS: [L7 Hub product] Running-time input: Enter queues (durable, at most 8 of 16 KiB), Cmd+Enter interjects when the engine can and otherwise queues with a notice; the queue pauses after a failure or stop and after a crash
package product

import (
	"context"
	"errors"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// InterjectMode says what became of an interjection.
type InterjectMode string

const (
	// InterjectSent: the message was handed to the running engine. It is
	// shown in the conversation once the engine reports the delivery receipt.
	InterjectSent InterjectMode = "sent"
	// InterjectQueued: the engine cannot take it now; it waits in the queue.
	InterjectQueued InterjectMode = "queued"
)

// InterjectResult is the outcome of Interject.
type InterjectResult struct {
	Mode  InterjectMode
	Input hub.PendingInput
	// Notice explains a downgrade to the queue, for an inline hint.
	Notice string
}

// TurnCapabilities is what the session's active engine can do while running.
type TurnCapabilities struct {
	// Interject: a running Turn takes a message at its next model request.
	Interject bool
	// Fork: a version or fork continues the engine's own native context.
	Fork bool
}

// Capabilities reports the active engine's conversation operations. The UI
// shows the interject shortcut only when Interject is true.
func (t *TurnService) Capabilities(ctx context.Context, id string) (TurnCapabilities, error) {
	detail, err := t.service.Store.Detail(ctx, id)
	if err != nil {
		return TurnCapabilities{}, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return TurnCapabilities{}, err
	}
	return t.EngineCapabilities(branch.Target.EngineID), nil
}

// EngineCapabilities is Capabilities for an engine the caller already knows
// (the open session's active route); it reads nothing.
func (t *TurnService) EngineCapabilities(engineID string) TurnCapabilities {
	descriptor, ok := t.service.Runtime.Descriptor(engineID)
	if !ok || descriptor.Runtime == nil {
		return TurnCapabilities{}
	}
	caps := descriptor.Runtime.Capabilities()
	_, interjector := descriptor.Runtime.(engine.Interjector)
	return TurnCapabilities{Interject: caps.Interjection && interjector, Fork: caps.Fork}
}

// Queue appends a message to the conversation's durable queue. While a Turn
// runs it waits there. An idle conversation whose last Turn succeeded has
// nothing to wait for, so the head starts at once; one that is paused (the last
// Turn failed or was stopped, or older messages already wait) keeps the message
// queued until the user resumes it with SendQueued.
func (t *TurnService) Queue(ctx context.Context, id, text string) (hub.PendingInput, error) {
	item, err := t.service.Store.QueueInput(ctx, id, text, hub.PendingQueued)
	if err != nil {
		return item, err
	}
	if _, active := t.service.Runtime.ActiveExecution(id); active {
		return item, nil
	}
	if latest, latestErr := t.service.Store.LatestTurn(ctx, id); latestErr == nil && latest.Status != engine.Succeeded {
		return item, nil // paused
	} else if latestErr != nil && !errors.Is(latestErr, hub.ErrNotFound) {
		return item, nil
	}
	items, err := t.service.Store.PendingInputs(ctx, id)
	if err != nil || len(items) == 0 || items[0].ID != item.ID {
		return item, nil
	}
	if _, err = t.dispatch(ctx, id, item); err != nil && !errors.Is(err, hub.ErrPendingGone) && !errors.Is(err, hub.ErrSessionBusy) {
		_ = t.service.Store.NotePendingError(ctx, id, item.ID, "未能发送："+err.Error())
	}
	return item, nil
}

// Queued lists the queue in dispatch order.
func (t *TurnService) Queued(ctx context.Context, id string) ([]hub.PendingInput, error) {
	return t.service.Store.PendingInputs(ctx, id)
}

// EditQueued replaces a still-queued message's text.
func (t *TurnService) EditQueued(ctx context.Context, id, inputID, text string) (hub.PendingInput, error) {
	return t.service.Store.EditPendingInput(ctx, id, inputID, text)
}

// CancelQueued removes a message that is not with the engine.
func (t *TurnService) CancelQueued(ctx context.Context, id, inputID string) error {
	return t.service.Store.CancelPendingInput(ctx, id, inputID)
}

// SendQueued starts a Turn from the queue head, or from the named queued
// message when inputID is not empty. It is the explicit resume of a queue that
// paused after a failure, a stop or a crash, and it refuses while the session
// still runs.
func (t *TurnService) SendQueued(ctx context.Context, id, inputID string) (TurnStart, error) {
	if _, active := t.service.Runtime.ActiveExecution(id); active {
		return TurnStart{}, hub.ErrSessionBusy
	}
	items, err := t.service.Store.PendingInputs(ctx, id)
	if err != nil {
		return TurnStart{}, err
	}
	for _, item := range items {
		if item.State != hub.PendingQueued && item.State != hub.PendingUncertain {
			continue
		}
		if inputID != "" && item.ID != inputID {
			continue
		}
		return t.dispatch(ctx, id, item)
	}
	return TurnStart{}, hub.ErrPendingGone
}

// dispatch admits one queued message as an ordinary Turn. The message leaves
// the queue in the admission transaction.
func (t *TurnService) dispatch(ctx context.Context, id string, item hub.PendingInput) (TurnStart, error) {
	// An uncertain message (it may have reached the model before a crash) goes
	// the same way as a queued one: the admission transaction deletes it, so a
	// failed admission keeps its row, its mark and its place.
	return t.service.admitWith(ctx, context.Background(), id, item.Text, nil, nil, nil, item.Text, "", nil, item.ID)
}

// Interject hands a message to the running Turn, executionID being the
// identity the caller observed. An engine that cannot take it (or a run that
// has no native conversation yet) gets the message queued instead, with a
// notice. The message is durable either way: with the engine it is
// "interjecting" until the delivery receipt deletes it, and the end of the
// Turn without one returns it to the queue head.
func (t *TurnService) Interject(ctx context.Context, id, executionID, text string) (InterjectResult, error) {
	if err := hub.ValidPendingText(text); err != nil {
		return InterjectResult{}, err
	}
	active, running := t.service.Runtime.ActiveExecution(id)
	if !running || executionID != "" && active != executionID {
		return InterjectResult{}, hub.ErrSessionNotRunning
	}
	identity, can := t.service.Runtime.CanInterject(id)
	if !can {
		item, err := t.Queue(ctx, id, text)
		if err != nil {
			return InterjectResult{}, err
		}
		notice := "当前引擎不支持运行中插话，已加入队列"
		if capabilities, capErr := t.Capabilities(ctx, id); capErr == nil && capabilities.Interject {
			notice = "这一轮还没有开始，已加入队列"
		}
		return InterjectResult{Mode: InterjectQueued, Input: item, Notice: notice}, nil
	}
	item, err := t.service.Store.QueueInput(ctx, id, text, hub.PendingInterjecting)
	if err != nil {
		return InterjectResult{}, err
	}
	err = t.service.Runtime.Interject(ctx, id, identity, engine.Interjection{ID: item.ID, Text: text})
	if err == nil {
		return InterjectResult{Mode: InterjectSent, Input: item}, nil
	}
	if requeueErr := t.service.Store.RequeuePendingInput(context.WithoutCancel(ctx), id, item.ID, "插话未能送达，已转为排队"); requeueErr != nil {
		return InterjectResult{}, errors.Join(err, requeueErr)
	}
	item.State = hub.PendingQueued
	return InterjectResult{Mode: InterjectQueued, Input: item, Notice: "插话未能送达，已加入队列"}, nil
}

// dispatchAfterExit is the Runtime's exit observer. The queue head starts
// only after a Turn that succeeded: a failure, a stop or a wait leaves the
// queue paused for the user to resume. The Turn must still be the
// conversation's newest, so a manual send that already replaced it wins.
func (s *HubService) dispatchAfterExit(exit hub.ExecutionExit) {
	if s.Turns == nil || exit.SessionID == "" {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	latest, err := s.Store.LatestTurn(ctx, exit.SessionID)
	if err != nil || latest.ID != exit.TurnID || latest.Status != engine.Succeeded {
		return
	}
	items, err := s.Store.PendingInputs(ctx, exit.SessionID)
	if err != nil || len(items) == 0 || items[0].State != hub.PendingQueued {
		return
	}
	if _, err = s.Turns.dispatch(ctx, exit.SessionID, items[0]); err != nil && !errors.Is(err, hub.ErrPendingGone) && !errors.Is(err, hub.ErrSessionBusy) {
		// The queue pauses on the head, which says why.
		_ = s.Store.NotePendingError(ctx, exit.SessionID, items[0].ID, "未能发送："+err.Error())
	}
}
