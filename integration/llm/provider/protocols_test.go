// INPUT: local httptest SSE fixtures and provider-neutral stream accumulator
// OUTPUT: Three real protocol round trips, reasoning/cache identity and malformed-stream regressions
// POS: [integration / model transport] Provider acceptance without paid endpoints, ambient accounts or fake tool execution
package provider

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func fixtureSSE(events ...string) string {
	var out strings.Builder
	for _, event := range events {
		out.WriteString("data: " + event + "\n\n")
	}
	return out.String()
}

var responsesFixture = fixtureSSE(
	`{"type":"response.reasoning_summary_text.delta","delta":"plan"}`,
	`{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"reason-1","summary":[{"type":"summary_text","text":"plan"}],"encrypted_content":"opaque"}}`,
	`{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"item-1","call_id":"call-1","name":"read_file","arguments":""}}`,
	`{"type":"response.function_call_arguments.delta","output_index":1,"item_id":"item-1","delta":"{\"path\":"}`,
	`{"type":"response.function_call_arguments.delta","output_index":1,"item_id":"item-1","delta":"\"log\"}"}`,
	`{"type":"response.function_call_arguments.done","output_index":1,"item_id":"item-1","arguments":"{\"path\":\"log\"}"}`,
	`{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"item-1","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"log\"}","status":"completed"}}`,
	`{"type":"response.completed","response":{"id":"response-1","status":"completed","usage":{"input_tokens":11,"output_tokens":7,"total_tokens":18,"input_tokens_details":{"cached_tokens":3},"output_tokens_details":{"reasoning_tokens":2}}}}`,
)
var anthropicFixture = fixtureSSE(
	`{"type":"message_start","message":{"id":"response-1","role":"assistant","usage":{"input_tokens":5,"output_tokens":1,"cache_read_input_tokens":3,"cache_creation_input_tokens":3}}}`,
	`{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}`,
	`{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"plan"}}`,
	`{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed"}}`,
	`{"type":"content_block_stop","index":0}`,
	`{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"call-1","name":"read_file","input":{}}}`,
	`{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"log\"}"}}`,
	`{"type":"content_block_stop","index":1}`,
	`{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}`,
	`{"type":"message_stop"}`,
)
var geminiFixture = fixtureSSE(
	`{"responseId":"response-1","candidates":[{"index":0,"content":{"parts":[{"text":"plan","thought":true,"thoughtSignature":"reason-sig"}]}}]}`,
	`{"candidates":[{"index":0,"content":{"parts":[{"functionCall":{"id":"call-1","name":"read_file","args":{"path":"log"}},"thoughtSignature":"call-sig"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":11,"candidatesTokenCount":7,"cachedContentTokenCount":3,"thoughtsTokenCount":2,"totalTokenCount":18}}`,
)

func TestProtocolHTTPRoundTripPreservesReasoningAndToolIdentity(t *testing.T) {
	for _, test := range []struct {
		kind                             Kind
		stream, header, value, signature string
	}{
		{OpenAIResponsesKind, responsesFixture, "Authorization", "Bearer fixture", "opaque"},
		{AnthropicKind, anthropicFixture, "X-Api-Key", "fixture", "signed"},
		{GeminiKind, geminiFixture, "X-Goog-Api-Key", "fixture", "call-sig"},
	} {
		t.Run(string(test.kind), func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Method != http.MethodPost || r.Header.Get(test.header) != test.value || r.Header.Get("X-Explicit") != "present" {
					t.Error("explicit request headers missing")
				}
				if test.kind == GeminiKind && r.URL.Query().Get("alt") != "sse" {
					t.Error("Gemini SSE query missing")
				}
				var payload map[string]json.RawMessage
				if err := json.NewDecoder(r.Body).Decode(&payload); err != nil {
					t.Error(err)
				}
				if !strings.Contains(fmt.Sprint(payload), "read_file") {
					t.Error("tool definition absent")
				}
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprint(w, test.stream)
			}))
			defer server.Close()
			config := Config{Endpoint: server.URL, Model: "fixture", APIKey: "fixture", MaxOutputTokens: 123, Headers: map[string]string{"X-Explicit": "present"}}
			model, err := Build(test.kind, config)
			if err != nil {
				t.Fatal(err)
			}
			acc := wire.NewStreamAccumulator(1 << 20)
			request := abi.ModelRequest{Messages: []wire.Message{{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "read log"}}}}, Tools: []wire.ToolDefinition{{Name: "read_file", Parameters: json.RawMessage(`{"type":"object"}`)}}}
			if err = model.Stream(context.Background(), request, acc.Push); err != nil {
				t.Fatal(err)
			}
			message, err := acc.Finish()
			if err != nil {
				t.Fatal(err)
			}
			wantOutput := uint64(7)
			if test.kind == GeminiKind {
				wantOutput = 9
			}
			if message.ResponseID != "response-1" || message.FinishReason != "tool_calls" || message.Usage == nil || message.Usage.InputTokens != 11 || message.Usage.OutputTokens != wantOutput || message.Usage.CachedInputTokens == nil || *message.Usage.CachedInputTokens != 3 {
				t.Fatalf("lost response/usage %#v", message)
			}
			var call *wire.ToolCall
			for _, block := range message.Content {
				if block.ToolCall != nil {
					call = block.ToolCall
				}
			}
			if call == nil || call.ID != "call-1" || call.Name != "read_file" || string(call.Arguments) != `{"path":"log"}` {
				t.Fatalf("lost exact function call: %#v", call)
			}
			request.Messages = append(request.Messages, message, wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "stored small reference"}}}}}})
			payload, err := encodeProtocolFixture(test.kind, config, request)
			if err != nil {
				t.Fatal(err)
			}
			encoded, _ := json.Marshal(payload)
			if !strings.Contains(string(encoded), test.signature) || !strings.Contains(string(encoded), "call-1") || !strings.Contains(string(encoded), "stored small reference") {
				t.Fatalf("continuation lost signature or pairing: %s", encoded)
			}
		})
	}
}

func TestProtocolRejectsTruncationIncompleteArgumentsAndCallbackFailure(t *testing.T) {
	for _, test := range []struct {
		name, stream string
		parse        func(context.Context, io.Reader, int, func(wire.StreamChunk) error) error
	}{
		{"responses", responsesFixture, parseResponsesFixture}, {"anthropic", anthropicFixture, parseAnthropicFixture}, {"gemini", geminiFixture, parseGeminiFixture},
	} {
		t.Run(test.name, func(t *testing.T) {
			stop := strings.LastIndex(strings.TrimSuffix(test.stream, "\n\n"), "data:")
			finished := false
			err := test.parse(context.Background(), strings.NewReader(test.stream[:stop]), 1<<20, func(c wire.StreamChunk) error {
				if c.Kind == wire.ChunkFinish {
					finished = true
				}
				return nil
			})
			if err == nil || finished {
				t.Fatalf("truncated stream accepted: %v finished=%v", err, finished)
			}
			want := errors.New("consumer failed")
			err = test.parse(context.Background(), strings.NewReader(test.stream), 1<<20, func(wire.StreamChunk) error { return want })
			if !errors.Is(err, want) {
				t.Fatalf("callback failure hidden: %v", err)
			}
			bad := strings.ReplaceAll(test.stream, `{\"path\":\"log\"}`, `{`)
			bad = strings.ReplaceAll(bad, `"args":{"path":"log"}`, `"args":[]`)
			if err = test.parse(context.Background(), strings.NewReader(bad), 1<<20, func(wire.StreamChunk) error { return nil }); err == nil {
				t.Fatal("invalid/inconsistent arguments accepted")
			}
		})
	}
}

func TestProviderConfigCannotSerializeSecretsAndCopiesHeaders(t *testing.T) {
	config := Config{Endpoint: "https://example.test/model", Model: "fixture", APIKey: "secret", Headers: map[string]string{"X-Api-Key": "private"}}
	model, err := NewOpenAIChat(config)
	if err != nil {
		t.Fatal(err)
	}
	config.Headers["X-Api-Key"] = "changed"
	if model.config.Headers["X-Api-Key"] != "private" {
		t.Fatal("configuration aliases caller headers")
	}
	data, _ := json.Marshal(config)
	if strings.Contains(string(data), "secret") || strings.Contains(string(data), "private") {
		t.Fatal("config exposed credentials")
	}
}
