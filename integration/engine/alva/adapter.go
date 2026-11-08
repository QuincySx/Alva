// INPUT: context, crypto/rand, encoding/json, sync, harness.Runner, engine, shared record redaction, kernel/abi, llm/wire
// OUTPUT: Adapter, explicit native-session RunnerFactory/FactoryOptions and independent Descriptor
// POS: [L7 Alva product adapter] Complete Alva harness bridge preserving qualified native facts, bounded tool evidence/display, cache-aware usage, context reports and scoped cancellation
package alva

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
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
// to its actual harness. Ordinary factories conservatively reject optional settings.
type FactoryOptions struct {
	SystemPrompt, MaxTurns, Generation, DurableWaits, FileInputs, SkillInputs bool
	// ContextCompaction: the factory installs harness compaction, honours
	// Request.CompactContext and wires Request.ReportContext.
	ContextCompaction bool
}
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
	d.Config.SupportsGeneration = a.options.Generation
	return d
}
func (a *Adapter) Capabilities() engine.Capabilities {
	return engine.Capabilities{DurableWaits: a.options.DurableWaits, Streaming: true, Resume: true, Cancel: true, ToolControl: true, PermissionCallback: a.respond != nil, FileInputs: a.options.FileInputs, SkillInputs: a.options.SkillInputs, ContextCompaction: a.options.ContextCompaction}
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
	request.Generation = request.Generation.Clone()
	if err := engine.ValidateInputFiles(request.InputFiles); err != nil {
		return err
	}
	if len(request.InputFiles) > 0 && !a.options.FileInputs {
		return engine.ErrUnsupported
	}
	request.InputFiles = engine.CloneInputFiles(request.InputFiles)
	if err := engine.ValidateInputSkills(request.InputSkills); err != nil {
		return err
	}
	if len(request.InputSkills) > 0 && !a.options.SkillInputs {
		return engine.ErrUnsupported
	}
	request.InputSkills = engine.CloneInputSkills(request.InputSkills)
	if err := request.Generation.Validate(); err != nil {
		return err
	}
	if !request.Generation.Empty() && !a.options.Generation {
		return fmt.Errorf("%w: configure Alva generation options in the harness factory", engine.ErrUnsupported)
	}
	if request.MaxTurns < 0 || request.MaxTurns > 1024 || len(request.SystemPrompt) > 8<<10 || !utf8.ValidString(request.SystemPrompt) || strings.ContainsRune(request.SystemPrompt, '\x00') {
		return errors.New("engine request requires0..1024 turns and at most8KiB UTF-8 system prompt")
	}
	if (request.SystemPrompt != "" && !a.options.SystemPrompt) || (request.MaxTurns != 0 && !a.options.MaxTurns) {
		return fmt.Errorf("%w: configure Alva system/turn limits in the harness factory", engine.ErrUnsupported)
	}
	if request.CompactContext && !a.options.ContextCompaction {
		return fmt.Errorf("%w: configure Alva context compaction in the harness factory", engine.ErrUnsupported)
	}
	if request.WaitRevision != 0 && (!a.options.DurableWaits || request.ResumeSession == "" || request.Prompt != "") {
		return errors.New("durable resume requires a supported existing native session and no prompt")
	}
	if request.WaitRevision == 0 && (strings.TrimSpace(request.Prompt) == "" || len(request.Prompt) > 64<<10) {
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
	// A summary request's usage belongs to this attempt's completion Usage.
	var compactionUsage []engine.Usage
	request.ReportContext = func(report engine.ContextReport) error {
		if compaction := report.Compaction; compaction != nil && compaction.Usage != nil {
			observerMu.Lock()
			compactionUsage = append(compactionUsage, *compaction.Usage)
			observerMu.Unlock()
		}
		return emit(engine.Event{Kind: engine.ContextUpdated, SessionID: id, Context: &report})
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
		if sendErr := emit(engine.Event{Kind: engine.Error, Text: boundedError(err.Error()), Failure: failure}); sendErr != nil {
			return errors.Join(err, sendErr)
		}
		return errors.Join(err, emit(engine.Event{Kind: engine.Completed, SessionID: id, Outcome: outcome}))
	}
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: id}); err != nil {
		return settleOwned(err)
	}
	open := make(map[string]openTool)
	// Product events get the same redaction the native store applies to the
	// records they reference (credential keys and markers; Alva configures no
	// literals), so the conversation never shows what the trace hides. With no
	// literals NewRedactor cannot fail, and a nil Redactor still applies the
	// record markers, so this never ends the stream early.
	redactor, _ := redaction.NewRedactor()
	var text strings.Builder
	var usage engine.Usage
	var callbackErr error
	blocks := 0
	responseStarted := false
	responseIndex := 0
	var input []wire.Message
	if request.WaitRevision == 0 {
		input = []wire.Message{wire.TextMessage(wire.RoleUser, request.Prompt)}
	}
	err = runner.Run(ctx, input, func(event abi.Event) error {
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
				text, truncated := bounded(message.Text(), 60<<10)
				output = &engine.Event{Kind: engine.Message, ID: id, Text: text, Truncated: truncated, Reference: reference}
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
			text, truncated := bounded(visible, 16<<10)
			output = &engine.Event{Kind: engine.ContentBlock, ID: id, Name: string(block.Kind), Text: text, Truncated: truncated}
		case abi.EventToolStarted:
			if event.ToolCall == nil {
				return errors.New("tool start missing payload")
			}
			call := event.ToolCall
			// Arguments are redacted as the native intent record is, before
			// they are bounded or presented.
			arguments, redactErr := redactor.JSON(call.Arguments)
			if redactErr != nil {
				arguments = nil
			}
			display := finishDisplay(startDisplay(call.Name, arguments), redactor)
			open[call.ID] = openTool{name: call.Name, display: display}
			// The kernel event carries no effect self-description (that lives
			// in the tool intent record), so the effect stays unknown here.
			output = &engine.Event{Kind: engine.ToolStart, ID: call.ID, Name: call.Name, Input: boundedInput(arguments), Reference: reference, Effect: engine.EffectUnknown, Display: display}
		case abi.EventToolCompleted:
			if event.ToolResult == nil {
				return errors.New("tool end missing payload")
			}
			tool := open[event.ToolResult.CallID]
			delete(open, event.ToolResult.CallID)
			// Each content block is redacted as in the native outcome record,
			// then the joined text is bounded.
			result := redactResult(*event.ToolResult, redactor)
			text, truncated := bounded(toolResultText(result), toolResultPrefix)
			output = &engine.Event{Kind: engine.ToolEnd, ID: result.CallID, Name: tool.name, Reference: reference, IsError: result.IsError, Text: text, Truncated: truncated, Display: finishDisplay(endDisplay(tool.display, result), redactor)}
		case abi.EventUsage:
			if event.Usage != nil {
				responseStarted = true
				// Cache counters keep the kernel's pointer meaning, and a
				// response that did not report them leaves the run's sum
				// unreported (a mixed sum would understate the hit rate).
				usage.Add(engine.Usage{InputTokens: event.Usage.InputTokens, OutputTokens: event.Usage.OutputTokens, CachedInputTokens: event.Usage.CachedInputTokens, CacheWriteInputTokens: event.Usage.CacheWriteInputTokens})
			}
		}
		if output != nil {
			callbackErr = emit(*output)
			return callbackErr
		}
		return nil
	})
	err = errors.Join(settleOwned(errors.Join(err, callbackErr)), ctx.Err())
	// Tool cleanup precedes fatal Error, preserving the final-only contract.
	for callID, tool := range open {
		if sendErr := emit(engine.Event{Kind: engine.ToolEnd, ID: callID, Name: tool.name, IsError: true, Text: "Execution ended before tool settlement was observed; inspect native facts.", Display: tool.display}); sendErr != nil {
			return errors.Join(err, sendErr)
		}
	}
	outcome := engine.Succeeded
	var checkpoint *engine.WaitCheckpoint
	if err == nil {
		if source, ok := runner.(engine.WaitingRunner); ok {
			checkpoint = source.WaitCheckpoint()
			if checkpoint != nil {
				if !a.options.DurableWaits || checkpoint.Validate() != nil {
					err = errors.New("runner reported an invalid or unsupported waiting checkpoint")
					checkpoint = nil
				} else {
					outcome = engine.Waiting
				}
			}
		}
	}
	if err != nil {
		var failure engine.FailureKind
		outcome, failure = executionFailure(ctx, err, responseStarted)
		if sendErr := emit(engine.Event{Kind: engine.Error, Text: boundedError(err.Error()), Failure: failure}); sendErr != nil {
			return errors.Join(err, sendErr)
		}
	}
	observerMu.Lock()
	for _, part := range compactionUsage {
		usage.Add(part)
	}
	observerMu.Unlock()
	if usage.Validate() != nil {
		// A provider's cache counts above its own input cannot be shown as a
		// hit rate; keep the token counts and report the cache as unknown.
		usage.CachedInputTokens, usage.CacheWriteInputTokens = nil, nil
	}
	return emit(engine.Event{Kind: engine.Completed, SessionID: id, Outcome: outcome, Usage: &usage, Wait: checkpoint})
}

// bounded returns a UTF-8-safe prefix and whether bytes were dropped. Final
// messages and content blocks report that through engine.Event.Truncated.
func bounded(value string, limit int) (string, bool) {
	if len(value) <= limit {
		return value, false
	}
	value = value[:limit]
	for !utf8.ValidString(value) && len(value) > 0 {
		value = value[:len(value)-1]
	}
	return value, true
}

// openTool is a started call awaiting its result: its name and the display
// derived from its arguments, which the end display extends.
type openTool struct {
	name    string
	display *engine.ToolDisplay
}

// Tool events copy only bounded evidence into product history: arguments up
// to the contract's 16 KiB input bound and an 8 KiB result prefix. The kernel
// records behind each event's Reference keep the complete call and result.
const (
	toolInputLimit   = 16 << 10
	toolResultPrefix = 8 << 10
)

// boundedInput returns a copy of a call's JSON arguments when they fit the
// input bound; larger or malformed arguments are reached only by Reference.
func boundedInput(arguments json.RawMessage) json.RawMessage {
	if len(arguments) == 0 || len(arguments) > toolInputLimit || !json.Valid(arguments) {
		return nil
	}
	return append(json.RawMessage(nil), arguments...)
}

// redactResult returns a copy whose text blocks and reference metadata are
// redacted with the record rule.
func redactResult(result wire.ToolResult, redactor *redaction.Redactor) wire.ToolResult {
	result.Content = append([]wire.ContentBlock(nil), result.Content...)
	for i, block := range result.Content {
		switch {
		case block.Kind == wire.ContentText:
			result.Content[i].Text = redactor.Text(block.Text)
		case block.Kind == wire.ContentReference && block.Reference != nil:
			reference := *block.Reference
			reference.Processing = redactor.Text(reference.Processing)
			result.Content[i].Reference = &reference
		}
	}
	return result
}

// toolResultText renders a tool result as text: text blocks verbatim and a
// session reference as its small JSON locator. Other content (media, opaque
// provider state) is named but never decoded or copied.
func toolResultText(result wire.ToolResult) string {
	var parts []string
	for _, block := range result.Content {
		switch {
		case block.Kind == wire.ContentText:
			parts = append(parts, block.Text)
		case block.Kind == wire.ContentReference && block.Reference != nil:
			data, err := json.Marshal(struct {
				Reference wire.Reference `json:"reference"`
			}{*block.Reference})
			if err == nil {
				parts = append(parts, string(data))
			}
		default:
			parts = append(parts, "["+string(block.Kind)+" content retained in the native record]")
		}
	}
	return strings.Join(parts, "\n")
}

// boundedError keeps the visible marker: Error events cannot carry Truncated.
func boundedError(value string) string {
	text, truncated := bounded(value, 4096)
	if truncated {
		text += " [bounded]"
	}
	return text
}
