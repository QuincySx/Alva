// INPUT: explicit wire payloads/provenance and metadata-only references
// OUTPUT: Selected-media encoding, frame timestamp and no implicit delivery regressions
// POS: [integration / protocol codec] Validate model delivery without opening files, fetching URLs or transforming bytes
package codec

import (
	"encoding/base64"
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestSelectedImageAndReferenceAreDistinctAcrossProviders(t *testing.T) {
	image := wire.Image{Payload: wire.MediaPayload{MediaType: "image/png", Data: []byte("exact fixture bytes"), Provenance: &wire.MediaProvenance{Original: wire.Reference{URI: "session://original", Bytes: 40, SHA256: "original-hash"}, Description: "Explicitly resized by product before this request", Operations: []wire.MediaOperation{{Kind: "resize", Description: "800px longest edge", InputSHA256: "original-hash", OutputSHA256: "derived-hash"}}}}}
	for _, kind := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(kind), func(t *testing.T) {
			request := Request{Messages: []wire.Message{{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &wire.Reference{URI: "session://not-selected", MediaType: "image/png", Bytes: 100, Processing: "Not processed; not delivered"}}, {Kind: wire.ContentImage, Image: &image}}}}}
			var encoded []byte
			var err error
			if kind == OpenAIChat {
				encoded, err = encodeChatFixture(Request{Model: "fixture"}, request)
			} else {
				var payload map[string]any
				payload, err = encodeNativeFixture(kind, Request{Model: "fixture"}, request)
				encoded, _ = json.Marshal(payload)
			}
			if err != nil {
				t.Fatal(err)
			}
			value := string(encoded)
			for _, required := range []string{"raw bytes not delivered", "not-selected", "800px longest edge", "derived-hash", base64.StdEncoding.EncodeToString(image.Payload.Data)} {
				if !strings.Contains(value, required) {
					t.Errorf("lost delivery fact %q", required)
				}
			}
			if strings.Count(value, base64.StdEncoding.EncodeToString(image.Payload.Data)) != 1 {
				t.Fatal("payload duplicated")
			}
		})
	}
}
func TestSelectedVideoFramesRetainExactTimesAndUnsupportedMediaErrors(t *testing.T) {
	frame := wire.Image{Payload: wire.MediaPayload{MediaType: "image/jpeg", Data: []byte("frame")}}
	video := wire.ContentBlock{Kind: wire.ContentVideo, Video: &wire.Video{DurationMillis: 10000, Frames: []wire.VideoFrame{{TimestampMillis: 1234, Image: frame}, {TimestampMillis: 8765, Image: frame}}}}
	for _, kind := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		parts, err := encodeMedia(kind, video)
		if err != nil {
			t.Fatal(err)
		}
		data, _ := json.Marshal(parts)
		if !strings.Contains(string(data), "Frame at 1234 ms") || !strings.Contains(string(data), "Frame at 8765 ms") || !strings.Contains(string(data), "not full video") {
			t.Fatal("lost precise extraction facts")
		}
		raw := wire.ContentBlock{Kind: wire.ContentVideo, Video: &wire.Video{Payload: &wire.MediaPayload{MediaType: "video/mp4", Data: []byte("raw video")}}}
		_, err = encodeMedia(kind, raw)
		if (kind == Gemini) != (err == nil) {
			t.Fatalf("raw video protocol support wrong %s: %v", kind, err)
		}
	}
}
func TestToolResultUnsupportedContentNeverSilentlyDisappears(t *testing.T) {
	request := Request{Messages: []wire.Message{{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "call", Content: []wire.ContentBlock{{Kind: wire.ContentImage, Image: &wire.Image{Payload: wire.MediaPayload{MediaType: "image/png", Data: []byte("fixture")}}}}}}}}}}
	if _, err := encodeChatFixture(Request{Model: "fixture"}, request); err == nil {
		t.Fatal("unsupported result media silently ignored")
	}
}
