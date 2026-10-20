// INPUT: context, crypto/rand, encoding/hex/json, errors, fmt, reflect, sync, time, kernel/abi, kernel/bus, llm/wire
// OUTPUT: AgentLoop, DefaultAgentLoop, RunConfig
// POS: [L2 kernel runtime] Replaceable agent mechanism with conflict-aware tool waves, atomic intents and terminal settlement.
package core

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"sync"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type RunConfig struct {
	State              *abi.AgentState
	Middleware         []abi.Middleware
	SystemPrompt       string
	MaxIterations      int
	MaxResponseBytes   int
	Workspace          string
	Bus                bus.Handle
	ContextSystem      *abi.ContextSystem
	ContextSource      abi.ContextSource
	MaxConcurrentTools int
	ModelConfig        wire.ModelConfig
}

type AgentLoop interface {
	Run(context.Context, RunConfig, []wire.Message, func(abi.Event) error) error
}

// DefaultAgentLoop executes explicit concurrency-safe calls in conflict-free waves.
type DefaultAgentLoop struct{}

func (DefaultAgentLoop) Run(ctx context.Context, config RunConfig, input []wire.Message, emit func(abi.Event) error) (runErr error) {
	if config.State == nil {
		return errors.New("runtime state is required")
	}
	initialSession := config.State.Session()
	if nilImplementation(initialSession) || initialSession.ID() == "" || nilImplementation(config.State.Model()) || config.State.Tools() == nil {
		return errors.New("initialized runtime state is required")
	}
	for index, layer := range config.Middleware {
		if nilImplementation(layer) {
			return fmt.Errorf("middleware %d implementation is required", index)
		}
	}
	if err := validateContextSystem(config.ContextSystem); err != nil {
		return err
	}
	if config.ContextSource != nil && nilImplementation(config.ContextSource) {
		return errors.New("context source implementation is required")
	}
	config.ModelConfig = wire.CloneModelConfig(config.ModelConfig)
	if err := config.ModelConfig.Validate(); err != nil {
		return err
	}
	if emit == nil {
		return errors.New("event callback is required")
	}
	if config.MaxIterations <= 0 {
		config.MaxIterations = 32
	}
	if config.MaxResponseBytes <= 0 {
		config.MaxResponseBytes = 4 << 20
	}
	runID, err := newID()
	if err != nil {
		return err
	}
	diagnostics, _ := bus.Resolve[DiagnosticObserver](config.Bus)
	runtime := abi.RuntimeContext{RunID: runID, State: config.State, Workspace: config.Workspace, Capabilities: config.Bus}
	session := config.State.Session()
	var touchedLogs []abi.ConversationLog
	touch := func(log abi.ConversationLog) {
		// Logical IDs can legitimately be backed by distinct pending writers.
		// Deduplicate only comparable actual handles, never by session ID.
		if reflect.ValueOf(log).Comparable() {
			for _, existing := range touchedLogs {
				if reflect.TypeOf(existing) == reflect.TypeOf(log) && reflect.ValueOf(existing).Comparable() && existing == log {
					return
				}
			}
		}
		touchedLogs = append(touchedLogs, log)
	}
	touch(session)
	// Cleanup uses a separate bounded context, not a cancelled request context.
	contextRuntime := NewContextRuntime(session.ID(), config.ContextSystem)
	// Flush failure is observable even if execution itself succeeded or failed.
	defer func() {
		cleanup, cancel := settlementContext(ctx)
		defer cancel()
		runErr = errors.Join(runErr, contextRuntime.Dispose(cleanup))
		endLog := config.State.Session()
		touch(endLog)
		endAudit := newHookBatch(runID, "on_run_end", "")
		for i := len(config.Middleware) - 1; i >= 0; i-- {
			if hook, ok := config.Middleware[i].(abi.RunEndHook); ok {
				started := time.Now()
				err := hook.OnRunEnd(cleanup, runtime, runErr)
				endAudit.observe(config.Middleware[i], i, started, err)
				runErr = errors.Join(runErr, err)
			}
		}
		appendHookBatches(cleanup, endLog, diagnostics, endAudit)
		current := config.State.Session()
		touch(current)
		for _, log := range touchedLogs {
			if err := log.Flush(cleanup); err != nil {
				runErr = errors.Join(runErr, &abi.SessionError{Operation: "flush", SessionID: log.ID(), Err: err})
			}
		}
		terminal := abi.Event{Kind: abi.EventRunCompleted}
		if runErr != nil {
			terminal.Kind = abi.EventRunFailed
			terminal.Error = runErr.Error()
		}
		// Never report success before settlement/flush. An unavailable subscriber
		// cannot prevent cleanup; terminal delivery failure remains returned.
		if ctx.Err() == nil {
			runErr = errors.Join(runErr, emit(terminal))
		}
	}()
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := invokeHookPhase(ctx, session, diagnostics, runID, "on_run_start", config.Middleware, false, func(layer abi.Middleware) (bool, error) {
		if hook, ok := layer.(abi.RunStartHook); ok {
			return true, hook.OnRunStart(ctx, runtime)
		}
		return false, nil
	}); err != nil {
		return err
	}
	session = config.State.Session()
	touch(session)
	contextRuntime = NewContextRuntime(session.ID(), config.ContextSystem)
	if err := contextRuntime.Bootstrap(ctx); err != nil {
		return err
	}
	initial := make([]abi.SessionRecord, 0, len(input))
	for _, message := range input {
		if err := message.Validate(); err != nil {
			return err
		}
		message := message
		initial = append(initial, abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
	}
	if len(initial) > 0 {
		committed, err := appendRecords(ctx, session, initial)
		if err != nil {
			return fmt.Errorf("commit input: %w", err)
		}
		for _, record := range committed {
			if record.Message != nil {
				message := *record.Message
				if err := invokeHookPhase(ctx, session, diagnostics, runID, "input_committed", config.Middleware, false, func(layer abi.Middleware) (bool, error) {
					if hook, ok := layer.(abi.InputCommittedHook); ok {
						return true, hook.InputCommitted(ctx, runtime, wire.CloneMessages([]wire.Message{message})[0])
					}
					return false, nil
				}); err != nil {
					return err
				}
				if err := contextRuntime.OnMessage(ctx, message); err != nil {
					return err
				}
			}
		}
	}
	if err := emit(abi.Event{Kind: abi.EventRunStarted}); err != nil {
		return err
	}
	for iteration := 0; iteration < config.MaxIterations; iteration++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		nextSession := config.State.Session()
		if nextSession.ID() != session.ID() {
			session = nextSession
			contextRuntime = NewContextRuntime(session.ID(), config.ContextSystem)
			if err := contextRuntime.Bootstrap(ctx); err != nil {
				return err
			}
		} else {
			session = nextSession
		}
		touch(session)
		invocationRuntime := runtime
		invocationRuntime.InvocationSessionID = session.ID()
		toolSnapshot := config.State.Tools().Snapshot()
		messages, err := contextRuntime.Load(ctx, session, config.ContextSource)
		if err != nil {
			if config.ContextSource == nil {
				return err
			}
			return &abi.SessionError{Operation: "context_source", SessionID: session.ID(), Err: err}
		}
		stablePrompt := config.SystemPrompt
		for _, addition := range config.ModelConfig.TurnSystemAdditions {
			if addition != "" {
				stablePrompt += "\n\n" + addition
			}
		}
		prompts, working, err := contextRuntime.Prepare(ctx, []string{stablePrompt, ""}, messages)
		if err != nil {
			return err
		}
		var definitions []wire.ToolDefinition
		for _, definition := range toolSnapshot.Definitions {
			if config.ModelConfig.AllowsTool(definition.Name) {
				definitions = append(definitions, definition)
			}
		}
		request := abi.ModelRequest{Messages: working, Tools: cloneDefinitions(definitions), Config: wire.CloneModelConfig(config.ModelConfig)}
		var systemMessages []wire.Message
		for _, prompt := range prompts {
			if prompt != "" {
				systemMessages = append(systemMessages, wire.TextMessage(wire.RoleSystem, prompt))
			}
		}
		request.Messages = append(systemMessages, request.Messages...)
		if err := invokeHookPhase(ctx, session, diagnostics, runID, "before_model", config.Middleware, false, func(layer abi.Middleware) (bool, error) {
			if hook, ok := layer.(abi.BeforeModelHook); ok {
				return true, hook.BeforeModel(ctx, invocationRuntime, &request)
			}
			return false, nil
		}); err != nil {
			return err
		}
		if err := request.Config.Validate(); err != nil {
			return err
		}
		filtered := request.Tools[:0]
		for _, definition := range request.Tools {
			if config.ModelConfig.AllowsTool(definition.Name) && request.Config.AllowsTool(definition.Name) {
				filtered = append(filtered, definition)
			}
		}
		request.Tools = filtered
		requestID, err := newID()
		if err != nil {
			return err
		}
		actualModel := config.State.Model()
		if _, err := appendRecords(ctx, session, []abi.SessionRecord{{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: requestID}}}); err != nil {
			return fmt.Errorf("commit model intent: %w", err)
		}
		if diagnostics != nil {
			observed := abi.ModelRequest{Messages: wire.CloneMessages(request.Messages), Tools: cloneDefinitions(request.Tools), Config: wire.CloneModelConfig(request.Config)}
			reportDiagnostic(ctx, diagnostics, session, "model start", diagnostics.ModelStart(ctx, session, ModelObservation{RunID: runID, RequestID: requestID, SystemPrompt: config.SystemPrompt, Model: actualModel, Request: observed}))
		}
		modelAudit := newHookBatch(runID, "wrap_model", "")
		admissionAudit := newHookBatch(runID, "admit_model", "")
		var observation *StreamObservation
		if diagnostics != nil {
			observation = newStreamObservation(runID, requestID)
		}
		message, calls, usage, streamErr := collectObserved(ctx, wrappedModel{runtime: invocationRuntime, middleware: config.Middleware, audit: modelAudit, admission: admissionAudit, actual: actualModel}, request, config.MaxResponseBytes, emit, observation)
		if diagnostics != nil {
			cleanup, stopAudit := settlementContext(ctx)
			reportDiagnostic(cleanup, diagnostics, session, "model stream", diagnostics.ModelEnd(cleanup, session, observation.snapshot(usage, errors.Join(streamErr, ctx.Err()))))
			stopAudit()
		}
		commitHookBatches(ctx, session, diagnostics, modelAudit, admissionAudit)
		if streamErr == nil {
			streamErr = ctx.Err()
		}
		if streamErr == nil {
			streamErr = invokeHookPhase(ctx, session, diagnostics, runID, "after_model", config.Middleware, true, func(layer abi.Middleware) (bool, error) {
				if hook, ok := layer.(abi.AfterModelHook); ok {
					return true, hook.AfterModel(ctx, invocationRuntime, &message)
				}
				return false, nil
			})
			if streamErr == nil {
				calls, streamErr = responseCalls(message)
			}
		}
		outcome := &abi.ModelOutcome{RequestID: requestID, Status: abi.OutcomeSucceeded, Usage: usage}
		if streamErr != nil {
			outcome.Status = statusFor(streamErr)
			outcome.Error = streamErr.Error()
			batch := []abi.SessionRecord{{Kind: abi.RecordModelOutcome, ModelOutcome: outcome}}
			// Incomplete tool calls must not enter the model transcript.
			if text := message.Text(); text != "" {
				partial := wire.TextMessage(wire.RoleAssistant, text)
				batch = append(batch, abi.SessionRecord{Kind: abi.RecordConversation, Message: &partial})
			}
			cleanup, cancel := settlementContext(ctx)
			_, commitErr := appendRecords(cleanup, session, batch)
			cancel()
			return errors.Join(streamErr, commitErr)
		}
		declaredCalls := calls
		tools := make(map[string]abi.Tool)
		advertisedTools := make(map[string]bool, len(request.Tools))
		for _, definition := range request.Tools {
			advertisedTools[definition.Name] = true
		}
		for _, tool := range toolSnapshot.Tools {
			if advertisedTools[tool.Name()] && config.ModelConfig.AllowsTool(tool.Name()) && request.Config.AllowsTool(tool.Name()) {
				tools[tool.Name()] = tool
			}
		}
		var prepareAudit []*hookBatch
		calls, preparedResults, failedPreparation, prepareErr := prepareToolCalls(ctx, invocationRuntime, declaredCalls, tools, config.Middleware, &prepareAudit)
		batch := []abi.SessionRecord{{Kind: abi.RecordConversation, Message: &message}, {Kind: abi.RecordModelOutcome, ModelOutcome: outcome}}
		for index, call := range calls {
			effect := abi.Effect{Kind: abi.EffectUnknown}
			scheduling := abi.Scheduling{}
			readOnly := false
			if tool, exists := tools[call.Name]; exists && prepareErr == nil {
				owned := wire.CloneToolCall(call)
				effect = tool.DescribeEffect(owned)
				mutation := !reflect.DeepEqual(owned, call)
				owned = wire.CloneToolCall(call)
				scheduling = tool.Scheduling(owned)
				mutation = mutation || !reflect.DeepEqual(owned, call)
				owned = wire.CloneToolCall(call)
				readOnly = tool.IsReadOnly(owned)
				if mutation || !reflect.DeepEqual(owned, call) {
					prepareErr = errors.New("tool descriptor mutated frozen call")
					failedPreparation = index
				}
				if err := scheduling.Validate(); err != nil {
					prepareErr = errors.Join(prepareErr, err)
					failedPreparation = index
					// A rejected descriptor cannot be committed as a valid claim.
					scheduling = abi.Scheduling{}
				}
			}
			intent := &abi.ToolIntent{RunID: runID, Call: wire.CloneToolCall(call), Effect: effect, Scheduling: scheduling, ReadOnly: readOnly}
			if !reflect.DeepEqual(call, declaredCalls[index]) {
				declared := wire.CloneToolCall(declaredCalls[index])
				intent.DeclaredCall = &declared
			}
			_, intent.NoSideEffect = preparedResults[call.ID]
			batch = append(batch, abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: intent})
		}
		commitContext := ctx
		commitCancel := func() {}
		if prepareErr != nil {
			for _, record := range batch[2:] {
				record.ToolIntent.NoSideEffect = true
			}
			batch = append(batch, failedPreparationRecords(runID, calls, failedPreparation, prepareErr)...)
			commitContext, commitCancel = settlementContext(ctx)
		}
		committed, err := appendRecords(commitContext, session, batch)
		commitCancel()
		if err != nil {
			// Atomic response failure means no tool intent was committed. Settle
			// the already committed model request, never perform a side effect.
			failed := &abi.ModelOutcome{RequestID: requestID, Status: statusFor(err), Error: err.Error(), Usage: usage}
			cleanup, cancel := settlementContext(ctx)
			_, settlementErr := appendRecords(cleanup, session, []abi.SessionRecord{{Kind: abi.RecordModelOutcome, ModelOutcome: failed}})
			cancel()
			return errors.Join(prepareErr, fmt.Errorf("commit model response and tool intents: %w", err), settlementErr)
		}
		// The committed store value is authoritative, including AfterModel
		// changes and store-owned identity. Never expose a mutable record alias.
		committedMessage := wire.CloneMessages([]wire.Message{*committed[0].Message})[0]
		messageReference := reference(session.ID(), committed[0])
		emitErr := emit(abi.Event{Kind: abi.EventMessageCommitted, Message: &committedMessage, Record: &messageReference})
		commitHookBatches(ctx, session, diagnostics, prepareAudit...)
		if prepareErr != nil {
			return errors.Join(prepareErr, emitErr)
		}
		if emitErr != nil {
			cleanup, cancel := settlementContext(ctx)
			settleErr := settleUnexecuted(cleanup, session, runID, calls, emitErr)
			cancel()
			return errors.Join(emitErr, settleErr)
		}
		if err := contextRuntime.OnMessage(ctx, *committed[0].Message); err != nil {
			cleanup, cancel := settlementContext(ctx)
			settleErr := settleUnexecuted(cleanup, session, runID, calls, err)
			cancel()
			return errors.Join(err, settleErr)
		}
		batchConfig := ToolBatchConfig{Session: session, Middleware: config.Middleware, Workspace: config.Workspace, Bus: config.Bus, MaxConcurrentTools: config.MaxConcurrentTools, PreparedResults: preparedResults, Diagnostics: diagnostics}
		if err := (ToolBatchCoordinator{}).Execute(ctx, batchConfig, runID, calls, committed[2:], tools, contextRuntime, emit); err != nil {
			return err
		}
		if len(calls) == 0 {
			return contextRuntime.AfterTurn(ctx)
		}
		if err := contextRuntime.AfterTurn(ctx); err != nil {
			return err
		}
	}
	return errors.New("maximum agent iterations reached")
}

func collect(ctx context.Context, model abi.LanguageModel, request abi.ModelRequest, maxBytes int, emit func(abi.Event) error) (wire.Message, []wire.ToolCall, *wire.Usage, error) {
	return collectObserved(ctx, model, request, maxBytes, emit, nil)
}
func collectObserved(ctx context.Context, model abi.LanguageModel, request abi.ModelRequest, maxBytes int, emit func(abi.Event) error, observation *StreamObservation) (wire.Message, []wire.ToolCall, *wire.Usage, error) {
	accumulator := wire.NewStreamAccumulator(maxBytes)
	var callbackErr error
	var callbackMu sync.Mutex
	callbackOpen := true
	err := model.Stream(ctx, request, func(chunk wire.StreamChunk) (chunkErr error) {
		callbackMu.Lock()
		defer callbackMu.Unlock()
		if !callbackOpen {
			return errors.New("model stream callback called after stream returned")
		}
		if callbackErr != nil {
			return callbackErr
		}
		defer func() {
			if chunkErr != nil {
				callbackErr = chunkErr
			}
		}()
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := accumulator.Push(chunk); err != nil {
			return err
		}
		observation.observe(chunk)
		switch chunk.Kind {
		case wire.ChunkText:
			return emit(abi.Event{Kind: abi.EventTextDelta, Text: chunk.Text})
		case wire.ChunkReasoning:
			return emit(abi.Event{Kind: abi.EventReasoningDelta, Text: chunk.Text})
		case wire.ChunkContentBlock:
			block := wire.CloneContent([]wire.ContentBlock{*chunk.ContentBlock})[0]
			return emit(abi.Event{Kind: abi.EventContentBlock, ContentBlock: &block})
		case wire.ChunkUsage:
			eventUsage := wire.CloneUsage(*chunk.Usage)
			return emit(abi.Event{Kind: abi.EventUsage, Usage: &eventUsage})
		}
		return nil
	})
	callbackMu.Lock()
	defer callbackMu.Unlock()
	callbackOpen = false
	message := accumulator.Snapshot()
	var finishErr error
	// A transport/cancellation failure already explains an interrupted stream;
	// missing finish is a protocol error only on otherwise successful EOF.
	if err == nil && callbackErr == nil && ctx.Err() == nil {
		message, finishErr = accumulator.Finish()
	}
	err = errors.Join(err, callbackErr, finishErr)
	calls, callErr := responseCalls(message)
	return message, calls, message.Usage, errors.Join(err, callErr)
}

func responseCalls(message wire.Message) ([]wire.ToolCall, error) {
	if message.Role != wire.RoleAssistant {
		return nil, errors.New("model response role must be assistant")
	}
	if len(message.Content) > wire.MaxStreamContentBlocks {
		return nil, errors.New("modified model content block count limit exceeded")
	}
	if err := message.Validate(); err != nil {
		return nil, err
	}
	var calls []wire.ToolCall
	seen := make(map[string]bool)
	for _, block := range message.Content {
		if block.Kind == wire.ContentToolCall {
			call := wire.CloneToolCall(*block.ToolCall)
			if seen[call.ID] {
				return nil, errors.New("duplicate modified tool call")
			}
			seen[call.ID] = true
			calls = append(calls, call)
		}
	}
	return calls, nil
}

func settleUnexecuted(ctx context.Context, session abi.ConversationLog, runID string, calls []wire.ToolCall, cause error) error {
	batch := make([]abi.SessionRecord, 0, len(calls)*2)
	for _, call := range calls {
		result := wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Not executed: " + cause.Error()}}}
		message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
		batch = append(batch, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: runID, CallID: call.ID, Status: abi.OutcomeCancelled, Result: result, Error: cause.Error()}}, abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
	}
	if len(batch) == 0 {
		return nil
	}
	_, err := appendRecords(ctx, session, batch)
	return err
}

type callbackSink struct{ emit func(abi.Event) error }

func (s callbackSink) Emit(ctx context.Context, event abi.Event) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	return s.emit(event)
}
func appendRecords(ctx context.Context, session abi.ConversationLog, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	for i := range records {
		records[i] = abi.BuiltinRecord(records[i])
	}
	committed, err := session.AppendRecords(ctx, records)
	if err != nil {
		return nil, &abi.SessionError{Operation: "append_records", SessionID: session.ID(), Err: err}
	}
	return committed, nil
}

func reference(sessionID string, record abi.SessionRecord) abi.RecordReference {
	return abi.RecordReference{SessionID: sessionID, RecordID: record.ID, Sequence: record.Sequence}
}
func statusFor(err error) abi.OutcomeStatus {
	if abi.OnlyCancellation(err) {
		return abi.OutcomeCancelled
	}
	return abi.OutcomeFailed
}
func settlementContext(ctx context.Context) (context.Context, context.CancelFunc) {
	return context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
}
func cloneDefinitions(tools []wire.ToolDefinition) []wire.ToolDefinition {
	result := append([]wire.ToolDefinition(nil), tools...)
	for i := range result {
		result[i].Parameters = append(json.RawMessage(nil), result[i].Parameters...)
	}
	return result
}
func newID() (string, error) {
	var bytes [16]byte
	if _, err := rand.Read(bytes[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(bytes[:]), nil
}
