// INPUT: context, errors, kernel/abi, llm/wire
// OUTPUT: RulesHooks, DefaultHooks, HooksChain, DefaultSystem
// POS: [L3 capability library] Deterministic context policy and composable seven-hook lifecycle with callback fallback.
package agentcontext

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type RulesHooks struct{ MaxMessages int }

func (RulesHooks) Name() string { return "rules-context-plugin" }
func (hooks RulesHooks) OnBudgetExceeded(_ context.Context, _ abi.ContextHandle, _ string, snapshot abi.ContextSnapshot) ([]abi.CompressAction, error) {
	keep := hooks.MaxMessages
	if keep <= 0 {
		keep = 30
	}
	var actions []abi.CompressAction
	for _, entry := range snapshot.Entries {
		if entry.Priority == abi.Disposable {
			actions = append(actions, abi.CompressAction{Kind: abi.CompressPriority, Priority: abi.Disposable})
			break
		}
	}
	return append(actions, abi.CompressAction{Kind: abi.CompressWindow, KeepRecent: keep}), nil
}
func (RulesHooks) Assemble(_ context.Context, handle abi.ContextHandle, agentID string, entries []abi.ContextEntry, _ int) ([]abi.ContextEntry, error) {
	var result []abi.ContextEntry
	seen := make(map[string]bool)
	for _, entry := range handle.Entries(agentID) {
		if entry.Metadata.Layer != abi.RuntimeInject && !entry.Metadata.Excluded {
			result = append(result, entry)
			seen[entry.ID] = true
		}
	}
	for _, entry := range entries {
		if stored, ok := handle.ReadMessage(agentID, entry.ID); ok {
			entry = stored
		}
		if !entry.Metadata.Excluded && !seen[entry.ID] {
			result = append(result, cloneEntry(entry))
			seen[entry.ID] = true
		}
	}
	return result, nil
}
func (RulesHooks) AfterTurn(_ context.Context, handle abi.ContextHandle, agentID string) error {
	handle.AdvanceTurn(agentID)
	return nil
}

type DefaultHooks struct {
	RulesHooks
	DecideBudget func(context.Context, abi.ContextHandle, string, abi.ContextSnapshot) ([]abi.CompressAction, error)
}

func (DefaultHooks) Name() string { return "default-context-plugin" }
func (hooks DefaultHooks) OnBudgetExceeded(ctx context.Context, handle abi.ContextHandle, agentID string, snapshot abi.ContextSnapshot) ([]abi.CompressAction, error) {
	if hooks.DecideBudget != nil {
		actions, err := hooks.DecideBudget(ctx, handle, agentID, snapshot)
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		if err == nil && len(actions) > 0 {
			return actions, nil
		}
	}
	return hooks.RulesHooks.OnBudgetExceeded(ctx, handle, agentID, snapshot)
}
func DefaultSystem(budget, window int) *abi.ContextSystem {
	store := NewStore(budget, window, nil, nil)
	return &abi.ContextSystem{Hooks: DefaultHooks{RulesHooks: RulesHooks{MaxMessages: 30}}, Handle: store, Apply: DefaultApply{Summarizer: SummarizerFunc(store.SummarizeEntries)}, BudgetTokens: budget}
}

type HooksChain struct{ Hooks []abi.ContextHooks }

func (HooksChain) Name() string { return "context-hooks-chain" }
func (chain HooksChain) Bootstrap(ctx context.Context, handle abi.ContextHandle, id string) error {
	for _, layer := range chain.Hooks {
		if hook, ok := layer.(abi.ContextBootstrapHook); ok {
			if err := hook.Bootstrap(ctx, handle, id); err != nil {
				return err
			}
		}
	}
	return nil
}
func (chain HooksChain) OnMessage(ctx context.Context, handle abi.ContextHandle, id string, message wire.Message) ([]abi.Injection, error) {
	var all []abi.Injection
	for _, layer := range chain.Hooks {
		if hook, ok := layer.(abi.ContextMessageHook); ok {
			injections, err := hook.OnMessage(ctx, handle, id, wire.CloneMessages([]wire.Message{message})[0])
			if err != nil {
				return nil, err
			}
			all = append(all, injections...)
		}
	}
	return all, nil
}
func (chain HooksChain) OnBudgetExceeded(ctx context.Context, handle abi.ContextHandle, id string, snapshot abi.ContextSnapshot) ([]abi.CompressAction, error) {
	var all []abi.CompressAction
	for _, layer := range chain.Hooks {
		if hook, ok := layer.(abi.ContextBudgetHook); ok {
			owned := snapshot
			if snapshot.Layers != nil {
				owned.Layers = make(map[abi.ContextLayer]abi.LayerStats, len(snapshot.Layers))
				for layer, stats := range snapshot.Layers {
					owned.Layers[layer] = stats
				}
			}
			owned.Entries = append([]abi.EntrySnapshot(nil), snapshot.Entries...)
			owned.ToolPatterns = append([]abi.ToolPattern(nil), snapshot.ToolPatterns...)
			actions, err := hook.OnBudgetExceeded(ctx, handle, id, owned)
			if err != nil {
				return nil, err
			}
			all = append(all, actions...)
		}
	}
	return all, nil
}
func (chain HooksChain) Assemble(ctx context.Context, handle abi.ContextHandle, id string, entries []abi.ContextEntry, budget int) ([]abi.ContextEntry, error) {
	for _, layer := range chain.Hooks {
		if hook, ok := layer.(abi.ContextAssembleHook); ok {
			next, err := hook.Assemble(ctx, handle, id, cloneEntries(entries), budget)
			if err != nil {
				return nil, err
			}
			entries = next
		}
	}
	return entries, nil
}
func (chain HooksChain) Ingest(ctx context.Context, handle abi.ContextHandle, id string, entry abi.ContextEntry) (abi.IngestAction, error) {
	action := abi.IngestAction{Kind: abi.IngestKeep}
	for _, layer := range chain.Hooks {
		if hook, ok := layer.(abi.ContextIngestHook); ok {
			next, err := hook.Ingest(ctx, handle, id, cloneEntry(entry))
			if err != nil {
				return abi.IngestAction{}, err
			}
			switch next.Kind {
			case abi.IngestSkip:
				return next, nil
			case abi.IngestKeep:
			case abi.IngestModify:
				if next.Message == nil {
					return abi.IngestAction{}, errors.New("ingest modify missing message")
				}
				entry.Message = wire.CloneMessages([]wire.Message{*next.Message})[0]
				entry.Message.ID = entry.ID
				if next.Priority != nil {
					entry.Metadata.Priority = *next.Priority
					priority := *next.Priority
					action.Priority = &priority
				}
				action.Kind = abi.IngestModify
				message := wire.CloneMessages([]wire.Message{entry.Message})[0]
				action.Message = &message
			default:
				return abi.IngestAction{}, errors.New("unknown ingest action")
			}
		}
	}
	return action, nil
}
func (chain HooksChain) AfterTurn(ctx context.Context, handle abi.ContextHandle, id string) error {
	for _, layer := range chain.Hooks {
		if hook, ok := layer.(abi.ContextAfterTurnHook); ok {
			if err := hook.AfterTurn(ctx, handle, id); err != nil {
				return err
			}
		}
	}
	return nil
}
func (chain HooksChain) Dispose(ctx context.Context) error {
	var failures []error
	for i := len(chain.Hooks) - 1; i >= 0; i-- {
		if hook, ok := chain.Hooks[i].(abi.ContextDisposeHook); ok {
			failures = append(failures, hook.Dispose(ctx))
		}
	}
	return errors.Join(failures...)
}
