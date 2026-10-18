// INPUT: context, encoding/json, agent/context, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: ReadReference, SearchReference, History, guestReferences and historical index tool
// POS: [platform / WASM host or guest] Guest-owned volatile values plus explicitly delegated read-only native authority, never a database mount.
package guest

import (
	"context"
	"encoding/json"
	"strings"

	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	kernelabi "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

func ReadReference(ctx context.Context, request abi.ReferenceRead) (abi.TextPage, error) {
	var result abi.ReferenceResult
	if err := ctx.Err(); err != nil {
		return abi.TextPage{}, err
	}
	if err := request.Validate(); err != nil {
		return abi.TextPage{}, err
	}
	data, err := abi.Encode(request, abi.MaxReferenceBytes)
	if err != nil {
		return abi.TextPage{}, err
	}
	data, err = exchange(abi.OpReferenceRead, data, abi.MaxReferenceBytes)
	if err != nil {
		return abi.TextPage{}, err
	}
	if err = abi.Decode(data, abi.MaxReferenceBytes, &result); err != nil {
		return abi.TextPage{}, err
	}
	if err = result.ValidateRead(request); err != nil {
		return abi.TextPage{}, err
	}
	return *result.Page, nil
}
func SearchReference(ctx context.Context, request abi.ReferenceSearch) ([]abi.TextMatch, error) {
	var result abi.ReferenceResult
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := request.Validate(); err != nil {
		return nil, err
	}
	data, err := abi.Encode(request, abi.MaxReferenceBytes)
	if err != nil {
		return nil, err
	}
	data, err = exchange(abi.OpReferenceSearch, data, abi.MaxReferenceBytes)
	if err != nil {
		return nil, err
	}
	if err = abi.Decode(data, abi.MaxReferenceBytes, &result); err != nil {
		return nil, err
	}
	return result.Matches, result.ValidateSearch(request)
}
func History(ctx context.Context, request abi.HistoryRequest) (abi.HistoryPage, error) {
	var result abi.HistoryPage
	if err := ctx.Err(); err != nil {
		return result, err
	}
	if err := request.Validate(); err != nil {
		return result, err
	}
	data, err := abi.Encode(request, abi.MaxReferenceBytes)
	if err != nil {
		return result, err
	}
	data, err = exchange(abi.OpHistory, data, abi.MaxReferenceBytes*4)
	if err != nil {
		return result, err
	}
	if err = abi.Decode(data, abi.MaxReferenceBytes*4, &result); err != nil {
		return result, err
	}
	return result, result.Validate(request)
}

type guestReferences struct{ local agentcontext.ReferenceBackend }

func (r guestReferences) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	return r.local.StoreText(ctx, text)
}
func (r guestReferences) ReadText(ctx context.Context, uri string, offset int64, limit int) (agentcontext.TextPage, error) {
	if strings.HasPrefix(uri, "memory-ref:") {
		return r.local.ReadText(ctx, uri, offset, limit)
	}
	p, err := ReadReference(ctx, abi.ReferenceRead{Version: abi.Version, URI: uri, Offset: offset, Limit: limit})
	return agentcontext.TextPage{URI: p.URI, Text: p.Text, Offset: p.Offset, NextOffset: p.NextOffset, TotalBytes: p.TotalBytes, Truncated: p.Truncated}, err
}
func (r guestReferences) SearchText(ctx context.Context, uri, query string, limit int) ([]agentcontext.TextMatch, error) {
	if strings.HasPrefix(uri, "memory-ref:") {
		return r.local.SearchText(ctx, uri, query, limit)
	}
	hits, err := SearchReference(ctx, abi.ReferenceSearch{Version: abi.Version, URI: uri, Query: query, Limit: limit})
	if err != nil {
		return nil, err
	}
	out := make([]agentcontext.TextMatch, len(hits))
	for i, h := range hits {
		out[i] = agentcontext.TextMatch{URI: h.URI, Text: h.Text, Offset: h.Offset}
	}
	return out, nil
}

type historyTool struct{ sessionID string }

func (historyTool) Name() string { return "native_history" }
func (historyTool) Description() string {
	return "Read/search the actual native session's bounded historical index. Results contain small metadata and record references, not loaded history bodies. Use context_read/context_search on selected URIs. No access to parent/other sessions."
}
func (historyTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"query":{"type":"string"},"after":{"type":"integer","minimum":0},"high_water":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20}},"additionalProperties":false}`)
}
func (historyTool) Scheduling(wire.ToolCall) kernelabi.Scheduling {
	return kernelabi.Scheduling{ConcurrentSafe: true}
}
func (historyTool) DescribeEffect(wire.ToolCall) kernelabi.Effect {
	return kernelabi.Effect{Kind: kernelabi.EffectRead, Description: "Read actual native session history metadata"}
}
func (historyTool) IsReadOnly(wire.ToolCall) bool { return true }
func (t historyTool) Execute(ctx context.Context, e kernelabi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
	if e.SessionID != t.sessionID {
		return fileFailure(c, "Foreign native history execution"), nil
	}
	var a struct {
		Query     string `json:"query"`
		After     uint64 `json:"after"`
		HighWater uint64 `json:"high_water"`
		Limit     int    `json:"limit"`
	}
	if err := decodeArgs(c.Arguments, &a); err != nil {
		return fileFailure(c, "Invalid history arguments"), nil
	}
	if a.Limit == 0 {
		a.Limit = 10
	}
	request := abi.HistoryRequest{Version: abi.Version, Query: a.Query, After: a.After, HighWater: a.HighWater, Limit: a.Limit}
	if err := request.Validate(); err != nil {
		return fileFailure(c, "Invalid history bounds"), nil
	}
	page, err := History(ctx, request)
	if err != nil {
		return wire.ToolResult{}, err
	}
	data, err := abi.Encode(page, abi.MaxReferenceBytes*4)
	return wire.ToolResult{CallID: c.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, err
}
