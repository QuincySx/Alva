// INPUT: Claude Code stream-json stdout control frames (control_request can_use_tool / control_cancel_request) and the product's PermissionDecision
// OUTPUT: PermissionRequest/InteractionResolved events, one-shot control_response frames on the child's stdin, per-run pending registry
// POS: [integration / native external-engine adapter] Claude --permission-prompt-tool stdio channel: the host answers only the "ask" outcome, never persists a grant and never touches the user's own settings.
package process

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"regexp"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
)

const (
	claudeMaxPending   = 16
	claudeMaxRequests  = 256
	claudeMaxIdentity  = 256
	claudeMaxReasonLen = 512
	claudeMaxInputSize = 64 << 10
)

// errTransportUnsupported marks a failure inside the handshake of a transport
// that carries approvals: the installed engine is too old, or is not that
// engine. It is never answered by silently falling back to a transport that
// would not ask.
var errMethodNotFound = errors.New("engine reported an unknown method or command")

// claudeUnknownOption matches commander's "error: unknown option '--flag'" (verified
// against claude 2.1.319) for the flags this transport needs.
var claudeUnknownOption = regexp.MustCompile(`unknown option '--(permission-prompt-tool|input-format|permission-mode)'`)

var errTransportUnsupported = errors.New("engine does not speak the approval transport")

func unsupportedTransportText(transport, explicit string) string {
	return "引擎版本过旧或不支持带审批的 " + transport + " 协议，未能启动。请升级该引擎；也可显式使用 " + explicit + "，但那个模式不会询问。Alva 不会自动降级。"
}

// claudeStream reports whether the adapter drives Claude through the
// bidirectional stream-json channel that carries permission prompts.
func (a *Adapter) claudeStream() bool {
	return a.config.Vendor == "claude" && a.config.Transport == "stream-json"
}

type claudePending struct {
	requestID string
	toolUseID string
	toolName  string
	input     json.RawMessage
}

// claudeRun owns one stream-json child's stdin and its unanswered prompts.
// emitMu serialises the run's event stream: a decision's write and its
// resolved event happen under it, so no later native frame is published first.
type claudeRun struct {
	ctx     context.Context
	runID   string
	stdin   io.WriteCloser
	secrets []string
	// emit is the contract-checked sink; emitMu guards it and the stdin writes.
	emitMu  *sync.Mutex
	emit    func(engine.Event) error
	journal func(string)
	fail    func()

	mu       sync.Mutex
	pending  map[string]*claudePending
	seen     map[string]bool
	closed   bool
	inputEnd bool
}

func (r *claudeRun) writeFrame(frame any) error {
	data, err := json.Marshal(frame)
	if err != nil {
		return err
	}
	_, err = r.stdin.Write(append(data, '\n'))
	return err
}

// sendPrompt writes the single user message. stdin stays open afterwards: the
// child needs it to receive permission decisions until its native result.
func (r *claudeRun) sendPrompt(text string) error {
	r.emitMu.Lock()
	defer r.emitMu.Unlock()
	return r.writeFrame(map[string]any{"type": "user", "message": map[string]any{"role": "user", "content": text}})
}

// closeInput ends the child's input once its native result arrived.
func (r *claudeRun) closeInput() {
	r.mu.Lock()
	if r.inputEnd {
		r.mu.Unlock()
		return
	}
	r.inputEnd = true
	r.mu.Unlock()
	r.emitMu.Lock()
	_ = r.stdin.Close()
	r.emitMu.Unlock()
}

func claudeEffect(tool string) engine.Effect {
	switch tool {
	case "Bash", "BashOutput", "KillShell", "PowerShell":
		return engine.EffectExecute
	case "Write", "Edit", "MultiEdit", "NotebookEdit":
		return engine.EffectWrite
	case "WebFetch", "WebSearch":
		return engine.EffectNetwork
	case "Read", "Grep", "Glob", "LS", "NotebookRead":
		return engine.EffectRead
	}
	return engine.EffectUnknown
}

// handleLine consumes Claude control frames. handled is true when the line
// must not reach the event mapper.
func (r *claudeRun) handleLine(line string) (handled bool, err error) {
	frame := decode([]byte(line))
	switch str(frame, "type") {
	case "control_request":
		return true, r.request(frame)
	case "control_cancel_request":
		return true, r.cancelled(str(frame, "request_id"))
	case "control_response":
		// The host never sends a control request, so nothing can answer one.
		return true, nil
	}
	return false, nil
}

func validClaudeIdentity(value string) bool {
	return value != "" && len(value) <= claudeMaxIdentity && utf8.ValidString(value) && !strings.ContainsAny(value, "\x00\r\n")
}

func (r *claudeRun) request(frame map[string]json.RawMessage) error {
	requestID := str(frame, "request_id")
	body := decode(frame["request"])
	if !validClaudeIdentity(requestID) {
		return errors.New("Claude control request identity is invalid")
	}
	if str(body, "subtype") != "can_use_tool" {
		// Hooks, MCP bridges and other host callbacks are not supplied.
		r.emitMu.Lock()
		defer r.emitMu.Unlock()
		return r.writeFrame(map[string]any{"type": "control_response", "response": map[string]any{"subtype": "error", "request_id": requestID, "error": "unsupported control request"}})
	}
	tool, toolUse := str(body, "tool_name"), str(body, "tool_use_id")
	if !validClaudeIdentity(tool) || !validClaudeIdentity(toolUse) || len(body["input"]) == 0 || len(body["input"]) > 1<<20 {
		return errors.New("Claude permission request is malformed")
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(body["input"], &object) != nil || object == nil {
		return errors.New("Claude permission request input is not an object")
	}
	if len(body["input"]) > claudeMaxInputSize {
		// Too large to echo back as updatedInput: refuse instead of asking.
		r.emitMu.Lock()
		defer r.emitMu.Unlock()
		return r.writeFrame(claudeDecisionFrame(requestID, engine.PermissionDecision{Reason: "The tool input is too large for this host to approve."}, nil))
	}
	if tool == "AskUserQuestion" {
		// Human questions have no card on this transport: refuse explicitly so
		// the model carries on, exactly as the previous non-asking mode did.
		r.emitMu.Lock()
		defer r.emitMu.Unlock()
		return r.writeFrame(claudeDecisionFrame(requestID, engine.PermissionDecision{Reason: "This host cannot ask the user questions."}, body["input"]))
	}
	input := append(json.RawMessage(nil), body["input"]...)
	r.mu.Lock()
	if r.closed || r.ctx.Err() != nil {
		r.mu.Unlock()
		return context.Canceled
	}
	if r.seen[requestID] || len(r.seen) >= claudeMaxRequests || len(r.pending) >= claudeMaxPending {
		r.mu.Unlock()
		return errors.New("Claude permission request identity repeated or exceeded its bound")
	}
	r.seen[requestID] = true
	r.pending[claudeInteractionID(r.runID, requestID)] = &claudePending{requestID: requestID, toolUseID: toolUse, toolName: tool, input: input}
	r.mu.Unlock()
	event := engine.Event{
		Kind: engine.PermissionRequest, ID: claudeInteractionID(r.runID, requestID), Name: tool,
		Input: redactToolInput(input, r.secrets), Effect: claudeEffect(tool),
		Display: finishDisplay(claudeDisplay(tool, input), r.secrets),
		Text:    "Approve this exact Claude native tool call before it runs. Alva permission profiles do not auto-approve it.",
	}
	r.emitMu.Lock()
	defer r.emitMu.Unlock()
	return r.emit(event)
}

func claudeInteractionID(runID, requestID string) string {
	return "claude:" + strings.TrimPrefix(runID, "engine-") + ":" + requestID
}

// claudeDecisionFrame is the one-shot answer for exactly one tool call. It
// carries no updatedPermissions, so nothing becomes a persistent grant.
func claudeDecisionFrame(requestID string, decision engine.PermissionDecision, original json.RawMessage) map[string]any {
	response := map[string]any{"behavior": "deny", "message": "The user denied this tool call."}
	if decision.Allow {
		input := original
		if len(decision.UpdatedInput) != 0 {
			input = decision.UpdatedInput
		}
		response = map[string]any{"behavior": "allow", "updatedInput": json.RawMessage(input)}
	} else if decision.Reason != "" {
		response["message"] = decision.Reason
	}
	return map[string]any{"type": "control_response", "response": map[string]any{"subtype": "success", "request_id": requestID, "response": response}}
}

func (r *claudeRun) cancelled(requestID string) error {
	id := claudeInteractionID(r.runID, requestID)
	r.mu.Lock()
	_, ok := r.pending[id]
	delete(r.pending, id)
	r.mu.Unlock()
	if !ok {
		return nil
	}
	r.emitMu.Lock()
	defer r.emitMu.Unlock()
	return r.emit(engine.Event{Kind: engine.InteractionResolved, ID: id, Name: "permission"})
}

func (r *claudeRun) respond(ctx context.Context, id string, decision engine.PermissionDecision) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if len(decision.UpdatedInput) != 0 {
		var object map[string]json.RawMessage
		if len(decision.UpdatedInput) > claudeMaxInputSize || json.Unmarshal(decision.UpdatedInput, &object) != nil || object == nil {
			return errors.New("updated Claude tool input must be a bounded JSON object")
		}
	}
	if len(decision.Reason) > claudeMaxReasonLen || !utf8.ValidString(decision.Reason) || strings.ContainsRune(decision.Reason, 0) {
		decision.Reason = ""
	}
	r.emitMu.Lock()
	defer r.emitMu.Unlock()
	r.mu.Lock()
	pending, ok := r.pending[id]
	if !ok || r.closed {
		r.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	if err := r.ctx.Err(); err != nil {
		r.mu.Unlock()
		return err
	}
	// Consumed before the write: a decision can be delivered at most once.
	delete(r.pending, id)
	r.mu.Unlock()
	if err := r.writeFrame(claudeDecisionFrame(pending.requestID, decision, pending.input)); err != nil {
		r.fail()
		return fmt.Errorf("Claude permission decision could not be delivered: %w", err)
	}
	r.journal("[control_response " + map[bool]string{true: "allow", false: "deny"}[decision.Allow] + " for " + pending.requestID + "]")
	allowed := decision.Allow
	resolution := publishedResolution(&engine.InteractionResolution{Allowed: &allowed}, "permission", r.secrets, false)
	return r.emit(engine.Event{Kind: engine.InteractionResolved, ID: id, Name: "permission", Resolution: resolution})
}

// abandon closes the run to further decisions and settles every unanswered
// prompt as denied: the child ended, so none of those tool calls ran.
func (r *claudeRun) abandon() error {
	r.mu.Lock()
	r.closed = true
	left := make([]string, 0, len(r.pending))
	for id := range r.pending {
		left = append(left, id)
	}
	r.pending = make(map[string]*claudePending)
	r.mu.Unlock()
	r.emitMu.Lock()
	defer r.emitMu.Unlock()
	for _, id := range left {
		denied := false
		if err := r.emit(engine.Event{Kind: engine.InteractionResolved, ID: id, Name: "permission", Resolution: &engine.InteractionResolution{Allowed: &denied}}); err != nil {
			return err
		}
	}
	return nil
}

func (a *Adapter) claudeRun(session string) *claudeRun {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.claudeRuns[session]
}

func (a *Adapter) respondClaudePermission(ctx context.Context, session, id string, decision engine.PermissionDecision) error {
	run := a.claudeRun(session)
	if run == nil {
		return engine.ErrSessionNotFound
	}
	return run.respond(ctx, id, decision)
}
