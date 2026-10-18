// INPUT: Offline native JSON fixtures and real pinned Magpie conversion routes.
// OUTPUT: Cross-protocol requests/responses, alias/state/loss/cache-presence and single-use regressions.
// POS: [integration / protocol codec] Proves the raw converter is real, isolated and fail-closed rather than a neutral Chat round trip.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
)

var requestFixtures = map[Protocol]string{
	OpenAIChat:      `{"model":"alias","messages":[{"role":"user","content":"hello"}],"max_tokens":1024}`,
	OpenAIResponses: `{"model":"alias","input":"hello","max_output_tokens":1024}`,
	Anthropic:       `{"model":"alias","messages":[{"role":"user","content":"hello"}],"max_tokens":1024}`,
	Gemini:          `{"contents":[{"role":"user","parts":[{"text":"hello"}]}],"generationConfig":{"maxOutputTokens":1024}}`,
}
var completeFixtures = map[Protocol]string{
	OpenAIChat:      `{"id":"r","model":"actual","object":"chat.completion","created":1,"choices":[{"index":0,"message":{"role":"assistant","content":"reply"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}`,
	OpenAIResponses: `{"id":"r","model":"actual","object":"response","status":"completed","output":[{"id":"m","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"reply","annotations":[]}]}],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}`,
	Anthropic:       `{"id":"r","model":"actual","type":"message","role":"assistant","content":[{"type":"text","text":"reply"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":2}}`,
	Gemini:          `{"responseId":"r","modelVersion":"actual-version","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"reply"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"totalTokenCount":5}}`,
}

func TestExchangeUsesActualMagpieRoutesAndModelResponsibilities(t *testing.T) {
	for inbound, request := range requestFixtures {
		for upstream, response := range completeFixtures {
			t.Run(string(inbound)+"_"+string(upstream), func(t *testing.T) {
				exchange, err := NewExchange(inbound, upstream, Options{Model: "upstream-actual", ResponseModel: "client-alias", DefaultMaxOutputTokens: 1024})
				if err != nil {
					t.Fatal(err)
				}
				converted, err := exchange.Request(context.Background(), []byte(request))
				if err != nil {
					t.Fatal(err)
				}
				if !strings.Contains(string(converted.Body), "hello") {
					t.Fatalf("request lost content: %s", converted.Body)
				}
				if upstream != Gemini && !strings.Contains(string(converted.Body), `"model":"upstream-actual"`) {
					t.Fatalf("wrong upstream model: %s", converted.Body)
				}
				if inbound != upstream && (converted.Converter == "" || len(converted.Steps) == 0) {
					t.Fatalf("Magpie route missing: %#v", converted)
				}
				result, err := exchange.Response(context.Background(), []byte(response))
				if err != nil {
					t.Fatal(err)
				}
				if !strings.Contains(string(result.Body), "reply") {
					t.Fatalf("response lost content: %s", result.Body)
				}
				if inbound != Gemini && !strings.Contains(string(result.Body), `"model":"client-alias"`) {
					t.Fatalf("wrong downstream alias: %s", result.Body)
				}
				if inbound == Gemini && upstream != Gemini {
					var object map[string]json.RawMessage
					_ = json.Unmarshal(result.Body, &object)
					if object["modelVersion"] != nil {
						t.Fatalf("cross conversion invented Gemini native provenance: %s", result.Body)
					}
				}
				if _, err = exchange.Response(context.Background(), []byte(response)); err == nil {
					t.Fatal("response state reused")
				}
			})
		}
	}
}
func TestIdentityPreservesOpaqueNativeFieldsAndContinuation(t *testing.T) {
	for _, protocol := range []Protocol{OpenAIResponses, Gemini, Anthropic} {
		exchange, _ := NewExchange(protocol, protocol, Options{Model: "actual", ResponseModel: "alias"})
		request := strings.TrimSuffix(requestFixtures[protocol], "}") + `,"opaque_native":{"exact":[1,"x"]}}`
		result, err := exchange.Request(context.Background(), []byte(request))
		if err != nil || !strings.Contains(string(result.Body), `"opaque_native":{"exact":[1,"x"]}`) {
			t.Fatalf("opaque request lost: %s %v", result.Body, err)
		}
		response := strings.TrimSuffix(completeFixtures[protocol], "}") + `,"opaque_native":{"signed":"exact"}}`
		if protocol == Anthropic {
			response = strings.Replace(response, `"end_turn"`, `"pause_turn"`, 1)
		}
		result, err = exchange.Response(context.Background(), []byte(response))
		if err != nil || !strings.Contains(string(result.Body), `"opaque_native":{"signed":"exact"}`) {
			t.Fatalf("opaque response lost: %s %v", result.Body, err)
		}
		if protocol == Gemini && !strings.Contains(string(result.Body), `"modelVersion":"actual-version"`) {
			t.Fatal("Gemini provenance was replaced with route alias")
		}
	}
}
func TestStrictHostRejectsResponseContinuationDiagnostic(t *testing.T) {
	exchange, _ := NewExchange(OpenAIChat, Anthropic, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
	if _, err := exchange.Request(context.Background(), []byte(requestFixtures[OpenAIChat])); err != nil {
		t.Fatal(err)
	}
	_, err := exchange.Response(context.Background(), []byte(strings.Replace(completeFixtures[Anthropic], `"end_turn"`, `"pause_turn"`, 1)))
	var loss *LossError
	if !errors.As(err, &loss) {
		t.Fatalf("response diagnostics silently accepted: %v", err)
	}
	found := false
	for _, diagnostic := range loss.Diagnostics {
		found = found || diagnostic.Code == "continuation_state_lost"
	}
	if !found {
		t.Fatalf("missing continuation diagnostic: %#v", loss)
	}
}
func TestRawNativeJSONRejectsDuplicateNestedAndDepth(t *testing.T) {
	for _, raw := range []string{`{"model":"x","model":"y"}`, `{"x":{"a":1,"a":2}}`, strings.Repeat("[", 130) + "0" + strings.Repeat("]", 130)} {
		exchange, _ := NewExchange(OpenAIChat, OpenAIChat, Options{Model: "actual"})
		if _, err := exchange.Request(context.Background(), []byte(raw)); err == nil {
			t.Fatalf("ambiguous JSON accepted: %.80s", raw)
		}
	}
}
func TestExchangeConvertedCachePresenceUsesRawSource(t *testing.T) {
	for _, field := range []string{"", `,"cachedContentTokenCount":null`, `,"cachedContentTokenCount":0`, `,"cachedContentTokenCount":3`} {
		exchange, _ := NewExchange(OpenAIChat, Gemini, Options{Model: "actual"})
		if _, err := exchange.Request(context.Background(), []byte(requestFixtures[OpenAIChat])); err != nil {
			t.Fatal(err)
		}
		raw := strings.Replace(completeFixtures[Gemini], `"promptTokenCount":3`, `"promptTokenCount":3`+field, 1)
		result, err := exchange.Response(context.Background(), []byte(raw))
		if err != nil {
			t.Fatal(err)
		}
		var response struct {
			Usage struct {
				Details struct {
					Cached *uint64 `json:"cached_tokens"`
				} `json:"prompt_tokens_details"`
			}
		}
		if json.Unmarshal(result.Body, &response) != nil {
			t.Fatal("invalid conversion")
		}
		want := strings.HasSuffix(field, ":0") || strings.HasSuffix(field, ":3")
		if (response.Usage.Details.Cached != nil) != want {
			t.Fatalf("source presence changed: %q => %s", field, result.Body)
		}
		if strings.HasSuffix(field, ":0") && *response.Usage.Details.Cached != 0 {
			t.Fatal("explicit zero lost")
		}
	}
}
func TestCacheWritePresenceUsesOneNativeResponsesField(t *testing.T) {
	for _, value := range []string{"0", "3"} {
		for _, source := range []Protocol{Anthropic, OpenAIResponses} {
			target := OpenAIResponses
			if source == OpenAIResponses {
				target = Anthropic
			}
			exchange, _ := NewExchange(target, source, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
			if _, err := exchange.Request(context.Background(), []byte(requestFixtures[target])); err != nil {
				t.Fatal(err)
			}
			raw := completeFixtures[source]
			if source == Anthropic {
				raw = strings.Replace(raw, `"input_tokens":3`, `"cache_creation_input_tokens":`+value+`,"input_tokens":3`, 1)
			} else {
				raw = strings.Replace(raw, `"input_tokens":3`, `"input_tokens_details":{"cache_write_tokens":`+value+`},"input_tokens":3`, 1)
			}
			result, err := exchange.Response(context.Background(), []byte(raw))
			if err != nil {
				t.Fatal(err)
			}
			object, err := rawObject(result.Body, responseLimit)
			if err != nil {
				t.Fatal(err)
			}
			var observed cachePresence
			if err = observed.observe(target, result.Body); err != nil {
				t.Fatal(err)
			}
			if string(observed.write) != value {
				t.Fatalf("cache write presence lost: %s", result.Body)
			}
			usage, _, _, _ := usageObject(target, object)
			if target == OpenAIResponses && usage["cache_write_input_tokens"] != nil {
				t.Fatal("non-native flat alias emitted")
			}
		}
	}
}
func TestMalformedHistoricalArgumentsAreRejectedInsteadOfCoerced(t *testing.T) {
	for _, arguments := range []string{`"not json"`, `"[]"`, `"null"`, `"{\"x\":1,\"x\":2}"`} {
		exchange, _ := NewExchange(OpenAIChat, Anthropic, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
		raw := `{"messages":[{"role":"assistant","content":null,"tool_calls":[{"id":"call","type":"function","function":{"name":"fixture","arguments":` + arguments + `}}]}]}`
		if _, err := exchange.Request(context.Background(), []byte(raw)); err == nil {
			t.Fatal("malformed arguments were normalized into a new invocation")
		}
	}
}
