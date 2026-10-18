// INPUT: AEP type-tagged author SDK blocks and portable Go value fixtures
// OUTPUT: Canonical protocol translation and rich content round-trip/no-alias regressions
// POS: [integration / external protocol] Prevents empty text, lost tool identities or implicit historical format interpretation
package aep

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestAuthorSDKMessageShapeIsTranslatedNotInternalWire(t *testing.T) {
	var dto MessageDTO
	if json.Unmarshal([]byte(`{"id":"python-message","role":"system","content":[{"type":"text","text":"SDK-visible text"}],"timestamp":123}`), &dto) != nil {
		t.Fatal("invalid fixture")
	}
	message, err := MessageFromDTO(dto)
	if err != nil || message.Text() != "SDK-visible text" {
		t.Fatal("real SDK message became empty", err)
	}
	value, err := MessageToDTO(message)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(value)
	if !strings.Contains(string(data), `"type":"text"`) || strings.Contains(string(data), `"kind"`) {
		t.Fatal("internal wire leaked into protocol")
	}
	if _, err := MessageFromDTO(MessageDTO{Role: wire.RoleAssistant, Content: []ContentDTO{{Type: "text", Text: "x", Input: json.RawMessage(`{}`)}}}); err == nil {
		t.Fatal("mixed payload silently discarded")
	}
	if _, err := MessageFromDTO(MessageDTO{Role: wire.RoleAssistant, Content: []ContentDTO{{Type: "tool_call", ID: "x", Name: "y", Input: json.RawMessage(`{}`)}}}); err == nil {
		t.Fatal("internal alias auto-detected as external AEP")
	}
}

func TestRichContentRoundTripPreservesSelectedBytesAndOpaqueState(t *testing.T) {
	call := &wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{"x":1}`), ProviderState: []wire.ProviderState{{Provider: "gemini", ToolCallID: "call", Data: json.RawMessage(`{"thoughtSignature":"opaque"}`)}}}
	result := &wire.ToolResult{CallID: "prior", IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "failure"}, {Kind: wire.ContentReference, Reference: &wire.Reference{URI: "alva-artifact://example", Processing: "metadata only"}}}}
	message := wire.Message{ID: "identity", Role: wire.RoleAssistant, Content: []wire.ContentBlock{
		{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: "thought", Signature: "signed"}},
		{Kind: wire.ContentProviderState, ProviderState: &wire.ProviderState{Provider: "openai_responses", Data: json.RawMessage(`{"encrypted_content":"opaque"}`)}},
		{Kind: wire.ContentRefusal, Refusal: &wire.Refusal{Text: "refused"}},
		{Kind: wire.ContentToolCall, ToolCall: call}, {Kind: wire.ContentToolResult, ToolResult: result},
		{Kind: wire.ContentImage, Image: &wire.Image{Payload: wire.MediaPayload{MediaType: "image/png", Data: []byte{1, 2, 3}}, Detail: "low", Width: 2, Height: 3}},
	}}
	dto, err := MessageToDTO(message)
	if err != nil {
		t.Fatal(err)
	}
	rebuilt, err := MessageFromDTO(dto)
	if err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(rebuilt, message) {
		t.Fatalf("protocol content lost: %#v", rebuilt)
	}
	call.Arguments[2] = 'z'
	if string(rebuilt.Content[3].ToolCall.Arguments) != `{"x":1}` {
		t.Fatal("inbound DTO result borrowed caller bytes")
	}
}
