// INPUT: Actual absorbed Magpie custom/namespace/tool-search routes and adversarial native fixtures.
// OUTPUT: Request-local inverse tool mapping, literal text, parallel-call fidelity and explicit-loss regressions.
// POS: Migration acceptance exercises observable protocol behavior through the public codec boundary.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
	"testing"
)

func TestMagpieResponsesClientToolRoundTrips(t *testing.T) {
	cases := []struct{ name, tool, upstream, args, wantType, wantName, wantNamespace string }{
		{"custom", `{"type":"custom","name":"patch","description":"Edit","format":{"type":"text"}}`, "patch", `{"input":"*** Begin Patch\n*** End Patch"}`, "custom_tool_call", "patch", ""},
		{"namespace", `{"type":"namespace","name":"collaboration","tools":[{"type":"function","name":"spawn_agent","parameters":{"type":"object","properties":{"message":{"type":"string"}}}}]}`, "collaboration__spawn_agent", `{"message":"hello"}`, "function_call", "spawn_agent", "collaboration"},
		{"search", `{"type":"tool_search","execution":"client","description":"Find tools","parameters":{"type":"object","properties":{"query":{"type":"string"}}}}`, "tool_search", `{"query":"files"}`, "tool_search_call", "", ""},
	}
	for _, tc := range cases {
		for _, stream := range []bool{false, true} {
			t.Run(tc.name+map[bool]string{true: "/stream", false: "/complete"}[stream], func(t *testing.T) {
				raw := `{"model":"alias","input":"hello","tools":[` + tc.tool + `]}`
				if stream {
					raw = strings.TrimSuffix(raw, "}") + `,"stream":true}`
				}
				e, err := NewExchange(OpenAIResponses, OpenAIChat, Options{Model: "actual", ResponseModel: "alias", Stream: stream})
				if err != nil {
					t.Fatal(err)
				}
				converted, err := e.Request(context.Background(), []byte(raw))
				if err != nil {
					t.Fatal(err)
				}
				if converted.Converter != "magpie" || len(converted.Steps) != 2 || !strings.Contains(string(converted.Body), tc.upstream) {
					t.Fatalf("not the actual Magpie route: %s", converted.Body)
				}
				quoted, _ := json.Marshal(tc.args)
				var response []byte
				if !stream {
					result, err := e.Response(context.Background(), []byte(`{"id":"r","object":"chat.completion","model":"actual","choices":[{"index":0,"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call-actual","type":"function","function":{"name":"`+tc.upstream+`","arguments":`+string(quoted)+`}}]},"finish_reason":"tool_calls"}]}`))
					if err != nil {
						t.Fatal(err)
					}
					response = result.Body
				} else {
					s, err := e.NewResponseStream()
					if err != nil {
						t.Fatal(err)
					}
					emit := func(ev Event) error {
						var obj struct {
							Type     string
							Response json.RawMessage
						}
						if json.Unmarshal(ev.Data, &obj) == nil && obj.Type == "response.completed" {
							response = append([]byte(nil), obj.Response...)
						}
						return nil
					}
					for _, frame := range []string{`{"id":"r","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call-actual","type":"function","function":{"name":"` + tc.upstream + `","arguments":` + string(quoted) + `}}]}}]}`, `{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}`, `[DONE]`} {
						if err = s.Push(context.Background(), Event{Data: []byte(frame)}, emit); err != nil {
							t.Fatal(err)
						}
					}
					if err = s.Finish(context.Background(), emit); err != nil {
						t.Fatal(err)
					}
				}
				var body map[string]json.RawMessage
				if json.Unmarshal(response, &body) != nil {
					t.Fatalf("bad response %s", response)
				}
				var output []map[string]json.RawMessage
				_ = json.Unmarshal(body["output"], &output)
				if len(output) != 1 {
					t.Fatalf("output %s", response)
				}
				item := output[0]
				get := func(key string) string { var s string; _ = json.Unmarshal(item[key], &s); return s }
				if get("type") != tc.wantType || get("call_id") != "call-actual" || get("name") != tc.wantName || get("namespace") != tc.wantNamespace {
					t.Fatalf("inverse tool metadata lost: %s", response)
				}
				if tc.name == "custom" && get("input") != "*** Begin Patch\n*** End Patch" {
					t.Fatalf("custom input changed %s", response)
				}
				if tc.name == "search" && get("execution") != "client" {
					t.Fatalf("search execution changed %s", response)
				}
			})
		}
	}
}

func TestMagpieNeverReinterpretsLiteralModelText(t *testing.T) {
	for _, tc := range []struct {
		source, target Protocol
		text           string
	}{{OpenAIChat, Anthropic, "<think>literal example</think>"}, {Gemini, OpenAIChat, `call:default_api:remove_file{path:"example"}`}} {
		e, _ := NewExchange(tc.target, tc.source, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
		if _, err := e.Request(context.Background(), []byte(requestFixtures[tc.target])); err != nil {
			t.Fatal(err)
		}
		raw := strings.Replace(completeFixtures[tc.source], "reply", strings.Trim(string(mustJSON(t, tc.text)), `"`), 1)
		result, err := e.Response(context.Background(), []byte(raw))
		if err != nil {
			t.Fatal(err)
		}
		var decoded any
		if json.Unmarshal(result.Body, &decoded) != nil {
			t.Fatal("invalid converted reply")
		}
		if strings.Contains(string(result.Body), `"tool_calls":[{`) || strings.Contains(string(result.Body), `"type":"tool_use"`) {
			t.Fatalf("prose became execution %s", result.Body)
		}
		if !strings.Contains(string(result.Body), "literal example") && !strings.Contains(string(result.Body), "default_api") {
			t.Fatalf("literal text lost %s", result.Body)
		}
	}
}
func mustJSON(t *testing.T, v any) []byte {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestMagpieRejectsUnrepresentableRequestSemantics(t *testing.T) {
	cases := []struct {
		source, target Protocol
		raw            string
	}{
		{OpenAIResponses, OpenAIChat, `{"input":"hi","tools":[{"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":"start: \"a\""}}]}`},
		{OpenAIResponses, OpenAIChat, `{"input":"hi","tools":[{"type":"function","name":"spawn","parameters":{"type":"object","properties":{"message":{"type":"string","encrypted":true}}}}]}`},
		{OpenAIResponses, OpenAIChat, `{"input":"hello","tools":[{"type":"computer_use_preview"}]}`},
		{OpenAIResponses, OpenAIChat, `{"input":[{"type":"message","role":"user","content":[{"type":"input_file","file_id":"file-private"}]}]}`},
		{OpenAIChat, Anthropic, `{"messages":[{"role":"user","content":[{"type":"input_audio","input_audio":{"data":"AA==","format":"wav"}}]}]}`},
		{Anthropic, OpenAIChat, `{"messages":[{"role":"user","content":"hi"}],"max_tokens":4096,"thinking":{"type":"enabled","budget_tokens":1049}}`},
		{OpenAIChat, Anthropic, `{"messages":[{"role":"user","content":"hi"}],"tools":[{"type":"function","function":{"name":"choice","strict":true,"parameters":{"type":"object"}}}]}`},
		{OpenAIChat, Anthropic, `{"messages":[{"role":"user","content":"hi"}],"seed":42}`},
	}
	for _, tc := range cases {
		e, _ := NewExchange(tc.source, tc.target, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
		_, err := e.Request(context.Background(), []byte(tc.raw))
		var loss *LossError
		if !errors.As(err, &loss) {
			t.Errorf("silently accepted loss %s -> %s: %s (%v)", tc.source, tc.target, tc.raw, err)
		}
	}
}

func TestMagpieInterleavedCallsRemainOrderedAndExact(t *testing.T) {
	for _, target := range []Protocol{OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(target), func(t *testing.T) {
			stream := newStreamFixture(t, target, OpenAIChat)
			chunks := []string{
				`{"id":"r","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"first","type":"function","function":{"name":"alpha","arguments":"{\"value\":"}}]}}]}`,
				`{"id":"r","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"second","type":"function","function":{"name":"beta","arguments":"{\"value\":"}}]}}]}`,
				`{"id":"r","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"11}"}}]}}]}`,
				`{"id":"r","choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"function":{"arguments":"22}"}}]}}]}`,
				`{"id":"r","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}`,
				`[DONE]`,
			}
			decoder, err := NewDecoder(target, Limits{})
			if err != nil {
				t.Fatal(err)
			}
			var calls []string
			consume := func(chunk wire.StreamChunk) error {
				if chunk.ToolCall != nil {
					calls = append(calls, chunk.ToolCall.ID+"/"+chunk.ToolCall.Name+"/"+string(chunk.ToolCall.Arguments))
				}
				return nil
			}
			emit := func(event Event) error { return decoder.Push(context.Background(), event, consume) }
			for _, raw := range chunks {
				if err = stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
					t.Fatal(err)
				}
			}
			if err = stream.Finish(context.Background(), emit); err != nil {
				t.Fatal(err)
			}
			if err = decoder.Finish(context.Background(), consume); err != nil {
				t.Fatal(err)
			}
			if len(calls) != 2 || calls[0] != `first/alpha/{"value":11}` || calls[1] != `second/beta/{"value":22}` {
				t.Fatalf("parallel calls changed: %#v", calls)
			}
		})
	}
}

func TestMagpieToolStateIsRequestLocal(t *testing.T) {
	for _, custom := range []bool{true, false, true} {
		tool := `{"type":"function","name":"patch","parameters":{"type":"object","properties":{"input":{"type":"string"}}}}`
		if custom {
			tool = `{"type":"custom","name":"patch","format":{"type":"text"}}`
		}
		e, _ := NewExchange(OpenAIResponses, OpenAIChat, Options{Model: "actual"})
		if _, err := e.Request(context.Background(), []byte(`{"input":"hi","tools":[`+tool+`]}`)); err != nil {
			t.Fatal(err)
		}
		result, err := e.Response(context.Background(), []byte(`{"id":"r","choices":[{"index":0,"message":{"role":"assistant","tool_calls":[{"id":"same","type":"function","function":{"name":"patch","arguments":"{\"input\":\"text\"}"}}]},"finish_reason":"tool_calls"}]}`))
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(result.Body), `"type":"custom_tool_call"`) != custom {
			t.Fatalf("another request changed tool type %s", result.Body)
		}
	}
	e, _ := NewExchange(OpenAIResponses, OpenAIChat, Options{Model: "actual"})
	_, err := e.Request(context.Background(), []byte(`{"input":"hi","tools":[{"type":"namespace","name":"team","tools":[{"type":"function","name":"run","parameters":{"type":"object"}}]},{"type":"function","name":"team__run","parameters":{"type":"object"}}]}`))
	var loss *LossError
	if !errors.As(err, &loss) {
		t.Fatal("flattened tool-name collision accepted", err)
	}
}

func TestMagpiePreservesGeminiJSONSchemaConstraints(t *testing.T) {
	e, _ := NewExchange(OpenAIChat, Gemini, Options{Model: "actual"})
	result, err := e.Request(context.Background(), []byte(`{"messages":[{"role":"user","content":"hi"}],"tools":[{"type":"function","function":{"name":"choice","parameters":{"type":"object","properties":{"x":{"type":"number","exclusiveMinimum":1}}}}}]}`))
	if err != nil || !strings.Contains(string(result.Body), `"exclusiveMinimum":1`) {
		t.Fatalf("schema weakened %s %v", result.Body, err)
	}
}

func TestMagpieCompleteBodyDoesNotInheritSSEEventLimit(t *testing.T) {
	text := strings.Repeat("large reply ", 110000)
	for _, source := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(source), func(t *testing.T) {
			target := OpenAIChat
			if source == target {
				target = Anthropic
			}
			e, _ := NewExchange(target, source, Options{Model: "actual", DefaultMaxOutputTokens: 1024, Limits: Limits{MaxEventBytes: 1024}})
			if _, err := e.Request(context.Background(), []byte(requestFixtures[target])); err != nil {
				t.Fatal(err)
			}
			raw := strings.Replace(completeFixtures[source], "reply", text, 1)
			result, err := e.Response(context.Background(), []byte(raw))
			if err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(string(result.Body), text) {
				t.Fatal("large complete response was truncated")
			}
		})
	}
}

func TestMagpieSchemaLiteralEncryptedKeyIsNotAnAnnotation(t *testing.T) {
	e, _ := NewExchange(OpenAIResponses, OpenAIChat, Options{Model: "actual"})
	result, err := e.Request(context.Background(), []byte(`{"input":"hi","tools":[{"type":"function","name":"literal","parameters":{"type":"object","properties":{"value":{"const":{"encrypted":true,"data":"literal"}}}}}]}`))
	if err != nil || !strings.Contains(string(result.Body), `"encrypted":true`) {
		t.Fatal("literal business schema value changed", err)
	}
}

func TestMagpieGeminiHistoryWithoutCallIDsBindsOneRequest(t *testing.T) {
	for _, target := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic} {
		t.Run(string(target), func(t *testing.T) {
			e, _ := NewExchange(Gemini, target, Options{Model: "actual", DefaultMaxOutputTokens: 1024})
			result, err := e.Request(context.Background(), []byte(`{"contents":[{"role":"user","parts":[{"text":"read"}]},{"role":"model","parts":[{"functionCall":{"name":"lookup","args":{"key":"value"}}}]},{"role":"user","parts":[{"functionResponse":{"name":"lookup","response":{"result":"found"}}}]}]}`))
			if err != nil {
				t.Fatal(err)
			}
			if !strings.Contains(string(result.Body), "lookup") || !strings.Contains(string(result.Body), "found") {
				t.Fatal("Gemini history lost call/result binding")
			}
		})
	}
}
