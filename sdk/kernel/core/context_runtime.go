// INPUT: context, encoding/json, errors, fmt, reflect, unicode/utf8, kernel/abi, llm/wire
// OUTPUT: ContextRuntime
// POS: [L2 kernel runtime] Coordinates lifecycle timing and prepare-model context without owning injection/compression policy.
package core

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"reflect"
	"unicode/utf8"
)

type ContextRuntime struct {
	agentID string
	system  *abi.ContextSystem
	pending []abi.Injection
}

func NewContextRuntime(agentID string, system *abi.ContextSystem) *ContextRuntime {
	return &ContextRuntime{agentID: agentID, system: system}
}
func (runtime *ContextRuntime) Bootstrap(ctx context.Context) error {
	if err := validateContextSystem(runtime.system); err != nil {
		return err
	}
	if runtime.system != nil {
		if hook, ok := runtime.system.Hooks.(abi.ContextBootstrapHook); ok {
			return hook.Bootstrap(ctx, runtime.system.Handle, runtime.agentID)
		}
	}
	return nil
}

func validateContextSystem(system *abi.ContextSystem) error {
	if system == nil {
		return nil
	}
	if nilImplementation(system.Hooks) || nilImplementation(system.Handle) || system.Apply != nil && nilImplementation(system.Apply) || system.Counter != nil && nilImplementation(system.Counter) {
		return errors.New("invalid context system")
	}
	return nil
}

func nilImplementation(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		return v.IsNil()
	default:
		return false
	}
}

// Load selects a working-view source without changing the bound conversation
// log. A configured source is authoritative: failures never trigger full reads.
func (runtime *ContextRuntime) Load(ctx context.Context, log abi.ConversationReader, source abi.ContextSource) ([]wire.Message, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := validateContextSystem(runtime.system); err != nil {
		return nil, err
	}
	if nilImplementation(log) || log.ID() == "" || log.ID() != runtime.agentID {
		return nil, errors.New("context source does not belong to this runtime session")
	}
	var messages []wire.Message
	var err error
	if source != nil {
		if nilImplementation(source) {
			return nil, errors.New("typed-nil context source")
		}
		messages, err = source.Load(ctx, contextReader{reader: log})
		if err != nil {
			return nil, fmt.Errorf("load context working view: %w", err)
		}
	} else {
		messages, err = log.Messages(ctx)
		if err != nil {
			return nil, &abi.SessionError{Operation: "messages", SessionID: log.ID(), Err: err}
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	for _, message := range messages {
		if err := message.Validate(); err != nil {
			return nil, fmt.Errorf("invalid context working view: %w", err)
		}
	}
	return wire.CloneMessages(messages), nil
}

type contextReader struct{ reader abi.ConversationReader }

func (reader contextReader) ID() string { return reader.reader.ID() }
func (reader contextReader) Messages(ctx context.Context) ([]wire.Message, error) {
	return reader.reader.Messages(ctx)
}

func (runtime *ContextRuntime) OnMessage(ctx context.Context, message wire.Message) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := validateContextSystem(runtime.system); err != nil {
		return err
	}
	if runtime.system == nil {
		return nil
	}
	message = wire.CloneMessages([]wire.Message{message})[0]
	entry := abi.ContextEntry{ID: message.ID, Message: message, Metadata: abi.ContextMetadata{Layer: abi.RuntimeInject, Priority: abi.Normal, EstimatedTokens: estimateMessages([]wire.Message{message}, runtime.system.Counter)}}
	if hook, ok := runtime.system.Hooks.(abi.ContextIngestHook); ok {
		action, err := hook.Ingest(ctx, runtime.system.Handle, runtime.agentID, entry)
		if err != nil {
			return err
		}
		switch action.Kind {
		case abi.IngestKeep:
		case abi.IngestSkip:
			if err := runtime.system.Handle.InjectMessage(runtime.agentID, abi.RuntimeInject, entry.Message); err != nil {
				return err
			}
			return runtime.system.Handle.TagExclude(runtime.agentID, entry.ID)
		case abi.IngestModify:
			if action.Message == nil {
				return errors.New("context ingest modify missing message")
			}
			entry.Message = wire.CloneMessages([]wire.Message{*action.Message})[0]
			entry.Message.ID = entry.ID
			if action.Priority != nil {
				entry.Metadata.Priority = *action.Priority
			}
		default:
			return errors.New("unknown context ingest action")
		}
	}
	if err := runtime.system.Handle.InjectMessage(runtime.agentID, abi.RuntimeInject, entry.Message); err != nil {
		return err
	}
	if err := runtime.system.Handle.TagPriority(runtime.agentID, entry.ID, entry.Metadata.Priority); err != nil {
		return err
	}
	if hook, ok := runtime.system.Hooks.(abi.ContextMessageHook); ok {
		injections, err := hook.OnMessage(ctx, runtime.system.Handle, runtime.agentID, entry.Message)
		if err != nil {
			return err
		}
		runtime.pending = append(runtime.pending, injections...)
	}
	return nil
}
func (runtime *ContextRuntime) Prepare(ctx context.Context, prompt []string, messages []wire.Message) ([]string, []wire.Message, error) {
	if err := validateContextSystem(runtime.system); err != nil {
		return nil, nil, err
	}
	working := wire.CloneMessages(messages)
	prompts := append([]string(nil), prompt...)
	if runtime.system == nil {
		return prompts, working, nil
	}
	for i := range working {
		if working[i].ID == "" {
			working[i].ID = fmt.Sprintf("%s/message/%d", runtime.agentID, i)
		}
	}
	if len(runtime.pending) > 0 {
		pending := runtime.pending
		runtime.pending = nil
		if runtime.system.Apply != nil {
			if err := runtime.system.Apply.ApplyInjections(ctx, pending, &prompts, &working); err != nil {
				return nil, nil, err
			}
		}
	}
	budget := runtime.system.BudgetTokens
	if budget <= 0 {
		budget = runtime.system.Handle.Budget(runtime.agentID).BudgetTokens
	}
	if budget <= 0 && runtime.system.Counter != nil {
		budget = runtime.system.Counter.ContextWindow()
	}
	if budget <= 0 {
		budget = int(^uint(0) >> 1)
	}
	var entries []abi.ContextEntry
	for _, message := range working {
		entry := abi.ContextEntry{ID: message.ID, Message: message, Metadata: abi.ContextMetadata{Layer: abi.RuntimeInject, Priority: abi.Normal, EstimatedTokens: estimateMessages([]wire.Message{message}, runtime.system.Counter)}}
		if stored, ok := runtime.system.Handle.ReadMessage(runtime.agentID, message.ID); ok {
			entry = stored
		}
		if !entry.Metadata.Excluded {
			entries = append(entries, entry)
		}
	}
	working = make([]wire.Message, len(entries))
	for i, entry := range entries {
		working[i] = entry.Message
	}
	if hook, ok := runtime.system.Hooks.(abi.ContextAssembleHook); ok {
		assembled, err := hook.Assemble(ctx, runtime.system.Handle, runtime.agentID, entries, budget)
		if err != nil {
			return nil, nil, err
		}
		working = make([]wire.Message, len(assembled))
		for i, entry := range assembled {
			working[i] = entry.Message
		}
	}
	tokens := estimateMessages(working, runtime.system.Counter)
	for _, segment := range prompts {
		tokens += estimateText(segment, runtime.system.Counter)
	}
	if tokens > budget {
		snapshot := runtime.system.Handle.Snapshot(runtime.agentID)
		snapshot.TotalTokens, snapshot.BudgetTokens = tokens, budget
		snapshot.UsageRatio = float64(tokens) / float64(budget)
		if hook, ok := runtime.system.Hooks.(abi.ContextBudgetHook); ok {
			actions, err := hook.OnBudgetExceeded(ctx, runtime.system.Handle, runtime.agentID, snapshot)
			if err != nil {
				return nil, nil, err
			}
			if runtime.system.Apply != nil {
				if err := runtime.system.Apply.ApplyCompressions(ctx, actions, &working, runtime.system.Handle, runtime.agentID); err != nil {
					return nil, nil, err
				}
			}
		}
	}
	return prompts, working, nil
}
func (runtime *ContextRuntime) AfterTurn(ctx context.Context) error {
	if err := validateContextSystem(runtime.system); err != nil {
		return err
	}
	if runtime.system != nil {
		if hook, ok := runtime.system.Hooks.(abi.ContextAfterTurnHook); ok {
			return hook.AfterTurn(ctx, runtime.system.Handle, runtime.agentID)
		}
	}
	return nil
}
func (runtime *ContextRuntime) Dispose(ctx context.Context) error {
	if err := validateContextSystem(runtime.system); err != nil {
		return err
	}
	if runtime.system != nil {
		if hook, ok := runtime.system.Hooks.(abi.ContextDisposeHook); ok {
			return hook.Dispose(ctx)
		}
	}
	return nil
}
func estimateText(text string, counter abi.TokenCounter) int {
	if counter != nil {
		tokens := counter.CountTokens(text)
		if tokens > 0 {
			return tokens
		}
		return 0
	}
	return (utf8.RuneCountInString(text) + 3) / 4
}
func estimateMessages(messages []wire.Message, counter abi.TokenCounter) int {
	tokens := 0
	for _, message := range messages {
		encoded, _ := json.Marshal(message.Content)
		tokens += estimateText(string(encoded), counter) + 4
	}
	return tokens
}
