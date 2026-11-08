// INPUT: context, encoding/json, errors, fmt, engine Interjection/ForkPoint, the running Codex/Pi run owners
// OUTPUT: Adapter.Interject (Codex turn/steer, Pi steer), InputDelivered receipts from native delivery evidence, and the Codex thread/fork cut
// POS: [integration / native external-engine adapter] Conversation operations reuse each run's own protocol connection; a nil Interject is never a delivery receipt
package process

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/QuincySx/Alva/integration/engine"
)

var _ engine.Interjector = (*Adapter)(nil)

const (
	// maxPendingInterjections bounds queued, not yet delivered interjections of one run.
	maxPendingInterjections = 16
	// maxRunInterjections bounds the identities one run remembers to refuse repeats.
	maxRunInterjections = 64
)

// Interject queues a user message into the running execution that owns the
// native session. It returns ErrUnsupported for transports without a native
// steering channel and ErrSessionNotFound when that session has no execution
// that can still take one. A nil error only means the engine queued it; the
// run emits InputDelivered when the engine shows it entered the conversation.
func (a *Adapter) Interject(ctx context.Context, native string, input engine.Interjection) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := input.Validate(); err != nil {
		return err
	}
	switch a.config.Transport {
	case "app-server":
		return a.interjectCodex(ctx, native, input)
	case "rpc":
		return a.interjectPi(ctx, native, input)
	}
	return fmt.Errorf("%w: print/exec transport has no native steering channel", engine.ErrUnsupported)
}

// reserveInterjection records an identity for one run. Callers hold the run lock.
func reserveInterjection(ids map[string]bool, pending int, input engine.Interjection) error {
	if ids[input.ID] {
		return errors.New("interjection identity was already used in this execution")
	}
	if pending >= maxPendingInterjections || len(ids) >= maxRunInterjections {
		return errors.New("interjections for this execution exceed their bound")
	}
	ids[input.ID] = true
	return nil
}

func (a *Adapter) interjectCodex(ctx context.Context, native string, input engine.Interjection) error {
	run := a.codexRun(native)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	run.mu.Lock()
	if !run.steerOpen || run.turnID == "" {
		run.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	if run.steerIDs == nil {
		run.steerIDs, run.steers = make(map[string]bool), make(map[string]engine.Interjection)
	}
	if err := reserveInterjection(run.steerIDs, len(run.steers), input); err != nil {
		run.mu.Unlock()
		return err
	}
	// Registered before sending: the delivered userMessage item can be handled
	// by the run loop before this call observes the steer reply.
	run.steers[input.ID] = input
	turn := run.turnID
	run.mu.Unlock()
	// The product identity travels as clientUserMessageId; Codex records it as
	// the delivered userMessage item's clientId (rust-v0.166.0 turn_processor.rs
	// turn_steer -> core session record_user_prompt_and_emit_turn_item).
	params := map[string]any{"threadId": native, "expectedTurnId": turn, "clientUserMessageId": input.ID, "input": []any{map[string]any{"type": "text", "text": input.Text, "text_elements": []any{}}}}
	var steered struct {
		TurnID string `json:"turnId"`
	}
	// The run context owns the write (a torn frame fails the connection); the
	// caller's context only bounds waiting for Codex's acceptance.
	if err := run.rpc.callUntil(run.ctx, ctx, "turn/steer", params, &steered); err != nil {
		if ctx.Err() == nil {
			// Rejected or never written: no receipt can follow for this identity.
			run.mu.Lock()
			delete(run.steers, input.ID)
			delete(run.steerIDs, input.ID)
			run.mu.Unlock()
			return fmt.Errorf("Codex did not accept the interjection: %w", err)
		}
		// The steer may already be queued; a later receipt still reports it.
		return err
	}
	if steered.TurnID != turn {
		return errors.New("Codex steered a turn other than the running execution")
	}
	return nil
}

// codexDelivered reports a steered message once Codex recorded it as a
// userMessage item carrying the interjection identity as its clientId.
func (r *codexRun) codexDelivered(clientID string) error {
	if clientID == "" {
		return nil
	}
	r.mu.Lock()
	input, ok := r.steers[clientID]
	delete(r.steers, clientID)
	r.mu.Unlock()
	if !ok {
		return nil
	}
	// The receipt echoes the product's own text; it is not native output.
	return r.emit(engine.Event{Kind: engine.InputDelivered, SessionID: r.session, ID: input.ID, Text: input.Text})
}

// closeSteering stops accepting interjections; undelivered ones get no receipt.
func (r *codexRun) closeSteering() {
	r.mu.Lock()
	r.steerOpen = false
	r.steers = nil
	r.mu.Unlock()
}

// codexForkCut selects the fork boundary that drops the most recent
// DropTurns turns of the source thread. Turn ids come from the read-only
// thread/turns/list; the source thread is never resumed or modified. The
// stable lastTurnId keeps through an older turn; only when every turn is
// dropped does it use the experimental beforeTurnId on the oldest one.
func codexForkCut(ctx context.Context, rpc *codexRPC, fork *engine.ForkPoint, params map[string]any) error {
	if fork.DropTurns == 0 {
		return nil
	}
	var turns []string
	seen := make(map[string]bool)
	cursor := ""
	for len(turns) <= fork.DropTurns {
		list := map[string]any{"threadId": fork.Session, "limit": min(fork.DropTurns+1-len(turns), 100), "sortDirection": "desc", "itemsView": "notLoaded"}
		if cursor != "" {
			list["cursor"] = cursor
		}
		var page struct {
			Data []struct {
				ID string `json:"id"`
			} `json:"data"`
			NextCursor *string `json:"nextCursor"`
		}
		if err := rpc.call(ctx, "thread/turns/list", list, &page); err != nil {
			return err
		}
		for _, turn := range page.Data {
			if !nativeID.MatchString(turn.ID) || seen[turn.ID] {
				return errors.New("Codex listed an invalid or repeated source turn identity")
			}
			seen[turn.ID] = true
			turns = append(turns, turn.ID)
		}
		if len(page.Data) == 0 || page.NextCursor == nil || *page.NextCursor == "" {
			break
		}
		cursor = *page.NextCursor
	}
	switch {
	case len(turns) > fork.DropTurns:
		params["lastTurnId"] = turns[fork.DropTurns]
	case len(turns) == fork.DropTurns:
		params["beforeTurnId"] = turns[fork.DropTurns-1]
	default:
		return errors.New("Codex source thread has fewer turns than the fork drops")
	}
	return nil
}

func (a *Adapter) interjectPi(ctx context.Context, native string, input engine.Interjection) error {
	run := a.piRun(native)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	// One steer at a time per run keeps Pi's queue order identical to the
	// order matched against delivered user messages.
	run.steerMu.Lock()
	defer run.steerMu.Unlock()
	run.mu.Lock()
	if !run.steerOpen || run.stopping || run.context.Err() != nil {
		run.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	if run.steerIDs == nil {
		run.steerIDs = make(map[string]bool)
	}
	if err := reserveInterjection(run.steerIDs, len(run.steers), input); err != nil {
		run.mu.Unlock()
		return err
	}
	run.steers = append(run.steers, input)
	run.mu.Unlock()
	var queued struct {
		Disposition string `json:"disposition"`
	}
	err := run.connection.callUntil(run.context, ctx, "steer", map[string]any{"message": input.Text}, &queued, false)
	if err == nil && queued.Disposition != "queued" {
		err = errors.New("Pi did not queue the interjection for the running prompt")
	}
	if err != nil && ctx.Err() == nil {
		run.mu.Lock()
		for i, pending := range run.steers {
			if pending.ID == input.ID {
				run.steers = append(run.steers[:i:i], run.steers[i+1:]...)
				break
			}
		}
		delete(run.steerIDs, input.ID)
		run.mu.Unlock()
	}
	return err
}

// piUserMessageEnded matches a delivered role=user message. The first one
// after the accepted prompt is that prompt; later ones are steers, which Pi
// delivers in queue order, so the first pending steer with the same text is
// the delivered one and any older pending entries were never queued.
func (r *piRun) piUserMessageEnded(message map[string]json.RawMessage) error {
	text := piUserText(message["content"])
	r.mu.Lock()
	if !r.promptEchoed {
		r.promptEchoed = true
		r.mu.Unlock()
		return nil
	}
	var delivered *engine.Interjection
	for i, pending := range r.steers {
		if pending.Text == text {
			delivered = &pending
			r.steers = append([]engine.Interjection(nil), r.steers[i+1:]...)
			break
		}
	}
	r.mu.Unlock()
	if delivered == nil {
		return nil
	}
	// The receipt echoes the product's own text; it is not native output.
	return r.emit(engine.Event{Kind: engine.InputDelivered, SessionID: r.session, ID: delivered.ID, Text: delivered.Text})
}

// piUserText joins a user message's text blocks (or its plain string content).
func piUserText(raw json.RawMessage) string {
	var plain string
	if json.Unmarshal(raw, &plain) == nil {
		return plain
	}
	var blocks []map[string]json.RawMessage
	_ = json.Unmarshal(raw, &blocks)
	text := ""
	for _, block := range blocks {
		if str(block, "type") == "text" {
			text += str(block, "text")
		}
	}
	return text
}

// closeSteering stops accepting interjections; undelivered ones get no receipt.
func (r *piRun) closeSteering() {
	r.mu.Lock()
	r.steerOpen = false
	r.steers = nil
	r.mu.Unlock()
}

// validateFork rejects a fork request this transport cannot honour before any
// journal, binding or process side effect.
func (a *Adapter) validateFork(request engine.Request) error {
	if request.Fork == nil {
		return nil
	}
	if request.ResumeSession != "" {
		return errors.New("fork and resume are mutually exclusive")
	}
	if err := request.Fork.Validate(); err != nil {
		return err
	}
	if !nativeID.MatchString(request.Fork.Session) {
		return errors.New("fork source is not a safe opaque native identity")
	}
	if a.config.Transport != "app-server" {
		// Pi steered user messages are indistinguishable from prompts in its fork
		// list, and Pi writes forked session files outside the private-file policy.
		return fmt.Errorf("%w: this transport has no verified native fork", engine.ErrUnsupported)
	}
	return nil
}
