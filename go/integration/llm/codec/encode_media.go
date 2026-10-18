// INPUT: explicitly delivered wire media bytes/URLs and truthful provenance
// OUTPUT: Provider-specific media parts without filesystem reads or implicit uploads
// POS: [integration / protocol codec] Translate selected payloads only; a stored Reference never enters this path
package codec

import (
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func textPart(kind Protocol, text string) map[string]any {
	switch kind {
	case OpenAIResponses:
		return map[string]any{"type": "input_text", "text": text}
	case Gemini:
		return map[string]any{"text": text}
	default:
		return map[string]any{"type": "text", "text": text}
	}
}
func mediaNotice(payload wire.MediaPayload) string {
	notice := map[string]any{"media_type": payload.MediaType, "delivery": "explicit bytes", "processing": "Provider adapter made no transformations; upstream processing is unspecified"}
	if payload.URL != "" {
		notice["delivery"] = "explicit URL for provider fetch; local adapter did not fetch"
	} else {
		sum := sha256.Sum256(payload.Data)
		notice["bytes"] = len(payload.Data)
		notice["sha256"] = hex.EncodeToString(sum[:])
	}
	if payload.Provenance != nil {
		notice["provenance"] = payload.Provenance
		notice["processing"] = "See explicit provenance; provider adapter made no additional transformations"
	}
	encoded, _ := json.Marshal(notice)
	return "[Selected media delivery: " + string(encoded) + "]"
}
func encodeMedia(kind Protocol, block wire.ContentBlock) ([]map[string]any, error) {
	if err := block.Validate(); err != nil {
		return nil, err
	}
	if block.Kind == wire.ContentVideo && block.Video.Payload == nil {
		if len(block.Video.Frames) > 128 {
			return nil, errors.New("selected video frames exceed 128 frame transport limit")
		}
		parts := []map[string]any{textPart(kind, fmt.Sprintf("[Selected video frames only, not full video. Source duration %d ms. Frame timestamps follow; no additional extraction or compression by provider adapter.]", block.Video.DurationMillis))}
		for _, frame := range block.Video.Frames {
			parts = append(parts, textPart(kind, fmt.Sprintf("[Frame at %d ms]", frame.TimestampMillis)))
			encoded, err := encodeMedia(kind, wire.ContentBlock{Kind: wire.ContentImage, Image: &frame.Image})
			if err != nil {
				return nil, err
			}
			parts = append(parts, encoded...)
		}
		return parts, nil
	}
	var payload wire.MediaPayload
	switch block.Kind {
	case wire.ContentImage:
		payload = block.Image.Payload
	case wire.ContentAudio:
		payload = block.Audio.Payload
	case wire.ContentVideo:
		payload = *block.Video.Payload
	default:
		return nil, errors.New("not a media block")
	}
	if len(payload.Data) > 20<<20 || len(payload.URL) > 8192 {
		return nil, errors.New("selected media exceeds transport limits")
	}
	parts := []map[string]any{textPart(kind, mediaNotice(payload))}
	var media map[string]any
	switch kind {
	case OpenAIChat, OpenAIResponses:
		if block.Kind != wire.ContentImage {
			return nil, errors.New("this OpenAI transport supports selected images or extracted video frames, not raw audio/video")
		}
		location := payload.URL
		if location == "" {
			location = "data:" + payload.MediaType + ";base64," + base64.StdEncoding.EncodeToString(payload.Data)
		}
		detail := block.Image.Detail
		if detail == "" {
			detail = "auto"
		}
		if kind == OpenAIChat {
			media = map[string]any{"type": "image_url", "image_url": map[string]any{"url": location, "detail": detail}}
		} else {
			media = map[string]any{"type": "input_image", "image_url": location, "detail": detail}
		}
	case Anthropic:
		if block.Kind != wire.ContentImage {
			return nil, errors.New("Anthropic transport supports selected images or extracted frames, not raw audio/video")
		}
		switch payload.MediaType {
		case "image/jpeg", "image/png", "image/gif", "image/webp":
		default:
			return nil, errors.New("unsupported Anthropic image media type")
		}
		var source map[string]any
		if payload.URL != "" {
			source = map[string]any{"type": "url", "url": payload.URL}
		} else {
			source = map[string]any{"type": "base64", "media_type": payload.MediaType, "data": base64.StdEncoding.EncodeToString(payload.Data)}
		}
		media = map[string]any{"type": "image", "source": source}
	case Gemini:
		if payload.URL != "" {
			media = map[string]any{"fileData": map[string]any{"mimeType": payload.MediaType, "fileUri": payload.URL}}
		} else {
			media = map[string]any{"inlineData": map[string]any{"mimeType": payload.MediaType, "data": base64.StdEncoding.EncodeToString(payload.Data)}}
		}
	default:
		return nil, errors.New("unsupported media transport")
	}
	if block.Kind == wire.ContentAudio && block.Audio.Transcript != "" {
		parts = append(parts, textPart(kind, "[Provided transcript, not newly transcribed by provider adapter]\n"+block.Audio.Transcript))
	}
	parts = append(parts, media)
	return parts, nil
}

func collapseChatText(parts []map[string]any) any {
	var text strings.Builder
	for _, part := range parts {
		if part["type"] != "text" {
			return parts
		}
		text.WriteString(part["text"].(string))
	}
	return text.String()
}
