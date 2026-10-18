// INPUT: Native signed/refusal/tool JSON and cumulative Gemini candidate/thought/tool-prompt counters.
// OUTPUT: Exact numeric/case loss gates and inclusive native/cross-protocol usage regressions.
// POS: [integration / protocol codec] Independent review counterexamples remain executable without network or fabricated success.
package codec

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestNativeResponsesOpaqueReasoningNumbersStayExact(t *testing.T) {
	original := json.RawMessage(`{"type":"reasoning","id":"opaque-id","encrypted_content":"exact-encrypted-state","signature":"exact-signed-state","summary":[],"metadata":{"order_id":9007199254740993,"ratio":0.12345678901234567890123456789,"nested":[{"counter":9007199254740993}]}}`)
	state := append(json.RawMessage(nil), original...)
	request := Request{Model: "actual", Stream: true, Messages: []wire.Message{{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "before"}, {Kind: wire.ContentProviderState, ProviderState: &wire.ProviderState{Provider: "openai_responses", Data: state}}, {Kind: wire.ContentText, Text: "after"}}}}}
	adapter, _ := NewProtocolAdapter(OpenAIResponses)
	raw, err := adapter.EncodeRequest(context.Background(), request)
	if err != nil {
		t.Fatal(err)
	}
	assertOpaque := func(raw []byte) {
		t.Helper()
		var body struct{ Input []json.RawMessage }
		if json.Unmarshal(raw, &body) != nil || len(body.Input) != 3 {
			t.Fatalf("native ordering changed: %s", raw)
		}
		if !bytes.Equal(body.Input[1], original) {
			t.Fatalf("opaque reasoning was re-encoded through numeric any values: %s", body.Input[1])
		}
	}
	assertOpaque(raw)
	// The prepared native body also owns its raw bytes, not the caller's
	// mutable provider-state buffer.
	body, err := encodeNative(OpenAIResponses, request)
	if err != nil {
		t.Fatal(err)
	}
	state[bytes.Index(state, []byte("9007199254740993"))] = '1'
	raw, err = json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	assertOpaque(raw)
	request.Messages[0].Content[1].ProviderState.Data = json.RawMessage(`{"Type":"reasoning","encrypted_content":"exact"}`)
	if _, err = adapter.EncodeRequest(context.Background(), request); err == nil {
		t.Fatal("noncanonical reasoning type accepted")
	}
}

func TestCrossToolNumbersNeverRoundRequestCompleteOrSplitStream(t *testing.T) {
	for _, number := range []string{"9007199254740993", "0.12345678901234567890123456789", "1e400"} {
		t.Run(number, func(t *testing.T) {
			arguments := `{"nested":[{"value":` + number + `}]}`
			quoted, _ := json.Marshal(arguments)
			exchange, _ := NewExchange(OpenAIChat, Anthropic, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
			_, err := exchange.Request(context.Background(), []byte(`{"messages":[{"role":"assistant","content":null,"tool_calls":[{"id":"call","type":"function","function":{"name":"fixture","arguments":`+string(quoted)+`}}]}]}`))
			assertPrecisionLoss(t, err)
			exchange, _ = NewExchange(OpenAIChat, Anthropic, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
			if _, err = exchange.Request(context.Background(), []byte(requestFixtures[OpenAIChat])); err != nil {
				t.Fatal(err)
			}
			_, err = exchange.Response(context.Background(), []byte(`{"type":"message","role":"assistant","content":[{"type":"tool_use","id":"call","name":"fixture","input":`+arguments+`}],"stop_reason":"tool_use"}`))
			assertPrecisionLoss(t, err)
			stream := newStreamFixture(t, Gemini, Anthropic)
			var events []Event
			emit := func(event Event) error { events = append(events, event); return nil }
			initial := []string{streamFixtures[Anthropic][0], `{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call","name":"fixture","input":{}}}`}
			for _, raw := range initial {
				if err = stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
					t.Fatal(err)
				}
			}
			for _, part := range []string{arguments[:len(arguments)/2], arguments[len(arguments)/2:]} {
				data, _ := json.Marshal(map[string]any{"type": "content_block_delta", "index": 0, "delta": map[string]any{"type": "input_json_delta", "partial_json": part}})
				if err = stream.Push(context.Background(), Event{Data: data}, emit); err != nil {
					t.Fatal(err)
				}
			}
			err = stream.Push(context.Background(), Event{Data: []byte(`{"type":"content_block_stop","index":0}`)}, emit)
			assertPrecisionLoss(t, err)
			for _, event := range events {
				if terminalEvent(Gemini, event) {
					t.Fatal("rounded tool invocation received successful terminal")
				}
			}
		})
	}
	for _, number := range []string{"9007199254740992", "0.1", "0.125", "1.0", "1e3", "-0"} {
		if err := argumentNumberGuard(Anthropic, OpenAIChat, []byte(`{"x":`+number+`}`)); err != nil {
			t.Fatalf("exact roundtrip number rejected: %s %v", number, err)
		}
	}
	if err := argumentNumberGuard(Anthropic, Anthropic, []byte(`{"x":9007199254740993}`)); err != nil {
		t.Fatal("identity route does not need float conversion")
	}
}
func assertPrecisionLoss(t *testing.T, err error) {
	t.Helper()
	var loss *LossError
	if !errors.As(err, &loss) || len(loss.Diagnostics) != 1 || loss.Diagnostics[0].Code != "tool_argument_precision_lost" {
		t.Fatalf("numeric loss did not fail explicitly: %v", err)
	}
}

func TestNoncanonicalNativeKeysCannotBypassLossAndStreamGates(t *testing.T) {
	for _, response := range []string{
		`{"type":"message","role":"assistant","content":[{"type":"thinking","thinking":"private","Signature":"signed"},{"type":"text","text":"reply"}],"stop_reason":"end_turn"}`,
		`{"type":"message","role":"assistant","Content":[{"Type":"thinking","thinking":"private","signature":"signed"}],"stop_reason":"end_turn"}`,
	} {
		exchange, _ := NewExchange(OpenAIChat, Anthropic, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
		_, _ = exchange.Request(context.Background(), []byte(requestFixtures[OpenAIChat]))
		if _, err := exchange.Response(context.Background(), []byte(response)); err == nil {
			t.Fatal("schema key alias bypassed continuation guard")
		}
	}
	for _, protocol := range []Protocol{OpenAIChat, Anthropic} {
		exchange, _ := NewExchange(protocol, protocol, Options{Model: "actual"})
		raw := strings.TrimSuffix(requestFixtures[protocol], "}") + `,"stream":false,"Stream":true}`
		if _, err := exchange.Request(context.Background(), []byte(raw)); err == nil {
			t.Fatal("stream alias changed actual mode")
		}
	}
	for _, kind := range []string{"response.refusal.delta", "response.refusal.done"} {
		stream := newStreamFixture(t, OpenAIChat, OpenAIResponses)
		raw, _ := json.Marshal(map[string]any{"type": kind, "output_index": 0, "content_index": 0, "delta": "private refusal", "refusal": "private refusal"})
		var loss *LossError
		err := stream.Push(context.Background(), Event{Data: raw}, func(Event) error { t.Fatal("lossy refusal delivered"); return nil })
		if !errors.As(err, &loss) || loss.Diagnostics[0].Code != "refusal_state_lost" {
			t.Fatal("refusal event was dropped", err)
		}
	}
	// Business keys in a real tool input are not native protocol aliases.
	exchange, _ := NewExchange(OpenAIChat, Anthropic, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
	_, _ = exchange.Request(context.Background(), []byte(requestFixtures[OpenAIChat]))
	result, err := exchange.Response(context.Background(), []byte(`{"type":"message","role":"assistant","content":[{"type":"tool_use","id":"call","name":"fixture","input":{"Type":1,"type":2,"Signature":"business","nested":{"Schema":{"Type":1,"Signature":"nested"},"metadata":{"Signature":"metadata-value"}}}}],"stop_reason":"tool_use"}`))
	if err != nil {
		t.Fatal("business input was incorrectly case-folded", err)
	}
	if !strings.Contains(string(result.Body), `\"Type\":1`) {
		t.Fatal("business key changed")
	}
	for _, value := range []string{`\"Signature\":\"business\"`, `\"Schema\"`, `\"Signature\":\"nested\"`, `\"Signature\":\"metadata-value\"`} {
		if !strings.Contains(string(result.Body), value) {
			t.Fatalf("business schema/metadata key changed: %s", result.Body)
		}
	}
}

func TestGeminiInclusiveUsageAndCrossTailCounters(t *testing.T) {
	for _, target := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(target), func(t *testing.T) {
			stream := newStreamFixture(t, target, Gemini)
			var events []Event
			emit := func(event Event) error { events = append(events, event); return nil }
			for _, raw := range []string{streamFixtures[Gemini][0], `{"candidates":[{"index":0,"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":11,"toolUsePromptTokenCount":3,"candidatesTokenCount":5,"thoughtsTokenCount":7,"totalTokenCount":26,"cachedContentTokenCount":2}}`, `{"usageMetadata":{"promptTokenCount":13,"toolUsePromptTokenCount":4,"candidatesTokenCount":6,"thoughtsTokenCount":8,"totalTokenCount":31,"cachedContentTokenCount":0}}`} {
				if err := stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
					t.Fatal(err)
				}
			}
			if err := stream.Finish(context.Background(), emit); err != nil {
				t.Fatal(err)
			}
			decoder, _ := NewDecoder(target, Limits{})
			var usage *wire.Usage
			consume := func(chunk wire.StreamChunk) error {
				if chunk.Usage != nil {
					copy := wire.CloneUsage(*chunk.Usage)
					usage = &copy
				}
				return nil
			}
			for _, event := range events {
				if err := decoder.Push(context.Background(), event, consume); err != nil {
					t.Fatal(err)
				}
			}
			if err := decoder.Finish(context.Background(), consume); err != nil {
				t.Fatal(err)
			}
			if usage == nil || usage.InputTokens != 17 || usage.OutputTokens != 14 || usage.TotalTokens != 31 || usage.CachedInputTokens == nil || *usage.CachedInputTokens != 0 {
				t.Fatalf("inclusive source usage changed: %#v", usage)
			}
			if target != Anthropic && usage.ReasoningTokens != 8 {
				t.Fatalf("reasoning subset changed: %#v", usage)
			}
		})
	}
}
func TestGeminiNativeUsageZeroAndOverflow(t *testing.T) {
	for _, fields := range []string{`"promptTokenCount":18446744073709551615,"toolUsePromptTokenCount":1`, `"candidatesTokenCount":18446744073709551615,"thoughtsTokenCount":1`, `"promptTokenCount":18446744073709551615,"candidatesTokenCount":1`, `"thoughtsTokenCount":-1`} {
		decoder, _ := NewDecoder(Gemini, Limits{})
		raw := `{"candidates":[{"index":0,"finishReason":"STOP"}],"usageMetadata":{` + fields + `}}`
		if err := decoder.Push(context.Background(), Event{Data: []byte(raw)}, func(wire.StreamChunk) error { return nil }); err == nil {
			t.Fatal("invalid/overflow usage accepted", fields)
		}
	}
	decoder, _ := NewDecoder(Gemini, Limits{})
	var usage *wire.Usage
	consume := func(chunk wire.StreamChunk) error {
		if chunk.Usage != nil {
			copy := wire.CloneUsage(*chunk.Usage)
			usage = &copy
		}
		return nil
	}
	for _, raw := range []string{`{"candidates":[{"index":0,"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"toolUsePromptTokenCount":2,"candidatesTokenCount":4,"thoughtsTokenCount":5,"totalTokenCount":14}}`, `{"usageMetadata":{"promptTokenCount":0,"toolUsePromptTokenCount":0,"candidatesTokenCount":0,"thoughtsTokenCount":0,"totalTokenCount":0}}`} {
		if err := decoder.Push(context.Background(), Event{Data: []byte(raw)}, consume); err != nil {
			t.Fatal(err)
		}
	}
	if err := decoder.Finish(context.Background(), consume); err != nil {
		t.Fatal(err)
	}
	if usage == nil || usage.InputTokens != 0 || usage.OutputTokens != 0 || usage.ReasoningTokens != 0 || usage.TotalTokens != 0 {
		t.Fatal("explicit zero did not override", usage)
	}
}
func TestForeignInclusiveOutputRestoresGeminiCandidateSubset(t *testing.T) {
	stream := newStreamFixture(t, Gemini, OpenAIChat)
	var events []Event
	emit := func(event Event) error { events = append(events, event); return nil }
	for _, raw := range []string{streamFixtures[OpenAIChat][0], streamFixtures[OpenAIChat][1], `{"choices":[],"usage":{"prompt_tokens":17,"completion_tokens":12,"total_tokens":29,"completion_tokens_details":{"reasoning_tokens":7}}}`, `[DONE]`} {
		if err := stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
			t.Fatal(err)
		}
	}
	if err := stream.Finish(context.Background(), emit); err != nil {
		t.Fatal(err)
	}
	found := false
	for _, event := range events {
		var object map[string]json.RawMessage
		_ = json.Unmarshal(event.Data, &object)
		if !terminalEvent(Gemini, event) {
			continue
		}
		var usage struct{ Candidates, Thoughts, Prompt uint64 }
		var fields map[string]json.RawMessage
		_ = json.Unmarshal(object["usageMetadata"], &fields)
		_ = json.Unmarshal(fields["candidatesTokenCount"], &usage.Candidates)
		_ = json.Unmarshal(fields["thoughtsTokenCount"], &usage.Thoughts)
		_ = json.Unmarshal(fields["promptTokenCount"], &usage.Prompt)
		if usage.Candidates != 5 || usage.Thoughts != 7 || usage.Prompt != 17 || fields["toolUsePromptTokenCount"] != nil {
			t.Fatalf("Gemini subset/dimension invented: %s", event.Data)
		}
		found = true
	}
	if !found {
		t.Fatal("Gemini completion missing")
	}
}
