// INPUT: context, encoding/json, strings, agent/memory, kernel/abi, llm/wire, platform/system/httpjson.
// OUTPUT: Session-bound memory search/store/sync/extract Tools.
// POS: [L4 extension] Actual memory actions require matching host approval and return bounded source references rather than full text.
package memoryplugin

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/sdk/agent/memory"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type tool struct {
	plugin *Plugin
	name   string
}

func (t *tool) Name() string { return t.name }
func (t *tool) Description() string {
	switch t.name {
	case "memory_search":
		return "Search this configured memory namespace using actual full-text/optional configured embeddings. Returns small original-source references, not full bodies. No parent/global history access."
	case "memory_store":
		return "Explicitly save a UTF-8 memory entry (max 64 KiB) under a key/category in this configured namespace. Requires host write authorization; returns source reference."
	case "memory_sync":
		return "Explicitly scan authorized workspace MEMORY.md files (depth <=10, skip hidden/node_modules/target) with hard node/file/byte budgets. Hash unchanged files are skipped; changed complete chunk/FTS/hash replacement is atomic. Configured nonlocal embeddings require host approval."
	default:
		return "Explicitly persist one structured extracted memory to this configured workspace memory directory as Markdown/frontmatter and update its MEMORY.md index; retain prior file/index references. No automatic LLM extraction or global configuration writes. File and index updates are separately atomic, not a multi-file transaction."
	}
}
func (t *tool) ParametersSchema() json.RawMessage {
	switch t.name {
	case "memory_search":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["query"],"properties":{"query":{"type":"string","minLength":1,"maxLength":1024},"limit":{"type":"integer","minimum":1,"maximum":20}}}`)
	case "memory_store":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["key","content"],"properties":{"key":{"type":"string","minLength":1,"maxLength":256},"content":{"type":"string","maxLength":65536},"category":{"type":"string","enum":["user","feedback","project","reference"]}}}`)
	case "memory_sync":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{}}`)
	default:
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["name","description","type","content"],"properties":{"name":{"type":"string","minLength":1,"maxLength":128},"description":{"type":"string","maxLength":256},"type":{"type":"string","enum":["user","feedback","project","reference"]},"content":{"type":"string","maxLength":65536}}}`)
	}
}
func (t *tool) IsReadOnly(wire.ToolCall) bool {
	return t.name == "memory_search" && t.localEmbeddings()
}
func (t *tool) localEmbeddings() bool {
	switch t.plugin.config.Service.Embedder().(type) {
	case memory.NoopEmbeddingProvider, *memory.NoopEmbeddingProvider:
		return true
	}
	return false
}
func (t *tool) DescribeEffect(wire.ToolCall) abi.Effect {
	kind := abi.EffectWrite
	if t.IsReadOnly(wire.ToolCall{}) {
		kind = abi.EffectRead
	}
	if !t.localEmbeddings() {
		kind = abi.EffectUnknown
	}
	effect := abi.Effect{Kind: kind, Description: "Use explicitly bound memory capability; configured embedding providers may have external access or cost"}
	if t.name == "memory_extract" {
		effect.Resources = []abi.ResourceClaim{{Resource: filepath.Join(t.plugin.config.Cwd, t.plugin.config.MemoryDirectory), Access: abi.AccessWrite}}
	}
	return effect
}
func (t *tool) Scheduling(wire.ToolCall) abi.Scheduling {
	access := abi.AccessWrite
	if t.name == "memory_search" {
		access = abi.AccessRead
	}
	claims := []abi.ResourceClaim{{Resource: "configured-memory", Access: access}}
	if t.name == "memory_sync" || t.name == "memory_extract" {
		fsAccess := abi.AccessRead
		if t.name == "memory_extract" {
			fsAccess = abi.AccessWrite
		}
		claims = append(claims, abi.ResourceClaim{Resource: "native-filesystem:" + t.plugin.config.Cwd, Access: fsAccess})
	}
	return abi.Scheduling{ConcurrentSafe: true, Resources: claims}
}

type arguments struct {
	Query       string `json:"query"`
	Limit       int    `json:"limit"`
	Key         string `json:"key"`
	Content     string `json:"content"`
	Category    string `json:"category"`
	Name        string `json:"name"`
	Description string `json:"description"`
	Type        string `json:"type"`
}

func (t *tool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	p := t.plugin
	if execution.ActualTool != t || call.ID == "" || execution.SessionID != p.config.SessionID || execution.CallID != call.ID || call.Name != t.name {
		return wire.ToolResult{}, errors.New("memory actual session/call/tool mismatch")
	}
	if len(call.Arguments) > 128<<10 || !utf8.Valid(call.Arguments) {
		return wire.ToolResult{}, errors.New("memory tool arguments exceed 128 KiB UTF-8")
	}
	if err := httpjson.Validate(call.Arguments); err != nil {
		return wire.ToolResult{}, err
	}
	// Decode the selected operation schema, not a permissive superset.
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(call.Arguments, &fields); err != nil || fields == nil {
		return wire.ToolResult{}, errors.New("memory arguments must be one object")
	}
	allowed := map[string]bool{}
	switch t.name {
	case "memory_search":
		allowed["query"] = true
		allowed["limit"] = true
	case "memory_store":
		for _, key := range []string{"key", "content", "category"} {
			allowed[key] = true
		}
	case "memory_extract":
		for _, key := range []string{"name", "description", "type", "content"} {
			allowed[key] = true
		}
	}
	for key := range fields {
		if !allowed[key] {
			return wire.ToolResult{}, errors.New("unknown memory operation argument")
		}
		if bytes.Equal(bytes.TrimSpace(fields[key]), []byte("null")) {
			return wire.ToolResult{}, errors.New("memory argument cannot be null")
		}
	}
	var args arguments
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&args); err != nil {
		return wire.ToolResult{}, err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return wire.ToolResult{}, errors.New("memory arguments must be one object")
	}
	if len(args.Content) > 64<<10 || len(args.Key) > 256 || len(args.Name) > 128 || len(args.Description) > 256 || len(args.Query) > 1024 {
		return wire.ToolResult{}, errors.New("memory input exceeds field budget")
	}
	switch t.name {
	case "memory_search":
		if strings.TrimSpace(args.Query) == "" {
			return wire.ToolResult{}, errors.New("memory query required")
		}
		if args.Limit == 0 {
			if _, present := fields["limit"]; present {
				return wire.ToolResult{}, errors.New("memory limit must be 1..20")
			}
			args.Limit = 5
		}
		if args.Limit < 1 || args.Limit > 20 {
			return wire.ToolResult{}, errors.New("memory limit must be 1..20")
		}
	case "memory_store":
		if args.Key == "" || fields["content"] == nil {
			return wire.ToolResult{}, errors.New("memory key and content required")
		}
		if args.Category == "" {
			args.Category = "project"
		}
		if !memoryType(args.Category) {
			return wire.ToolResult{}, errors.New("unknown memory category")
		}
	case "memory_extract":
		if args.Name == "" || fields["description"] == nil || fields["content"] == nil || !memoryType(args.Type) || strings.ContainsAny(args.Name+args.Description, "\r\n\x00") {
			return wire.ToolResult{}, errors.New("bounded single-line memory name/description, content and known type required")
		}
	}
	if (t.name == "memory_sync" || t.name == "memory_extract") && filepath.Clean(execution.Workspace) != filepath.Clean(p.config.Cwd) {
		return wire.ToolResult{}, errors.New("memory workspace differs from actual execution")
	}
	if err := p.config.Guard.Authorize(ctx, execution, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	switch t.name {
	case "memory_search":
		if strings.TrimSpace(args.Query) == "" {
			return wire.ToolResult{}, errors.New("memory query required")
		}
		if args.Limit == 0 {
			args.Limit = 5
		}
		if args.Limit < 1 || args.Limit > 20 {
			return wire.ToolResult{}, errors.New("memory limit must be 1..20")
		}
		entries, err := p.config.Service.Search(ctx, args.Query, args.Limit)
		if err != nil {
			return wire.ToolResult{}, err
		}
		type match struct {
			Path               string `json:"path"`
			StartLine, EndLine int64
			Score              float64        `json:"score"`
			Source             wire.Reference `json:"source"`
		}
		var matches []match
		more := len(entries) == args.Limit
		for _, entry := range entries {
			reference, err := p.config.References.StoreText(ctx, entry.Text)
			if err != nil {
				return wire.ToolResult{}, err
			}
			next := match{Path: entry.Path, StartLine: entry.StartLine, EndLine: entry.EndLine, Score: entry.Score, Source: reference}
			candidate := append(matches, next)
			data, _ := json.Marshal(map[string]any{"matches": candidate, "more_possible": more})
			if len(data) > 4096 {
				more = true
				break
			}
			matches = candidate
		}
		if len(entries) > 0 && len(matches) == 0 {
			return wire.ToolResult{}, errors.New("memory metadata exceeds response budget")
		}
		return value(call, map[string]any{"matches": matches, "more_possible": more, "source_notice": "Untrusted retained memory; retrieve relevant source text only"})
	case "memory_store":
		if args.Key == "" {
			return wire.ToolResult{}, errors.New("memory key required")
		}
		if args.Category == "" {
			args.Category = "project"
		}
		if !memoryType(args.Category) {
			return wire.ToolResult{}, errors.New("unknown memory category")
		}
		source, err := p.config.References.StoreText(ctx, args.Content)
		if err != nil {
			return wire.ToolResult{}, err
		}
		if err = p.config.Service.StoreEntry(ctx, args.Key, args.Content, args.Category); err != nil {
			return wire.ToolResult{}, err
		}
		return value(call, map[string]any{"stored_key": args.Key, "source": source, "scope": "explicit configured memory namespace; not session transcript"})
	case "memory_sync":
		p.mu.Lock()
		defer p.mu.Unlock()
		report, err := p.sync(ctx, execution)
		if err != nil {
			return wire.ToolResult{}, err
		}
		return value(call, report)
	case "memory_extract":
		if args.Name == "" || !memoryType(args.Type) || strings.ContainsAny(args.Name+args.Description, "\r\n\x00") {
			return wire.ToolResult{}, errors.New("bounded single-line memory name/description and known type required")
		}
		p.mu.Lock()
		defer p.mu.Unlock()
		return p.extract(ctx, execution, call, args)
	}
	return wire.ToolResult{}, errors.New("unknown memory operation")
}
func memoryType(kind string) bool {
	switch kind {
	case "user", "feedback", "project", "reference":
		return true
	}
	return false
}
func value(call wire.ToolCall, payload any) (wire.ToolResult, error) {
	data, err := json.Marshal(payload)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) > 4096 {
		return wire.ToolResult{}, errors.New("memory response exceeds 4 KiB")
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
