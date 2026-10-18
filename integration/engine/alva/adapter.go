// INPUT: context, crypto/rand, sync, harness.Runner, engine, kernel/abi, llm/wire
// OUTPUT: Adapter, explicit native-session RunnerFactory/FactoryOptions and independent Descriptor
// POS: [L7 Alva product adapter] Complete Alva harness bridge preserving qualified native facts and scoped cancellation
package alva

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Factory must load only the requested native session when resume=true; unknown
// identities must fail, never create a new session disguised as resume.
type RunnerFactory func(context.Context, string, engine.Request, bool) (harness.Runner, error)

// OwnedRunner is optional for factories that acquired native resources before
// Run. Close also runs if an initial durable observation fails, and must not
// implicitly execute or replay the requested task. Ordinary SDK Agents need
// not implement it.
type OwnedRunner interface {
	harness.Runner
	Close(context.Context) error
}
type PermissionResponder func(context.Context, string, string, engine.PermissionDecision) error

// FactoryOptions declares the exact request fields this trusted factory applies
// to its actual harness. Ordinary factories conservatively accept neither.
type FactoryOptions struct{ SystemPrompt, MaxTurns bool }
type Adapter struct {
	factory RunnerFactory
	respond PermissionResponder
	options FactoryOptions
	mu      sync.Mutex
	active  map[string]context.CancelFunc
}

func New(factory RunnerFactory) *Adapter {
	return &Adapter{factory: factory, active: make(map[string]context.CancelFunc)}
}

// NewWithPermissions advertises callback support only with a real scoped host.
func NewWithPermissions(factory RunnerFactory, respond PermissionResponder) *Adapter {
	adapter := New(factory)
	adapter.respond = respond
	return adapter
}
func NewConfigured(factory RunnerFactory, respond PermissionResponder, options FactoryOptions) *Adapter {
	a := NewWithPermissions(factory, respond)
	a.options = options
	return a
}
func (a *Adapter) Descriptor() engine.Descriptor {
	d := engine.DescribeRuntime(a)
	d.Config.SupportsSystemPrompt = a.options.SystemPrompt
	d.Config.SupportsMaxTurns = a.options.MaxTurns
	return d
}
func (a *Adapter) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true, PermissionCallback: a.respond != nil}
}
func (a *Adapter) RespondPermission(ctx context.Context, id, requestID string, decision engine.PermissionDecision) error {
	if a.respond == nil {
		return engine.ErrUnsupported
	}
	if requestID == "" || len(requestID) > 256 || len(decision.UpdatedInput) != 0 || len(decision.Reason) > 1024 {
		return errors.New("unsupported or unbounded permission decision")
	}
	a.mu.Lock()
	_, active := a.active[id]
	a.mu.Unlock()
	if !active {
		return engine.ErrSessionNotFound
	}
	return a.respond(ctx, id, requestID, decision)
}
func (a *Adapter) Cancel(_ context.Context, id string) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	cancel, ok := a.active[id]
	if !ok {
		return engine.ErrSessionNotFound
	}
	cancel()
	return nil
}
func (a *Adapter) Execute(parent context.Context, request engine.Request, sink func(engine.Event) error) error {
	if a.factory == nil || sink == nil {
		return errors.New("Alva engine requires a factory and event sink")
	}
	if request.MaxTurns < 0 || request.MaxTurns > 1024 || len(request.SystemPrompt) > 8<<10 || !utf8.ValidString(request.SystemPrompt) || strings.ContainsRune(request.SystemPrompt, '\x00') {
		return errors.New("engine request requires0..1024 turns and at most8KiB UTF-8 system prompt")
	}
	if (request.SystemPrompt != "" && !a.options.SystemPrompt) || (request.MaxTurns != 0 && !a.options.MaxTurns) {
		return fmt.Errorf("%w: configure Alva system/turn limits in the harness factory", engine.ErrUnsupported)
	}
	if strings.TrimSpace(request.Prompt) == "" || len(request.Prompt) > 64<<10 {
		return errors.New("prompt must be nonempty and at most 64 KiB")
	}
	id := request.ResumeSession
	if id == "" {
		var value [16]byte
		if _, err := rand.Read(value[:]); err != nil {
			return err
		}
		id = "native-" + hex.EncodeToString(value[:])
	}
	ctx, cancel := context.WithCancel(parent)
	a.mu.Lock()
	if _, busy := a.active[id]; busy {
		a.mu.Unlock()
		cancel()
		return errors.New("native session already running")
	}
	a.active[id] = cancel
	a.mu.Unlock()
	defer func() { cancel(); a.mu.Lock(); delete(a.active, id); a.mu.Unlock() }()
	contract := engine.StreamContract{}
	var observerErr error
	var observerMu sync.Mutex
	alive := true
	defer func() { observerMu.Lock(); alive = false; observerMu.Unlock() }()
	emit := func(event engine.Event) error {
		observerMu.Lock()
		defer observerMu.Unlock()
		if !alive {
			return errors.New("Alva activity owner has completed")
		}
		if observerErr != nil {
			return observerErr
		}
		next := contract.Clone()
		if err := next.Accept(event); err != nil {
			observerErr = err
			cancel()
			return err
		}
		if err := sink(event); err != nil {
			observerErr = err
			cancel()
			return err
		}
		contract = next
		return nil
	}
	request.ReportActivity = func(activity engine.Activity) error {
		return emit(engine.Event{Kind: engine.ExecutionActivity, SessionID: id, Activity: &activity})
	}
	runner, err := a.factory(ctx, id, request, request.ResumeSession != "")
	settleOwned := func(cause error) error {
		if owned, ok := runner.(OwnedRunner); ok {
			cleanup, stop := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
			defer stop()
			if closeErr := owned.Close(cleanup); closeErr != nil && !errors.Is(cause, closeErr) {
				return errors.Join(cause, &abi.SessionError{Operation: "close engine runner", SessionID: id, Err: closeErr})
			}
		}
		return cause
	}
	observerMu.Lock()
	observedFailure := observerErr
	observerMu.Unlock()
	if observedFailure != nil {
		return settleOwned(errors.Join(err, observedFailure))
	}
	if err == nil && runner == nil {
		err = errors.New("Alva harness factory returned no runner")
	}
	if err != nil {
		err = settleOwned(err)
		// Construction can already have admitted native activities. Retain its
		// actual classification and terminal edge without inventing a successful
		// SessionStarted or retrying an uncommitted observation.
		outcome, failure := executionFailure(ctx, err, false)
		if sendErr := emit(engine.Event{Kind: engine.Error, Text: bounded(err.Error(), 4096), Failure: failure}); sendErr != nil {
			return errors.Join(err, sendErr)
		}
		return errors.Join(err, emit(engine.Event{Kind: engine.Completed, SessionID: id, Outcome: outcome}))
	}
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: id}); err != nil {
		return settleOwned(err)
	}
	open := make(map[string]string)
	var text strings.Builder
	var usage engine.Usage
	var callbackErr error
	blocks := 0
	responseStarted := false
	responseIndex := 0
	err = runner.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, request.Prompt)}, func(event abi.Event) error {
		if callbackErr != nil {
			return callbackErr
		}
		var output *engine.Event
		var reference *engine.Reference
		if event.Record != nil {
			if event.Record.SessionID != id {
				callbackErr = errors.New("native event reference did not belong to the requested session")
				return callbackErr
			}
			reference = &engine.Reference{SessionID: id, RecordID: event.Record.RecordID, Sequence: event.Record.Sequence}
		}
		switch event.Kind {
		case abi.EventTextDelta:
			responseStarted = true
			if text.Len()+len(event.Text) > 1<<20 {
				callbackErr = errors.New("assistant display exceeds 1 MiB; execution stopped")
				return callbackErr
			}
			text.WriteString(event.Text)
			output = &engine.Event{Kind: engine.MessageDelta, ID: fmt.Sprintf("assistant-%d", responseIndex), Text: event.Text}
		case abi.EventMessageCommitted:
			message := event.Message
			if message == nil || message.Role != wire.RoleAssistant || reference == nil || message.ID != reference.RecordID {
				return errors.New("complete assistant event lacks its actual committed message/reference")
			}
			responseStarted = true
			id := fmt.Sprintf("assistant-%d", responseIndex)
			responseIndex++
			hasCalls := false
			for _, block := range message.Content {
				hasCalls = hasCalls || block.Kind == wire.ContentToolCall
			}
			if !hasCalls && message.Text() != "" {
				// This replaces only this response's stream draft. It is the
				// actual effective committed answer, not joined deltas, reasoning
				// or a tool-call preface. Whole-run success still waits for cleanup.
				output = &engine.Event{Kind: engine.Message, ID: id, Text: bounded(message.Text(), 60<<10), Reference: reference}
			}
		case abi.EventReasoningDelta:
			responseStarted = true
			output = &engine.Event{Kind: engine.ReasoningDelta, ID: fmt.Sprintf("reasoning-%d", responseIndex), Text: event.Text}
		case abi.EventContentBlock:
			responseStarted = true
			if event.ContentBlock == nil {
				return errors.New("content block event missing payload")
			}
			block := event.ContentBlock
			blocks++
			id := fmt.Sprintf("block-%d", blocks)
			visible := "Provider content retained in the native conversation; this overview does not interpret or automatically load media/opaque provider state."
			if block.Kind == wire.ContentReasoning && block.Reasoning != nil {
				id = fmt.Sprintf("reasoning-%d", responseIndex)
				visible = block.Reasoning.Text
				if visible == "" {
					visible = "Redacted reasoning retained opaquely; no decoding or assistant-answer text implied."
				}
			}
			if block.Kind == wire.ContentRefusal && block.Refusal != nil {
				visible = block.Refusal.Text
			}
			if block.Kind == wire.ContentText {
				visible = block.Text
			}
			output = &engine.Event{Kind: engine.ContentBlock, ID: id, Name: string(block.Kind), Text: bounded(visible, 16<<10)}
		case abi.EventToolStarted:
			if event.ToolCall == nil {
				return errors.New("tool start missing payload")
			}
			call := event.ToolCall
			open[call.ID] = call.Name
			output = &engine.Event{Kind: engine.ToolStart, ID: call.ID, Name: call.Name, Reference: reference, Effect: engine.EffectUnknown}
		case abi.EventToolCompleted:
			if event.ToolResult == nil {
				return errors.New("tool end missing payload")
			}
			result := event.ToolResult
			name := open[result.CallID]
			delete(open, result.CallID)
			output = &engine.Event{Kind: engine.ToolEnd, ID: result.CallID, Name: name, Reference: reference, IsError: result.IsError, Text: "Native tool result retained in session records; open its qualified reference to inspect."}
		case abi.EventUsage:
			if event.Usage != nil {
				responseStarted = true
				usage.InputTokens += event.Usage.InputTokens
				usage.OutputTokens += event.Usage.OutputTokens
			}
		}
		if output != nil {
			callbackErr = emit(*output)
			return callbackErr
		}
		return nil
	})
	err = settleOwned(errors.Join(err, callbackErr))
	// Tool cleanup precedes fatal Error, preserving the final-only contract.
	for callID, name := range open {
		if sendErr := emit(engine.Event{Kind: engine.ToolEnd, ID: callID, Name: name, IsError: true, Text: "Execution ended before tool settlement was observed; inspect native facts."}); sendErr != nil {
			return errors.Join(err, sendErr)
		}
	}
	outcome := engine.Succeeded
	if err != nil {
		var failure engine.FailureKind
		outcome, failure = executionFailure(ctx, err, responseStarted)
		if sendErr := emit(engine.Event{Kind: engine.Error, Text: bounded(err.Error(), 4096), Failure: failure}); sendErr != nil {
			return errors.Join(err, sendErr)
		}
	}
	return emit(engine.Event{Kind: engine.Completed, SessionID: id, Outcome: outcome, Usage: &usage})
}
func bounded(value string, limit int) string {
	if len(value) <= limit {
		return value
	}
	value = value[:limit]
	for !utf8.ValidString(value) && len(value) > 0 {
		value = value[:len(value)-1]
	}
	return value + " [bounded]"
}
