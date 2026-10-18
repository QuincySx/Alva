// INPUT: context, crypto/sha256, encoding/hex/json, errors, fmt, sort, strings, sync, time, kernel/abi, llm/wire
// OUTPUT: Store, NewStore, Summarizer, SummarizerFunc
// POS: [L3 capability library] Four-layer ephemeral context state with real budget, metadata, compression shortcuts and memory facts.
package agentcontext

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"sort"
	"strings"
	"sync"
	"time"
)

type Summarizer interface {
	Summarize(context.Context, []abi.ContextEntry, []string) (string, error)
}
type SummarizerFunc func(context.Context, []abi.ContextEntry, []string) (string, error)

func (fn SummarizerFunc) Summarize(ctx context.Context, entries []abi.ContextEntry, hints []string) (string, error) {
	return fn(ctx, entries, hints)
}

type contextSession struct {
	entries []abi.ContextEntry
	turn    uint64
}
type Store struct {
	mu             sync.RWMutex
	sessions       map[string]*contextSession
	facts          map[string]abi.MemoryFact
	next           uint64
	budget, window int
	counter        abi.TokenCounter
	summarizer     Summarizer
	services       StoreServices
	servicesErr    error
}

func NewStore(budget, window int, counter abi.TokenCounter, summarizer Summarizer, services ...StoreServices) *Store {
	store := &Store{sessions: make(map[string]*contextSession), facts: make(map[string]abi.MemoryFact), budget: budget, window: window, counter: counter, summarizer: summarizer}
	if len(services) > 1 {
		store.servicesErr = errors.New("at most one context services snapshot permitted")
	} else if len(services) == 1 {
		store.services = services[0]
	}
	if store.services.MaxFileBytes == 0 {
		store.services.MaxFileBytes = 64 << 10
	}
	if store.services.MaxFileBytes < 1 || store.services.MaxFileBytes > 1<<20 {
		store.servicesErr = errors.New("context file import limit must be within 1 MiB")
	}
	return store
}
func (store *Store) session(id string) *contextSession {
	session := store.sessions[id]
	if session == nil {
		session = &contextSession{}
		store.sessions[id] = session
	}
	return session
}
func (store *Store) InjectMessage(agentID string, layer abi.ContextLayer, message wire.Message) error {
	if agentID == "" {
		return errors.New("context agent identity required")
	}
	if err := message.Validate(); err != nil {
		return err
	}
	switch layer {
	case abi.AlwaysPresent, abi.OnDemand, abi.RuntimeInject, abi.MemoryLayer:
	default:
		return errors.New("unknown context layer")
	}
	message = wire.CloneMessages([]wire.Message{message})[0]
	tokens := store.tokens(message)
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	if message.ID == "" {
		store.next++
		message.ID = fmt.Sprintf("context/%s/%d", agentID, store.next)
	}
	for i, entry := range session.entries {
		if entry.ID == message.ID {
			entry.Message = message
			entry.Metadata.EstimatedTokens = tokens
			entry.Metadata.Layer = layer
			session.entries[i] = entry
			return nil
		}
	}
	session.entries = append(session.entries, abi.ContextEntry{ID: message.ID, Message: message, Metadata: abi.ContextMetadata{Layer: layer, Priority: abi.Normal, EstimatedTokens: tokens, CreatedAt: time.Now().UTC(), CreatedAtTurn: session.turn, Origin: string(message.Role)}})
	return nil
}
func (store *Store) ReadMessage(agentID, messageID string) (abi.ContextEntry, bool) {
	store.mu.RLock()
	defer store.mu.RUnlock()
	if session := store.sessions[agentID]; session != nil {
		for _, entry := range session.entries {
			if entry.ID == messageID {
				return cloneEntry(entry), true
			}
		}
	}
	return abi.ContextEntry{}, false
}
func (store *Store) Entries(agentID string) []abi.ContextEntry {
	store.mu.RLock()
	defer store.mu.RUnlock()
	session := store.sessions[agentID]
	if session == nil {
		return nil
	}
	return cloneEntries(session.entries)
}
func (store *Store) AdvanceTurn(agentID string) {
	store.mu.Lock()
	store.session(agentID).turn++
	store.mu.Unlock()
}
func (store *Store) Snapshot(agentID string) abi.ContextSnapshot {
	store.mu.RLock()
	defer store.mu.RUnlock()
	snapshot := abi.ContextSnapshot{BudgetTokens: store.budget, ModelWindow: store.window, Layers: make(map[abi.ContextLayer]abi.LayerStats)}
	session := store.sessions[agentID]
	if session == nil {
		return snapshot
	}
	for _, entry := range session.entries {
		if entry.Metadata.Excluded {
			continue
		}
		snapshot.TotalTokens += entry.Metadata.EstimatedTokens
		stats := snapshot.Layers[entry.Metadata.Layer]
		stats.Tokens += entry.Metadata.EstimatedTokens
		stats.Entries++
		snapshot.Layers[entry.Metadata.Layer] = stats
		preview := []rune(entry.Message.Text())
		if len(preview) > 128 {
			preview = preview[:128]
		}
		snapshot.Entries = append(snapshot.Entries, abi.EntrySnapshot{ID: entry.ID, Layer: entry.Metadata.Layer, Priority: entry.Metadata.Priority, EstimatedTokens: entry.Metadata.EstimatedTokens, Origin: entry.Metadata.Origin, AgeTurns: session.turn - entry.Metadata.CreatedAtTurn, Preview: string(preview)})
	}
	if store.budget > 0 {
		snapshot.UsageRatio = float64(snapshot.TotalTokens) / float64(store.budget)
	}
	for layer, stats := range snapshot.Layers {
		if snapshot.TotalTokens > 0 {
			stats.Percentage = float64(stats.Tokens) / float64(snapshot.TotalTokens)
		}
		snapshot.Layers[layer] = stats
	}
	snapshot.ToolPatterns = patterns(session.entries, 0)
	return snapshot
}
func (store *Store) Budget(agentID string) abi.BudgetInfo {
	snapshot := store.Snapshot(agentID)
	remaining := snapshot.BudgetTokens - snapshot.TotalTokens
	if remaining < 0 {
		remaining = 0
	}
	return abi.BudgetInfo{ModelWindow: snapshot.ModelWindow, BudgetTokens: snapshot.BudgetTokens, UsedTokens: snapshot.TotalTokens, RemainingTokens: remaining, UsageRatio: snapshot.UsageRatio}
}
func (store *Store) ToolPatterns(agentID string, last int) []abi.ToolPattern {
	store.mu.RLock()
	defer store.mu.RUnlock()
	if session := store.sessions[agentID]; session != nil {
		return patterns(session.entries, last)
	}
	return nil
}
func patterns(entries []abi.ContextEntry, last int) []abi.ToolPattern {
	if last > 0 && len(entries) > last {
		entries = entries[len(entries)-last:]
	}
	names := make(map[string]string)
	byName := make(map[string]abi.ToolPattern)
	for _, entry := range entries {
		for _, block := range entry.Message.Content {
			if block.ToolCall != nil {
				names[block.ToolCall.ID] = block.ToolCall.Name
				pattern := byName[block.ToolCall.Name]
				pattern.ToolName = block.ToolCall.Name
				pattern.CallCount++
				byName[pattern.ToolName] = pattern
			}
			if block.ToolResult != nil {
				if name := names[block.ToolResult.CallID]; name != "" {
					pattern := byName[name]
					pattern.TotalResultTokens += entry.Metadata.EstimatedTokens
					byName[name] = pattern
				}
			}
		}
	}
	var result []abi.ToolPattern
	for _, pattern := range byName {
		if pattern.CallCount > 0 {
			pattern.AverageResultTokens = pattern.TotalResultTokens / pattern.CallCount
		}
		result = append(result, pattern)
	}
	sort.Slice(result, func(i, j int) bool { return result[i].ToolName < result[j].ToolName })
	return result
}
func (store *Store) RemoveMessage(agentID, messageID string) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	for i, entry := range session.entries {
		if entry.ID == messageID {
			session.entries = append(session.entries[:i], session.entries[i+1:]...)
			return nil
		}
	}
	return errors.New("context message not found")
}
func (store *Store) RemoveRange(agentID string, selected abi.MessageRange) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	from, to, err := resolveRange(selected, session.entries)
	if err != nil {
		return err
	}
	session.entries = append(session.entries[:from], session.entries[to:]...)
	return nil
}
func (store *Store) RewriteMessage(agentID, messageID string, message wire.Message) error {
	if err := message.Validate(); err != nil {
		return err
	}
	message = wire.CloneMessages([]wire.Message{message})[0]
	message.ID = messageID
	tokens := store.tokens(message)
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	for i := range session.entries {
		if session.entries[i].ID == messageID {
			session.entries[i].Message = message
			session.entries[i].Metadata.EstimatedTokens = tokens
			session.entries[i].Metadata.Compacted = true
			return nil
		}
	}
	return errors.New("context message not found")
}
func (store *Store) ClearLayer(agentID string, layer abi.ContextLayer) {
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	kept := make([]abi.ContextEntry, 0, len(session.entries))
	for _, entry := range session.entries {
		if entry.Metadata.Layer != layer {
			kept = append(kept, entry)
		}
	}
	session.entries = kept
}
func (store *Store) ClearConversation(agentID string) { store.ClearLayer(agentID, abi.RuntimeInject) }
func (store *Store) ClearAll(agentID string) {
	store.mu.Lock()
	delete(store.sessions, agentID)
	store.mu.Unlock()
}
func (store *Store) SlidingWindow(agentID string, keep int) error {
	if keep < 0 {
		return errors.New("negative context window")
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	messages := make([]wire.Message, len(session.entries))
	for i, entry := range session.entries {
		messages[i] = entry.Message
	}
	cut := safeWindowCut(messages, keep)
	retained := make([]bool, len(messages))
	for i, entry := range session.entries {
		retained[i] = i >= cut || entry.Metadata.Layer != abi.RuntimeInject || entry.Metadata.Priority == abi.Critical
	}
	preserved := retainProtocolGroups(messages, retained)
	ids := make(map[string]bool, len(preserved))
	for _, message := range preserved {
		ids[message.ID] = true
	}
	var kept []abi.ContextEntry
	for _, entry := range session.entries {
		if ids[entry.Message.ID] {
			kept = append(kept, entry)
		}
	}
	session.entries = kept
	return nil
}
func (store *Store) ReplaceToolResult(agentID, messageID, summary string) error {
	entry, ok := store.ReadMessage(agentID, messageID)
	if !ok {
		return errors.New("context message not found")
	}
	for i, block := range entry.Message.Content {
		if block.ToolResult != nil {
			result := *block.ToolResult
			result.Content = []wire.ContentBlock{{Kind: wire.ContentText, Text: summary}}
			entry.Message.Content[i].ToolResult = &result
			return store.RewriteMessage(agentID, messageID, entry.Message)
		}
	}
	return errors.New("context message is not a tool result")
}
func (store *Store) Summarize(ctx context.Context, agentID string, selected abi.MessageRange, hints []string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	entries := store.Entries(agentID)
	from, to, err := resolveRange(selected, entries)
	if err != nil {
		return "", err
	}
	return store.SummarizeEntries(ctx, entries[from:to], hints)
}

// SummarizeEntries uses the exact detached projection selected by ContextApply,
// not unrelated positional entries from the backing four-layer store.
func (store *Store) SummarizeEntries(ctx context.Context, entries []abi.ContextEntry, hints []string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	entries = cloneEntries(entries)
	if store.summarizer != nil {
		return store.summarizer.Summarize(ctx, entries, append([]string(nil), hints...))
	}
	return summarizeEntries(ctx, entries)
}

func summarizeEntries(ctx context.Context, entries []abi.ContextEntry) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	var lines []string
	for _, entry := range entries {
		encoded, _ := json.Marshal(entry.Message)
		text := []rune(string(encoded))
		if len(text) > 512 {
			text = append(text[:512], []rune(" … [truncated]")...)
		}
		lines = append(lines, string(text))
	}
	return strings.Join(lines, "\n"), nil
}
func (store *Store) TagPriority(agentID, messageID string, priority abi.Priority) error {
	return store.tag(agentID, messageID, func(metadata *abi.ContextMetadata) { metadata.Priority = priority })
}
func (store *Store) TagExclude(agentID, messageID string) error {
	return store.tag(agentID, messageID, func(metadata *abi.ContextMetadata) { metadata.Excluded = true })
}
func (store *Store) tag(agentID, messageID string, update func(*abi.ContextMetadata)) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.session(agentID)
	for i := range session.entries {
		if session.entries[i].ID == messageID {
			update(&session.entries[i].Metadata)
			return nil
		}
	}
	return errors.New("context message not found")
}
func (store *Store) QueryMemory(ctx context.Context, query string, max int) ([]abi.MemoryFact, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if max < 0 || max > 1000 {
		return nil, errors.New("negative memory limit")
	}
	if store.servicesErr != nil {
		return nil, store.servicesErr
	}
	if store.services.Memory != nil {
		facts, err := store.services.Memory.Query(ctx, query, max)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return nil, err
		}
		if len(facts) > max {
			return nil, errors.New("memory backend exceeded result limit")
		}
		for _, fact := range facts {
			if err := validMemoryFact(fact); err != nil {
				return nil, err
			}
		}
		return append([]abi.MemoryFact(nil), facts...), nil
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	var result []abi.MemoryFact
	for id, fact := range store.facts {
		if strings.Contains(strings.ToLower(fact.Text), strings.ToLower(query)) {
			fact.LastAccessedAt = time.Now().UTC()
			fact.AccessCount++
			store.facts[id] = fact
			result = append(result, fact)
		}
	}
	sort.Slice(result, func(i, j int) bool {
		if result[i].Confidence == result[j].Confidence {
			return result[i].ID < result[j].ID
		}
		return result[i].Confidence > result[j].Confidence
	})
	if len(result) > max {
		result = result[:max]
	}
	return result, nil
}
func (store *Store) StoreMemory(ctx context.Context, fact abi.MemoryFact) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if store.servicesErr != nil {
		return store.servicesErr
	}
	if err := validMemoryFact(fact); err != nil {
		return err
	}
	if fact.Fingerprint == "" {
		hash := sha256.Sum256([]byte(fact.Text))
		fact.Fingerprint = hex.EncodeToString(hash[:])
	}
	if store.services.Memory != nil {
		return store.services.Memory.Store(ctx, fact)
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	if _, exists := store.facts[fact.ID]; !exists && len(store.facts) >= 10000 {
		return errors.New("context memory fact capacity exceeded")
	}
	store.facts[fact.ID] = fact
	return nil
}
func (store *Store) DeleteMemory(ctx context.Context, id string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if store.servicesErr != nil {
		return store.servicesErr
	}
	if store.services.Memory != nil {
		return store.services.Memory.Delete(ctx, id)
	}
	store.mu.Lock()
	delete(store.facts, id)
	store.mu.Unlock()
	return nil
}
func (store *Store) tokens(message wire.Message) int {
	encoded, _ := json.Marshal(message.Content)
	if store.counter != nil {
		return store.counter.CountTokens(string(encoded)) + 4
	}
	return (len([]rune(string(encoded)))+3)/4 + 4
}
func cloneEntry(entry abi.ContextEntry) abi.ContextEntry {
	entry.Message = wire.CloneMessages([]wire.Message{entry.Message})[0]
	return entry
}
func cloneEntries(entries []abi.ContextEntry) []abi.ContextEntry {
	result := make([]abi.ContextEntry, len(entries))
	for i, entry := range entries {
		result[i] = cloneEntry(entry)
	}
	return result
}
func resolveRange(selected abi.MessageRange, entries []abi.ContextEntry) (int, int, error) {
	resolve := func(selector abi.MessageSelector) (int, error) {
		switch selector.Kind {
		case abi.FromStart:
			return 0, nil
		case abi.ToEnd:
			return len(entries), nil
		case abi.ByIndex:
			if selector.Index >= 0 && selector.Index <= len(entries) {
				return selector.Index, nil
			}
		case abi.ByID:
			for i, entry := range entries {
				if entry.ID == selector.ID {
					return i, nil
				}
			}
		}
		return 0, errors.New("context message selector not found or out of range")
	}
	from, err := resolve(selected.From)
	if err != nil {
		return 0, 0, err
	}
	to, err := resolve(selected.To)
	if err != nil || to < from {
		return 0, 0, errors.New("invalid context message range")
	}
	return from, to, nil
}
