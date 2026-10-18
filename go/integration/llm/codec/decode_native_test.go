// INPUT: Native protocol tool, signed thinking, refusal, ordered content and malformed terminal fixtures
// OUTPUT: Lossless native continuation/ordering and fail-closed malformed-call regressions
// POS: [integration / protocol codec] Pure native JSON bridge tests do not exercise Magpie cross-protocol conversion or HTTP transport.
package codec

import (
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func decodeFixture(t *testing.T, protocol Protocol, events ...string) wire.Message {
	t.Helper()
	d, err := NewDecoder(protocol, Limits{})
	if err != nil {
		t.Fatal(err)
	}
	a := wire.NewStreamAccumulator(1 << 20)
	for _, raw := range events {
		if err = d.Push(t.Context(), decoderEvent(raw), a.Push); err != nil {
			t.Fatal(err)
		}
	}
	if err = d.Finish(t.Context(), a.Push); err != nil {
		t.Fatal(err)
	}
	message, err := a.Finish()
	if err != nil {
		t.Fatal(err)
	}
	return message
}

func TestDecoderChatDelayedCallsRetainOriginalAuthorityOrder(t *testing.T) {
	message := decodeFixture(t, OpenAIChat,
		`{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call","type":"function","function":{"name":"tool","arguments":"{\"x\":"}}]}}]}`,
		`{"choices":[{"index":0,"delta":{"content":"visible","refusal":"refused","reasoning_content":"thought","tool_calls":[{"index":0,"function":{"arguments":"1}"}}]}}]}`,
		`{"choices":[{"index":0,"finish_reason":"tool_calls"}]}`,
		`[DONE]`)
	if len(message.Content) != 4 || message.Content[0].ToolCall == nil || string(message.Content[0].ToolCall.Arguments) != `{"x":1}` || message.Content[1].Kind != wire.ContentRefusal || message.Content[1].Refusal.Text != "refused" || message.Content[2].Reasoning == nil || message.Content[2].Reasoning.Text != "thought" || message.Content[3].Text != "visible" {
		t.Fatal("delayed calls reordered/dropped", message.Content)
	}
}

func TestDecoderAnthropicSignedAndRedactedThinkingAndCallRemainDistinct(t *testing.T) {
	message := decodeFixture(t, Anthropic,
		`{"type":"message_start","message":{"id":"a","role":"assistant","usage":{"input_tokens":3,"output_tokens":0,"cache_read_input_tokens":4,"cache_creation_input_tokens":2}}}`,
		`{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"start"}}`,
		`{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"-end"}}`,
		`{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signature"}}`,
		`{"type":"content_block_stop","index":0}`,
		`{"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"opaque"}}`,
		`{"type":"content_block_stop","index":1}`,
		`{"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"call","name":"tool","input":{}}}`,
		`{"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{\"x\":1}"}}`,
		`{"type":"content_block_stop","index":2}`,
		`{"type":"content_block_start","index":3,"content_block":{"type":"text","text":"visible"}}`,
		`{"type":"content_block_stop","index":3}`,
		`{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":1,"cache_read_input_tokens":0}}`,
		`{"type":"message_stop"}`)
	if len(message.Content) != 4 || message.Content[0].Reasoning.Text != "start-end" || message.Content[0].Reasoning.Signature != "signature" || message.Content[1].Reasoning.RedactedData != "opaque" || message.Content[2].ToolCall.ID != "call" || message.Content[3].Text != "visible" {
		t.Fatal("thinking/call content lost", message.Content)
	}
	if message.Text() != "visible" || message.Usage == nil || message.Usage.InputTokens != 5 || *message.Usage.CachedInputTokens != 0 || *message.Usage.CacheWriteInputTokens != 2 || message.Usage.TotalTokens != 6 {
		t.Fatal("thinking mixed with answer or cache zero overwritten", message)
	}
}

func TestDecoderResponsesOpaqueReasoningAndFinalizedArgumentsKeepSlots(t *testing.T) {
	message := decodeFixture(t, OpenAIResponses,
		`{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"r","encrypted_content":"opaque","summary":[{"type":"summary_text","text":"summary"}]}}`,
		`{"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"item","call_id":"call","name":"tool","arguments":""}}`,
		`{"type":"response.function_call_arguments.delta","output_index":1,"item_id":"item","delta":"{\"x\":1}"}`,
		`{"type":"response.function_call_arguments.done","output_index":1,"item_id":"item","arguments":"{\"x\":1}"}`,
		`{"type":"response.output_text.delta","output_index":2,"content_index":0,"delta":"visible"}`,
		`{"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"item","call_id":"call","name":"tool","arguments":"{\"x\":1}","status":"completed"}}`,
		`{"type":"response.output_text.done","output_index":2,"content_index":0,"text":"visible"}`,
		`{"type":"response.output_item.done","output_index":2,"item":{"type":"message","content":[{"type":"output_text","text":"visible"}]}}`,
		`{"type":"response.completed","response":{"id":"r","status":"completed","usage":{"input_tokens":2,"output_tokens":1,"input_tokens_details":{"cached_tokens":0}}}}`)
	if len(message.Content) != 3 || message.Content[0].ProviderState == nil || !strings.Contains(string(message.Content[0].ProviderState.Data), `"encrypted_content":"opaque"`) || message.Content[1].ToolCall == nil || message.Content[1].ToolCall.ID != "call" || message.Content[2].Text != "visible" {
		t.Fatal("Responses order/continuation lost", message.Content)
	}
}

func TestDecoderGeminiThoughtSignatureBindsActualGeneratedCall(t *testing.T) {
	message := decodeFixture(t, Gemini, `{"candidates":[{"content":{"parts":[{"thought":true,"text":"reason","thoughtSignature":"r-signature"},{"functionCall":{"name":"tool","args":{"x":1}},"thoughtSignature":"tool-signature"},{"text":"visible"}]},"finishReason":"STOP"}]}`)
	if len(message.Content) != 3 || message.Content[0].Reasoning.Signature != "r-signature" || message.Content[1].ToolCall == nil || !strings.HasPrefix(message.Content[1].ToolCall.ID, "gemini_") || message.Content[2].Text != "visible" {
		t.Fatal("Gemini content lost", message.Content)
	}
	call := message.Content[1].ToolCall
	if len(call.ProviderState) != 1 || call.ProviderState[0].ToolCallID != call.ID || string(call.ProviderState[0].Data) != `{"thoughtSignature":"tool-signature"}` {
		t.Fatal("signature detached from call", call)
	}
}

func TestDecoderNativeFailuresCannotFinalizeDiscardedOutput(t *testing.T) {
	cases := []struct {
		protocol Protocol
		events   []string
	}{
		{OpenAIChat, []string{`{"choices":[{"finish_reason":"length"}]}`}},
		{OpenAIChat, []string{`{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"same","function":{"name":"a","arguments":"{}"}},{"index":1,"id":"same","function":{"name":"b","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}`, `[DONE]`}},
		{OpenAIChat, []string{`{"choices":[{"index":0},{"index":0}]}`}},
		{OpenAIChat, []string{`{"choices":[{"delta":{"audio":{"data":"AAAA"}}}]}`}},
		{OpenAIResponses, []string{`{"type":"response.output_text.delta","delta":"draft"}`, `{"type":"response.output_text.done","text":"different"}`}},
		{OpenAIResponses, []string{`{"type":"response.output_audio.delta","delta":"AAAA"}`}},
		{OpenAIResponses, []string{`{"type":"response.output_item.done","item":{"type":"message","content":[{"type":"output_image","text":"not-text"}]}}`}},
		{OpenAIResponses, []string{`{"type":"response.output_item.added","item":{"type":"function_call","id":"i","call_id":"c","name":"tool"}}`, `{"type":"response.completed","response":{"status":"completed"}}`}},
		{Anthropic, []string{`{"type":"message_start","message":{"role":"assistant"}}`, `{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"unsigned"}}`, `{"type":"content_block_stop","index":0}`}},
		{Anthropic, []string{`{"type":"message_start","message":{"role":"assistant"}}`, `{"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use"}}`}},
		{Gemini, []string{`{"candidates":[{"content":{"parts":[{"text":"visible","thoughtSignature":"unbound"}]},"finishReason":"STOP"}]}`}},
		{Gemini, []string{`{"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"image/png","data":"AAAA"}}]},"finishReason":"STOP"}]}`}},
		{Gemini, []string{`{"candidates":[{"index":0},{"index":0}]}`}},
	}
	for i, tc := range cases {
		d, _ := NewDecoder(tc.protocol, Limits{})
		var failure error
		finishes := 0
		emit := func(c wire.StreamChunk) error {
			if c.Kind == wire.ChunkFinish {
				finishes++
			}
			return nil
		}
		for _, raw := range tc.events {
			if failure = d.Push(t.Context(), decoderEvent(raw), emit); failure != nil {
				break
			}
		}
		if failure == nil || d.Terminal() || finishes != 0 || d.Finish(t.Context(), emit) == nil {
			t.Fatal("invalid native stream authorized completion", i, failure, finishes)
		}
	}
}

func toolDecoderEvents(protocol Protocol, second bool) []string {
	switch protocol {
	case OpenAIChat:
		calls := `{"index":0,"id":"a","function":{"name":"tool","arguments":"{}"}}`
		if second {
			calls += `,{"index":1,"id":"b","function":{"name":"tool","arguments":"{}"}}`
		}
		return []string{`{"choices":[{"delta":{"tool_calls":[` + calls + `]},"finish_reason":"tool_calls"}]}`, `[DONE]`}
	case OpenAIResponses:
		var events []string
		for i, id := range []string{"a", "b"} {
			if i == 1 && !second {
				break
			}
			index := []string{"0", "1"}[i]
			events = append(events, `{"type":"response.output_item.added","output_index":`+index+`,"item":{"type":"function_call","id":"item-`+id+`","call_id":"`+id+`","name":"tool","arguments":"{}"}}`, `{"type":"response.output_item.done","output_index":`+index+`,"item":{"type":"function_call","id":"item-`+id+`","call_id":"`+id+`","name":"tool","arguments":"{}","status":"completed"}}`)
		}
		return append(events, `{"type":"response.completed","response":{"status":"completed"}}`)
	case Anthropic:
		events := []string{`{"type":"message_start","message":{"role":"assistant"}}`}
		for i, id := range []string{"a", "b"} {
			if i == 1 && !second {
				break
			}
			index := []string{"0", "1"}[i]
			events = append(events, `{"type":"content_block_start","index":`+index+`,"content_block":{"type":"tool_use","id":"`+id+`","name":"tool","input":{}}}`, `{"type":"content_block_stop","index":`+index+`}`)
		}
		return append(events, `{"type":"message_delta","delta":{"stop_reason":"tool_use"}}`, `{"type":"message_stop"}`)
	case Gemini:
		parts := `{"functionCall":{"id":"a","name":"tool","args":{}}}`
		if second {
			parts += `,{"functionCall":{"id":"b","name":"tool","args":{}}}`
		}
		return []string{`{"candidates":[{"content":{"parts":[` + parts + `]},"finishReason":"STOP"}]}`}
	}
	panic("unknown native fixture protocol")
}

func TestDecoderSmallerToolAndArgumentBoundsApplyWithoutReadingOutput(t *testing.T) {
	for _, protocol := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		for _, limits := range []Limits{{MaxToolCalls: 1}, {MaxArgumentBytes: 1}} {
			d, _ := NewDecoder(protocol, limits)
			var failure error
			for _, raw := range toolDecoderEvents(protocol, limits.MaxToolCalls == 1) {
				if failure = d.Push(t.Context(), decoderEvent(raw), func(wire.StreamChunk) error { return nil }); failure != nil {
					break
				}
			}
			if failure == nil || d.Terminal() {
				t.Fatal("reduced limits ignored", protocol, limits)
			}
		}
	}
	// MaxToolCalls does not mean MaxContentBlocks: independent text blocks must
	// remain valid even when the caller permits only one actual tool call.
	d, _ := NewDecoder(Anthropic, Limits{MaxToolCalls: 1})
	for _, raw := range []string{`{"type":"message_start","message":{"role":"assistant"}}`, `{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"a"}}`, `{"type":"content_block_stop","index":0}`, `{"type":"content_block_start","index":1,"content_block":{"type":"text","text":"b"}}`, `{"type":"content_block_stop","index":1}`, `{"type":"message_delta","delta":{"stop_reason":"end_turn"}}`, `{"type":"message_stop"}`} {
		if err := d.Push(t.Context(), decoderEvent(raw), func(wire.StreamChunk) error { return nil }); err != nil {
			t.Fatal("tool limit became content limit", err)
		}
	}
	if !d.Terminal() {
		t.Fatal("text-only message did not complete")
	}
}

func TestDecoderTrailingAuthorityAfterNativeTerminalNeverFinalizes(t *testing.T) {
	for _, protocol := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		d, _ := NewDecoder(protocol, Limits{})
		emit := func(c wire.StreamChunk) error {
			if c.Kind == wire.ChunkFinish {
				t.Error("Push leaked successful finish")
			}
			return nil
		}
		for _, e := range basicDecoderEvents(protocol, "") {
			if err := d.Push(t.Context(), e, emit); err != nil {
				t.Fatal(err)
			}
		}
		if !d.Terminal() {
			t.Fatal("native terminal missing")
		}
		if d.Push(t.Context(), basicDecoderEvents(protocol, "")[0], emit) == nil || d.Finish(t.Context(), emit) == nil {
			t.Fatal("trailing authority became successful response", protocol)
		}
	}
}

func TestDecoderAnthropicCumulativeInputUsageAndExplicitZero(t *testing.T) {
	// Official message_delta usage is cumulative, not limited to output.
	// https://platform.claude.com/docs/en/build-with-claude/streaming
	for _, delta := range []struct {
		input string
		want  uint64
	}{{``, 2679}, {`,"input_tokens":10682`, 10682}, {`,"input_tokens":0`, 0}} {
		message := decodeFixture(t, Anthropic,
			`{"type":"message_start","message":{"role":"assistant","usage":{"input_tokens":2679,"output_tokens":3}}}`,
			fmt.Sprintf(`{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":510%s}}`, delta.input),
			`{"type":"message_stop"}`)
		if message.Usage == nil || message.Usage.InputTokens != delta.want || message.Usage.TotalTokens != delta.want+510 {
			t.Fatal("cumulative input was lost", delta, message.Usage)
		}
	}
	for _, usage := range []string{`"input_tokens":-1,"output_tokens":1`, `"input_tokens":18446744073709551615,"output_tokens":1`, `"input_tokens":18446744073709551615,"cache_read_input_tokens":1,"output_tokens":0`} {
		d, _ := NewDecoder(Anthropic, Limits{})
		var failure error
		for _, raw := range []string{`{"type":"message_start","message":{"role":"assistant","usage":{"input_tokens":1}}}`, `{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{` + usage + `}}`, `{"type":"message_stop"}`} {
			if failure = d.Push(t.Context(), decoderEvent(raw), func(wire.StreamChunk) error { return nil }); failure != nil {
				break
			}
		}
		if failure == nil || d.Terminal() {
			t.Fatal("negative/overflowing cumulative usage accepted", usage)
		}
	}
}
