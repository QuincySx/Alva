// INPUT: Pi RPC bridge dialogs, typed native events and bound Hub responses
// OUTPUT: Pre-execution permissions (with the call's display), free-text questions, exact pending-response correlation and relayed resolutions without answer bodies
// POS: [integration / native external-engine adapter] A native dialog authorizes one exact tool call and is separate from tool execution facts
package process

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"hash"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
)

type piPending struct {
	kind, toolID, nativeID string
	question               engine.UserInputRequest
}

// piResolution is one settled dialog and what this adapter relayed for it.
type piResolution struct {
	id, kind   string
	resolution *engine.InteractionResolution
}
type piAdmission struct{ resolved, allowed bool }
type piRun struct {
	connection     *piConnection
	context        context.Context
	cancel         context.CancelFunc
	done           chan struct{}
	cleanupErr     error
	session, nonce string
	mapper         *mapper
	emit           func(engine.Event) error
	mu             sync.Mutex
	pending        map[string]piPending
	admissions     map[string]piAdmission
	seen           map[string]bool
	resolutions    chan piResolution
	stopping       bool
	publishedID    string
	publishedBytes int
	publishedHash  hash.Hash
	nativeStream
}

func (r *piRun) reference() *engine.Reference { return r.mapper.reference() }
func (r *piRun) event(event engine.Event) error {
	if r.session != "" {
		event.SessionID = r.session
	}
	j := r.connection.journal
	j.mu.Lock()
	private := j.privateOutput
	secrets := append([]string(nil), j.secrets...)
	j.mu.Unlock()
	if event.Kind == engine.MessageDelta || event.Kind == engine.ReasoningDelta {
		if private {
			r.clearStreams()
			return nil
		}
		if len(secrets) != 0 {
			text, err := r.filterDelta(event, secrets)
			if err != nil {
				return err
			}
			if text == "" {
				return nil
			}
			event.Text = text
		}
	} else {
		text, err := redactCodexText(event.Text, secrets)
		if err != nil {
			return err
		}
		event.Text = text
		if len(event.Input) != 0 {
			event.Input, err = redaction.RedactJSON(event.Input, secrets...)
			if err != nil {
				return err
			}
		}
		event.Display = finishDisplay(event.Display, secrets)
		if event.Question != nil {
			copy := *event.Question
			copy.Questions = append([]engine.UserInputQuestion(nil), copy.Questions...)
			for i := range copy.Questions {
				copy.Questions[i].Question, err = redactCodexText(copy.Questions[i].Question, secrets)
				if err != nil {
					return err
				}
			}
			event.Question = &copy
		}
		// Pi questions are free text only, so a resolution never carries an
		// answer body, whatever shape the answer had.
		event.Resolution = publishedResolution(event.Resolution, event.Name, secrets, false)
	}
	if event.Kind == engine.Message {
		defer r.clearPublishedMessage()
		if len(event.Text) > 64<<10 {
			// These chunks come from one fully redacted native aggregate. They
			// are honest deltas, including after a private human reply; raw
			// native deltas above remain suppressed under that policy.
			prefix := 0
			if r.publishedID == event.ID {
				prefix = r.publishedBytes
				if prefix > len(event.Text) || r.publishedHash == nil {
					return errors.New("Pi complete message lost its published prefix")
				}
				digest := sha256.Sum256([]byte(event.Text[:prefix]))
				if string(digest[:]) != string(r.publishedHash.Sum(nil)) {
					return errors.New("Pi completed message changed already published text")
				}
			}
			for _, chunk := range chunkText(engine.MessageDelta, event.ID, event.Text[prefix:]) {
				chunk.SessionID = event.SessionID
				if err := r.emit(chunk); err != nil {
					return err
				}
			}
			return nil
		}
	}
	if len(event.Text) > 64<<10 || len(event.Input) > 16<<10 {
		event.Text, event.Input = "", nil
		event.Reference = r.reference()
	}
	if err := r.emit(event); err != nil {
		return err
	}
	if event.Kind == engine.MessageDelta {
		if r.publishedID != event.ID {
			r.clearPublishedMessage()
			r.publishedID = event.ID
			r.publishedHash = sha256.New()
		}
		_, _ = r.publishedHash.Write([]byte(event.Text))
		r.publishedBytes += len(event.Text)
	}
	return nil
}
func (r *piRun) clearPublishedMessage() {
	r.publishedID = ""
	r.publishedBytes = 0
	r.publishedHash = nil
}
func (r *piRun) dialog(data []byte) error {
	var frame struct{ ID, Method, Title, Message string }
	if json.Unmarshal(data, &frame) != nil || !nativeID.MatchString(frame.ID) {
		return errors.New("Pi RPC dialog has invalid native identity")
	}
	var bridge struct {
		AlvaBridge                                  int
		Nonce, Kind, ToolCallID, ToolName, Question string
		Input                                       json.RawMessage
	}
	body := frame.Message
	if frame.Method == "input" {
		body = frame.Title
	}
	if json.Unmarshal([]byte(body), &bridge) != nil || bridge.AlvaBridge != 1 || bridge.Nonce != r.nonce || bridge.ToolCallID == "" || len(bridge.ToolCallID) > 256 || !utf8.ValidString(bridge.ToolCallID) || strings.ContainsRune(bridge.ToolCallID, 0) {
		return errors.New("Pi RPC dialog did not originate from the owned bridge")
	}
	pending := piPending{toolID: bridge.ToolCallID, nativeID: frame.ID}
	id := "pi:" + strings.TrimPrefix(r.mapper.journal, "engine-journal://") + ":" + frame.ID
	event := engine.Event{ID: id, Reference: r.reference()}
	switch {
	case frame.Method == "confirm" && bridge.Kind == "permission":
		if bridge.ToolName != "bash" && bridge.ToolName != "edit" && bridge.ToolName != "write" {
			return errors.New("Pi bridge requested an unsupported tool permission")
		}
		if r.mapper.tools[bridge.ToolCallID] != bridge.ToolName {
			return errors.New("Pi approval does not match an active native tool call")
		}
		pending.kind = "permission"
		event.Kind, event.Name, event.Input = engine.PermissionRequest, bridge.ToolName, bridge.Input
		event.Effect = engine.EffectWrite
		if bridge.ToolName == "bash" {
			event.Effect = engine.EffectExecute
		}
		event.Text = "Approve this exact Pi native tool call before execution. Alva permission profiles do not auto-approve it."
		event.Display = piDisplay(bridge.ToolName, bridge.Input)
	case frame.Method == "input" && bridge.Kind == "question":
		if r.mapper.tools[bridge.ToolCallID] != "ask_user" {
			return errors.New("Pi question does not match the active ask_user tool")
		}
		pending.kind = "user_input"
		pending.question = engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "answer", Header: "Pi", Question: bridge.Question, IsOther: true}}}
		if err := pending.question.Validate(); err != nil {
			return err
		}
		event.Kind, event.Name, event.Question = engine.UserInputRequested, "ask_user", &pending.question
	default:
		return errors.New("Pi bridge emitted an unsupported dialog")
	}
	r.mu.Lock()
	if r.stopping || r.context.Err() != nil {
		r.mu.Unlock()
		return context.Canceled
	}
	if r.seen[frame.ID] || len(r.seen) >= 256 || len(r.pending) >= 16 {
		r.mu.Unlock()
		return errors.New("Pi native dialog identity repeated or exceeded its bound")
	}
	if pending.kind == "permission" {
		if _, exists := r.admissions[pending.toolID]; exists {
			r.mu.Unlock()
			return errors.New("Pi requested the same native tool permission twice")
		}
		r.admissions[pending.toolID] = piAdmission{}
	}
	r.seen[frame.ID] = true
	r.pending[id] = pending
	r.mu.Unlock()
	return r.event(event)
}
func (r *piRun) resolve(event piResolution) error {
	return r.event(engine.Event{Kind: engine.InteractionResolved, ID: event.id, Name: event.kind, Resolution: event.resolution})
}
func (r *piRun) handle(data []byte) error {
	frame := decode(data)
	message := decode(frame["message"])
	if str(message, "role") == "assistant" {
		switch str(frame, "type") {
		case "message_start":
			r.clearPublishedMessage()
			r.clearStreams()
		case "message_end":
			defer r.clearPublishedMessage()
			defer r.clearStreams()
		}
	}
	switch str(frame, "type") {
	case "extension_ui_request":
		return r.dialog(data)
	case "extension_error":
		return errors.New("Pi extension failed; native execution stopped")
	case "session":
		return errors.New("Pi RPC unexpectedly changed session identity")
	}
	r.mapper.piWholeMessage = true
	events, err := r.mapper.mapLine(data)
	r.mapper.piWholeMessage = false
	if err != nil {
		return err
	}
	for _, event := range events {
		if event.Kind == engine.ToolStart {
			switch event.Name {
			case "read", "grep", "find", "ls":
				event.Effect = engine.EffectRead
			case "write", "edit":
				event.Effect = engine.EffectWrite
			case "bash":
				event.Effect = engine.EffectExecute
			case "ask_user":
				event.Effect = engine.EffectNone
			default:
				return errors.New("Pi started a tool outside the owned explicit tool set")
			}
		}
		if event.Kind == engine.Message {
			for key, tail := range r.streamTails {
				if key.id == event.ID {
					delete(r.streamTails, key)
					r.streamBytes -= len(tail)
				}
			}
		}
		if event.Kind == engine.ToolEnd {
			r.mu.Lock()
			for _, pending := range r.pending {
				if pending.toolID == event.ID {
					r.mu.Unlock()
					return errors.New("Pi tool completed before its pending native dialog was answered")
				}
			}
			if event.Name == "bash" || event.Name == "edit" || event.Name == "write" {
				admission, exists := r.admissions[event.ID]
				if !event.IsError && (!exists || !admission.resolved || !admission.allowed) {
					r.mu.Unlock()
					return errors.New("Pi reported successful mutable tool execution without its exact approval")
				}
				delete(r.admissions, event.ID)
			}
			r.mu.Unlock()
		}
		if err := r.event(event); err != nil {
			return err
		}
	}
	return nil
}
func (a *Adapter) piRun(session string) *piRun {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.piRuns[session]
}
func (a *Adapter) respondPiPermission(ctx context.Context, session, id string, decision engine.PermissionDecision) error {
	if len(decision.UpdatedInput) != 0 {
		return fmt.Errorf("%w: Pi native tool input cannot be rewritten", engine.ErrUnsupported)
	}
	run := a.piRun(session)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	return run.respond(ctx, id, "permission", map[string]any{"type": "extension_ui_response", "confirmed": decision.Allow}, nil)
}
func (a *Adapter) respondPiInput(ctx context.Context, session, id string, response engine.UserInputResponse) error {
	run := a.piRun(session)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	run.mu.Lock()
	pending, ok := run.pending[id]
	run.mu.Unlock()
	if !ok || pending.kind != "user_input" {
		return engine.ErrSessionNotFound
	}
	if err := response.ValidateFor(pending.question); err != nil {
		return err
	}
	answers := response.Answers["answer"]
	if len(answers) > 1 {
		return errors.New("Pi asks for one free-text answer")
	}
	value := map[string]any{"type": "extension_ui_response", "cancelled": true}
	if len(answers) == 1 {
		value = map[string]any{"type": "extension_ui_response", "value": answers[0]}
	}
	return run.respond(ctx, id, "user_input", value, answers)
}
func (r *piRun) respond(parent context.Context, id, kind string, value map[string]any, answers []string) (err error) {
	if err := parent.Err(); err != nil {
		return err
	}
	r.mu.Lock()
	pending, ok := r.pending[id]
	if !ok || pending.kind != kind || r.stopping {
		r.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	if err := r.context.Err(); err != nil {
		r.mu.Unlock()
		return err
	}
	delete(r.pending, id)
	if kind == "permission" {
		allowed, _ := value["confirmed"].(bool)
		r.admissions[pending.toolID] = piAdmission{resolved: true, allowed: allowed}
	}
	r.mu.Unlock()
	value["id"] = pending.nativeID
	defer func() {
		if err != nil {
			r.connection.fail(err)
			r.connection.proc.BeginClose()
		}
	}()
	// Free-text replies may contain secrets without an explicit classification.
	// Register every answer before native input; suppress later incremental echoes.
	if err = r.connection.journal.registerPrivateAnswers(answers); err != nil {
		return err
	}
	ctx, cancel := context.WithCancel(parent)
	defer cancel()
	detach := context.AfterFunc(r.context, cancel)
	defer detach()
	if err = r.connection.send(ctx, value, kind == "user_input"); err != nil {
		return err
	}
	// A free-text answer is recorded as answered, never with its body.
	resolution := &engine.InteractionResolution{}
	if kind == "permission" {
		allowed, _ := value["confirmed"].(bool)
		resolution.Allowed = &allowed
	} else if len(answers) != 0 {
		resolution.Answers = map[string][]string{"answer": {}}
	}
	select {
	case r.resolutions <- piResolution{id, kind, resolution}:
		return nil
	case <-r.context.Done():
		return r.context.Err()
	default:
		return errors.New("Pi resolved-dialog backlog exceeded its bound")
	}
}

func (r *piRun) ready(data []byte) bool {
	frame := decode(data)
	if str(frame, "type") != "extension_ui_request" || str(frame, "method") != "notify" {
		return false
	}
	var body struct {
		AlvaBridge  int
		Nonce, Kind string
	}
	return json.Unmarshal([]byte(str(frame, "message")), &body) == nil && body.AlvaBridge == 1 && body.Nonce == r.nonce && body.Kind == "ready"
}
func (r *piRun) stopNative() {
	r.mu.Lock()
	r.stopping = true
	ids := make([]string, 0, len(r.pending))
	for _, pending := range r.pending {
		ids = append(ids, pending.nativeID)
	}
	r.pending = make(map[string]piPending)
	r.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 500*time.Millisecond)
	defer cancel()
	for _, id := range ids {
		_ = r.connection.send(ctx, map[string]any{"type": "extension_ui_response", "id": id, "cancelled": true}, true)
	}
	_ = r.connection.call(ctx, "abort", nil, nil, false)
	r.connection.proc.BeginClose()
}
