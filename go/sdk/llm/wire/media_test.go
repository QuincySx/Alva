// INPUT: encoding/json, testing
// OUTPUT: Multimodal explicit delivery, provenance, union and ownership regressions
// POS: [L0 foundation] Prevents stored references from silently becoming raw media or processing claims.
package wire

import (
	"encoding/json"
	"testing"
)

func TestMediaExplicitDeliveryAndProvenanceClone(t *testing.T) {
	payload := MediaPayload{MediaType: "image/png", Data: []byte{1, 2, 3}, Provenance: &MediaProvenance{Original: Reference{URI: "session://image", SHA256: "original", Bytes: 20}, Description: "Selected resized image only", Operations: []MediaOperation{{Kind: "resize", Description: "Resized to 32 by 32", Parameters: json.RawMessage(`{"width":32}`), FrameTimesMillis: []int64{100}}}}}
	image := ContentBlock{Kind: ContentImage, Image: &Image{Payload: payload, Width: 32, Height: 32}}
	if err := image.Validate(); err != nil {
		t.Fatal(err)
	}
	clone := CloneContent([]ContentBlock{image})
	clone[0].Image.Payload.Data[0] = 9
	clone[0].Image.Payload.Provenance.Operations[0].Parameters[0] = 'x'
	clone[0].Image.Payload.Provenance.Operations[0].FrameTimesMillis[0] = 500
	if image.Image.Payload.Data[0] != 1 || image.Image.Payload.Provenance.Operations[0].Parameters[0] != '{' || image.Image.Payload.Provenance.Operations[0].FrameTimesMillis[0] != 100 {
		t.Fatal("shared media backing")
	}
	accumulator := NewStreamAccumulator(4096)
	if err := accumulator.Push(StreamChunk{Kind: ChunkContentBlock, ContentBlock: &image}); err != nil {
		t.Fatal(err)
	}
	if err := accumulator.Push(StreamChunk{Kind: ChunkFinish}); err != nil {
		t.Fatal(err)
	}
	message, err := accumulator.Finish()
	if err != nil || len(message.Content) != 1 || message.Content[0].Image == nil || message.Text() != "" {
		t.Fatal(message, err)
	}
}
func TestMediaRejectsAmbiguousSourcesAndBadFrameClaims(t *testing.T) {
	for _, payload := range []MediaPayload{{MediaType: "image/png"}, {MediaType: "image/png", Data: []byte{1}, URL: "https://example.test/x"}, {MediaType: "image/png", URL: "file:///private/image"}, {MediaType: "image/png", Data: []byte{1}, Provenance: &MediaProvenance{Original: Reference{URI: "session://x"}}}} {
		if err := payload.Validate(); err == nil {
			t.Fatal("invalid source/provenance accepted")
		}
	}
	frame := VideoFrame{TimestampMillis: 100, Image: Image{Payload: MediaPayload{MediaType: "image/png", Data: []byte{1}}}}
	video := Video{DurationMillis: 200, Frames: []VideoFrame{frame}}
	if err := video.Validate(); err != nil {
		t.Fatal(err)
	}
	video.Frames = append(video.Frames, VideoFrame{TimestampMillis: 10, Image: frame.Image})
	if err := video.Validate(); err == nil {
		t.Fatal("out-of-order frames")
	}
	video = Video{Payload: &MediaPayload{MediaType: "video/mp4", URL: "https://example.test/video"}, Frames: []VideoFrame{frame}}
	if err := video.Validate(); err == nil {
		t.Fatal("ambiguous original video and frame delivery")
	}
}
