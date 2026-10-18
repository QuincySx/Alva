// INPUT: context, crypto/sha256, encoding/hex/json, errors, fmt, math, reflect, strings, unicode/utf8, kernel/abi, llm/wire
// OUTPUT: StoreServices, MemoryFactsBackend, ContextFileLoader, file DTOs and Store service operations
// POS: [L3 capability library] Explicit bounded SDK capabilities, no OS IO and no mutation of durable ConversationLog facts.
package agentcontext

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"math"
	"reflect"
	"strings"
	"unicode/utf8"
)

type LineRange struct{ Start, End int }
type MessageRewrite struct {
	ID      string
	Message wire.Message
}
type ContextFileRequest struct {
	AgentID, Path string
	Lines         *LineRange
	MaxBytes      int
}
type ContextFileResult struct {
	Path, Text         string
	Source             wire.Reference
	StartLine, EndLine int
	Truncated          bool
}

// ContextFileLoader is an explicitly authorized host capability. The actual
// reader binds AgentID to its owner; a path string alone grants no access.
type ContextFileLoader interface {
	Load(context.Context, ContextFileRequest) (ContextFileResult, error)
}

type MemoryFactsBackend interface {
	Query(context.Context, string, int) ([]abi.MemoryFact, error)
	Store(context.Context, abi.MemoryFact) error
	Delete(context.Context, string) error
}
type StoreServices struct {
	Files        ContextFileLoader
	Memory       MemoryFactsBackend
	References   ReferenceBackend
	Externalizer Externalizer
	MaxFileBytes int
}

func validMemoryFact(fact abi.MemoryFact) error {
	if fact.ID == "" || fact.Text == "" || len(fact.Text) > 1<<20 || !utf8.ValidString(fact.Text) || !utf8.ValidString(fact.ID) || math.IsNaN(fact.Confidence) || math.IsInf(fact.Confidence, 0) || fact.Confidence < 0 || fact.Confidence > 1 {
		return errors.New("invalid bounded memory fact")
	}
	for _, text := range []string{fact.Fingerprint, fact.Category, fact.SourceSession} {
		if !utf8.ValidString(text) {
			return errors.New("memory fact metadata must be valid UTF-8")
		}
	}
	return nil
}
func (store *Store) InjectMemory(ctx context.Context, agentID, query string, maxTokens int) ([]abi.MemoryFact, error) {
	if agentID == "" || maxTokens < 0 || maxTokens > 1<<20 {
		return nil, errors.New("invalid memory injection budget/agent")
	}
	if maxTokens == 0 {
		return nil, ctx.Err()
	}
	facts, err := store.QueryMemory(ctx, query, 20)
	if err != nil {
		return nil, err
	}
	used := 0
	var result []abi.MemoryFact
	for _, fact := range facts {
		tokens := (len([]rune(fact.Text)) + 3) / 4
		if store.counter != nil {
			tokens = store.counter.CountTokens(fact.Text)
		}
		if tokens < 0 {
			return nil, errors.New("token counter returned negative result")
		}
		if tokens == 0 && fact.Text != "" {
			tokens = 1
		}
		if tokens > maxTokens-used {
			break
		}
		used += tokens
		result = append(result, fact)
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return result, nil
}
func (store *Store) InjectFromFile(ctx context.Context, agentID, path string, lines *LineRange) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if store.servicesErr != nil {
		return store.servicesErr
	}
	if store.services.Files == nil {
		return errors.New("authorized context file loader unavailable")
	}
	if agentID == "" || path == "" || len(path) > 8192 || !utf8.ValidString(path) || strings.ContainsRune(path, 0) {
		return errors.New("invalid context file request")
	}
	var requested *LineRange
	if lines != nil {
		value := *lines
		if value.Start < 1 || value.End < value.Start || value.End-value.Start > 1<<20 {
			return errors.New("invalid file line range")
		}
		requested = &value
	}
	result, err := store.services.Files.Load(ctx, ContextFileRequest{AgentID: agentID, Path: path, Lines: requested, MaxBytes: store.services.MaxFileBytes})
	if err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if len(result.Text) > store.services.MaxFileBytes || !utf8.ValidString(result.Text) || result.Source.Validate() != nil || result.Path == "" || !utf8.ValidString(result.Path) || result.StartLine < 1 || result.EndLine < result.StartLine {
		return errors.New("file loader returned invalid bounded projection")
	}
	if requested != nil && (result.StartLine < requested.Start || result.EndLine > requested.End) {
		return errors.New("file loader exceeded selected line range")
	}
	metadata, _ := json.Marshal(struct {
		Path      string `json:"path"`
		StartLine int    `json:"start_line"`
		EndLine   int    `json:"end_line"`
		Truncated bool   `json:"truncated"`
	}{result.Path, result.StartLine, result.EndLine, result.Truncated})
	reference := result.Source
	reference.Processing += "; Explicit bounded UTF-8 text import; no additional media transformation"
	message := wire.Message{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &reference}, {Kind: wire.ContentText, Text: "[Explicit selected file text: " + string(metadata) + "]\n" + result.Text}}}
	key := sha256.Sum256([]byte(fmt.Sprintf("%s\x00%v", path, requested)))
	message.ID = "context/file/" + hex.EncodeToString(key[:])
	return store.InjectMessage(agentID, abi.OnDemand, message)
}
func (store *Store) RewriteBatch(agentID string, rewrites []MessageRewrite) error {
	if agentID == "" {
		return errors.New("context agent identity required")
	}
	owned := make([]MessageRewrite, len(rewrites))
	tokens := make([]int, len(rewrites))
	seen := make(map[string]bool, len(rewrites))
	for i, rewrite := range rewrites {
		if rewrite.ID == "" || seen[rewrite.ID] {
			return errors.New("empty/duplicate context rewrite identity")
		}
		seen[rewrite.ID] = true
		if err := rewrite.Message.Validate(); err != nil {
			return err
		}
		rewrite.Message = wire.CloneMessages([]wire.Message{rewrite.Message})[0]
		rewrite.Message.ID = rewrite.ID
		owned[i] = rewrite
	}
	for i, rewrite := range owned {
		tokens[i] = store.tokens(rewrite.Message)
	}
	store.mu.Lock()
	defer store.mu.Unlock()
	session := store.sessions[agentID]
	if session == nil {
		return errors.New("context session not found")
	}
	indices := make(map[string]int, len(session.entries))
	for i, entry := range session.entries {
		indices[entry.ID] = i
	}
	for _, rewrite := range owned {
		if _, exists := indices[rewrite.ID]; !exists {
			return errors.New("context rewrite target missing; no batch changes applied")
		}
	}
	for i, rewrite := range owned {
		index := indices[rewrite.ID]
		session.entries[index].Message = rewrite.Message
		session.entries[index].Metadata.EstimatedTokens = tokens[i]
	}
	return nil
}
func (store *Store) Externalize(ctx context.Context, agentID string, selected abi.MessageRange, path string) (wire.Reference, error) {
	if err := ctx.Err(); err != nil {
		return wire.Reference{}, err
	}
	if store.servicesErr != nil {
		return wire.Reference{}, store.servicesErr
	}
	entries := store.Entries(agentID)
	from, to, err := resolveRange(selected, entries)
	if err != nil {
		return wire.Reference{}, err
	}
	if from == to {
		return wire.Reference{}, errors.New("cannot externalize empty context range")
	}
	messages := make([]wire.Message, len(entries))
	for i, entry := range entries {
		messages[i] = entry.Message
	}
	from, to = safeRange(messages, from, to)
	encoded, err := json.Marshal(entries[from:to])
	if err != nil {
		return wire.Reference{}, err
	}
	if len(encoded) > 8<<20 {
		return wire.Reference{}, errors.New("context externalization exceeds 8 MiB")
	}
	var reference wire.Reference
	if path != "" {
		if store.services.Externalizer == nil {
			return wire.Reference{}, errors.New("authorized context externalizer unavailable")
		}
		reference, err = store.services.Externalizer.Externalize(ctx, path, string(encoded))
	} else {
		if store.services.References == nil {
			return wire.Reference{}, errors.New("context reference backend unavailable")
		}
		reference, err = store.services.References.StoreText(ctx, string(encoded))
	}
	if err != nil {
		return wire.Reference{}, err
	}
	if err := reference.Validate(); err != nil {
		return reference, err
	}
	if err := ctx.Err(); err != nil {
		return reference, fmt.Errorf("artifact retained at %s; context unchanged: %w", reference.URI, err)
	}
	reference.Processing += "; Context entries serialized as JSON; durable conversation facts unchanged"
	key := sha256.Sum256(encoded)
	message := wire.Message{ID: "context/externalized/" + hex.EncodeToString(key[:]), Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &reference}}}
	tokens := store.tokens(message)
	store.mu.Lock()
	defer store.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return reference, fmt.Errorf("artifact retained at %s; context unchanged: %w", reference.URI, err)
	}
	session := store.sessions[agentID]
	if session == nil || !reflect.DeepEqual(session.entries, entries) {
		return reference, fmt.Errorf("context changed during externalization; artifact retained at %s; original context not replaced", reference.URI)
	}
	replacement := abi.ContextEntry{ID: message.ID, Message: message, Metadata: abi.ContextMetadata{Layer: abi.OnDemand, Priority: abi.Normal, EstimatedTokens: tokens, ExternalizedPath: path, Origin: "context.externalize"}}
	next := append([]abi.ContextEntry(nil), session.entries[:from]...)
	next = append(next, replacement)
	next = append(next, session.entries[to:]...)
	session.entries = next
	return reference, nil
}
