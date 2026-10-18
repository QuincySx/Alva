// INPUT: encoding/json, testing
// OUTPUT: Invalid UTF-8 rejection before JSON byte normalization regressions
// POS: [L0 foundation] Invalid arguments/provider continuation/provenance cannot silently change bytes before replay or execution.
package wire

import (
	"encoding/json"
	"testing"
)

func TestWireRejectsInvalidUTF8RawJSONAndText(t *testing.T) {
	bad := json.RawMessage{'"', 0xff, '"'}
	if !json.Valid(bad) {
		t.Fatal("fixture must demonstrate encoding/json replacement tolerance")
	}
	if ValidJSON(bad) {
		t.Fatal("malformed UTF-8 accepted")
	}
	for _, block := range []ContentBlock{{Kind: ContentToolCall, ToolCall: &ToolCall{ID: "c", Name: "tool", Arguments: bad}}, {Kind: ContentProviderState, ProviderState: &ProviderState{Provider: "p", Data: bad}}, {Kind: ContentText, Text: string([]byte{0xff})}, {Kind: ContentImage, Image: &Image{Payload: MediaPayload{MediaType: "image/png", Data: []byte{1}, Provenance: &MediaProvenance{Original: Reference{URI: "session://image"}, Description: "selected original", Operations: []MediaOperation{{Kind: "resize", Description: "actual resize", Parameters: bad}}}}}}} {
		if err := block.Validate(); err == nil {
			t.Fatal("byte-normalizing malformed content accepted", block.Kind)
		}
	}
	if err := (ModelConfig{ExtraBody: append(append([]byte(`{"field":`), bad...), '}')}).Validate(); err == nil {
		t.Fatal("malformed extra body accepted")
	}
}

func TestWireJSONRejectsUnpairedSurrogatesWithoutRejectingLiteralEscapes(t *testing.T) {
	for _, raw := range []string{`"\ud800"`, `"\udfff"`, `{"path":"\ud800x"}`} {
		if !json.Valid([]byte(raw)) || ValidJSON([]byte(raw)) {
			t.Fatal("JSON replacement would alter input", raw)
		}
	}
	for _, raw := range []string{`"\ud83d\ude00"`, `"\\ud800"`, `{"path":"hello世界"}`} {
		if !ValidJSON([]byte(raw)) {
			t.Fatal("valid Unicode or literal backslash rejected", raw)
		}
	}
}
