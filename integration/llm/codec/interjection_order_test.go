// INPUT: Every protocol encoder and a conversation whose last tool result is followed by an interjected user message
// OUTPUT: Regression that Chat, Responses, Anthropic and Gemini requests keep the interjection after the tool result, in order, without loss or rejection
// POS: [integration / protocol codec] Alva appends the interjected user message after a tool result at the next model boundary; no protocol may drop, reorder or refuse it
package codec

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func interjectedConversation() []wire.Message {
	call := wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &wire.ToolCall{ID: "call-1", Name: "read_file", Arguments: json.RawMessage(`{"path":"a"}`)}}}}
	result := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "call-1", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "TOOL-RESULT-MARK"}}}}}}
	interjection := wire.TextMessage(wire.RoleUser, "INTERJECTION-MARK")
	interjection.ID = "interject:input-1"
	return []wire.Message{wire.TextMessage(wire.RoleSystem, "system"), wire.TextMessage(wire.RoleUser, "PROMPT-MARK"), call, result, interjection}
}

func TestEveryProtocolKeepsAnInterjectionAfterTheToolResult(t *testing.T) {
	for _, kind := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(kind), func(t *testing.T) {
			config := Request{Model: "gpt-5.6", MaxOutputTokens: 100}
			if kind == Anthropic {
				config.Model = "claude-sonnet-4-5"
			}
			if kind == Gemini {
				config.Model = "gemini-2.5-flash"
			}
			request := Request{Messages: interjectedConversation()}
			var encoded []byte
			if kind == OpenAIChat {
				data, err := encodeChatFixture(config, request)
				if err != nil {
					t.Fatal("the request with an interjection must encode:", err)
				}
				encoded = data
			} else {
				body, err := encodeNativeFixture(kind, config, request)
				if err != nil {
					t.Fatal("the request with an interjection must encode:", err)
				}
				data, err := json.Marshal(body)
				if err != nil {
					t.Fatal(err)
				}
				encoded = data
			}
			text := string(encoded)
			prompt, result, interjection := strings.Index(text, "PROMPT-MARK"), strings.Index(text, "TOOL-RESULT-MARK"), strings.Index(text, "INTERJECTION-MARK")
			if prompt < 0 || result < 0 || interjection < 0 {
				t.Fatal("a message was dropped", prompt, result, interjection)
			}
			if !(prompt < result && result < interjection) {
				t.Fatal("order must be prompt, tool result, interjection", prompt, result, interjection)
			}
			if strings.Count(text, "INTERJECTION-MARK") != 1 {
				t.Fatal("the interjection must appear exactly once")
			}
		})
	}
}
