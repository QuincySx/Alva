// INPUT: encoding/json, strings, testing, llm/wire
// OUTPUT: Pure sandbox DTO/codec budget, version and lossless wire regressions
// POS: [L0 foundation] Shared host/guest protocol validation independent of the native runtime.
package abi

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestVersionAndResultUnionRejectAmbiguity(t *testing.T) {
	for _, result := range []FetchResult{{Version: Version}, {Version: Version, Response: &FetchResponse{Status: 200}, Error: "ambiguous"}, {Version: 99, Error: "wrong"}} {
		if result.Validate() == nil {
			t.Fatal("invalid envelope accepted", result)
		}
	}
	if (EscalationRequest{Version: Version, Command: "x", Cwd: "relative", TimeoutMS: 1}).Validate() == nil {
		t.Fatal("relative host cwd accepted")
	}
}
func TestCodecByteAndUTF8Budgets(t *testing.T) {
	var value Context
	for _, raw := range []string{`{"version":1,"unknown":"x"}`, "{\"version\":\"\xff\"}", `{"version":1} {}`, `{"version":1,"version":2}`} {
		if Decode([]byte(raw), MaxContextBytes, &value) == nil {
			t.Fatal("bad JSON accepted")
		}
	}
	if _, err := Encode(strings.Repeat("x", 128), 64); err == nil {
		t.Fatal("output limit ignored")
	}
}

func TestCodecRejectsInvalidDecodeBudget(t *testing.T) {
	for _, limit := range []int{0, -1, MaxResponseBytes + 1} {
		var value map[string]any
		if err := Decode([]byte(`{}`), limit, &value); err == nil {
			t.Errorf("invalid decode limit accepted: %d", limit)
		}
	}
}
func TestModelProxyRetainsReasoningMediaUsageAndToolAssociation(t *testing.T) {
	index := 0
	cached := uint64(0)
	response := ModelResponse{Version: Version, Chunks: []wire.StreamChunk{{Kind: wire.ChunkContentBlock, ContentIndex: &index, ContentBlock: &wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: "private reasoning", Signature: "sig"}}}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: 1, CachedInputTokens: &cached, ReasoningTokens: 2}}, {Kind: wire.ChunkFinish, ResponseID: "real-id", FinishReason: "stop"}}}
	if err := response.Validate(); err != nil {
		t.Fatal(err)
	}
	data, err := Encode(response, MaxResponseBytes)
	if err != nil {
		t.Fatal(err)
	}
	var cloned ModelResponse
	if err := Decode(data, MaxResponseBytes, &cloned); err != nil {
		t.Fatal(err)
	}
	if cloned.Chunks[0].ContentBlock.Reasoning.Signature != "sig" || cloned.Chunks[1].Usage.ReasoningTokens != 2 || cloned.Chunks[1].Usage.CachedInputTokens == nil || *cloned.Chunks[1].Usage.CachedInputTokens != 0 || cloned.Chunks[1].Usage.CacheWriteInputTokens != nil || cloned.Chunks[2].ResponseID != "real-id" {
		t.Fatal("proxy lost typed evidence")
	}
	var fields map[string]json.RawMessage
	data, _ = Encode(ModelRequest{Version: Version}, MaxModelRequestBytes)
	_ = json.Unmarshal(data, &fields)
	if fields["api_key"] != nil || fields["provider"] != nil {
		t.Fatal("host secret policy in DTO")
	}
}

func TestReferenceBridgeRejectsOversizeForeignAndInconsistentResults(t *testing.T) {
	q := ReferenceRead{Version: 1, URI: "opaque://bound", Offset: 3, Limit: 4}
	for _, p := range []TextPage{{URI: "opaque://other", Offset: 3, NextOffset: 3, TotalBytes: 3}, {URI: q.URI, Offset: 3, Text: "你好", NextOffset: 9, TotalBytes: 9}, {URI: q.URI, Offset: 3, Text: "ok", NextOffset: 6, TotalBytes: 6}, {URI: q.URI, Offset: 3, Text: "ok", NextOffset: 5, TotalBytes: 10, Truncated: false}} {
		if (ReferenceResult{Version: 1, Page: &p}).ValidateRead(q) == nil {
			t.Fatal("bad reference page accepted", p)
		}
	}
	for _, q := range []ReferenceSearch{{Version: 1, URI: "x", Query: strings.Repeat("x", 257), Limit: 1}, {Version: 1, URI: "x", Query: "valid", Limit: 21}} {
		if q.Validate() == nil {
			t.Fatal("unbounded search accepted")
		}
	}
	h := HistoryRequest{Version: 1, Limit: 1, HighWater: 10}
	p := HistoryPage{Version: 1, Next: 2, HighWater: 10, HasMore: true, Entries: []HistoryEntry{{Sequence: 2, Namespace: "alva", Type: "test", Value: wire.Reference{URI: "alva-artifact://opaque"}}}}
	if err := p.Validate(h); err != nil {
		t.Fatal(err)
	}
	p.HighWater = 11
	if p.Validate(h) == nil {
		t.Fatal("history highwater shifted")
	}
	p.HighWater = 10
	p.Entries[0].Sequence = 11
	if p.Validate(h) == nil {
		t.Fatal("future record reference accepted")
	}
}

func TestHistoryRejectsUTF8SplitAcrossFields(t *testing.T) {
	request := HistoryRequest{Version: Version, Limit: 1}
	page := HistoryPage{Version: Version, Next: 1, HighWater: 1, Entries: []HistoryEntry{{Sequence: 1, Namespace: "test", Type: "entry", Summary: "\xc3", Snippet: "\xa9", Value: wire.Reference{URI: "opaque://entry"}}}}
	if err := page.Validate(request); err == nil {
		t.Fatal("invalid field strings were accepted because their concatenation forms valid UTF-8")
	}
}
