// INPUT: encoding/json, errors, fmt, net/url, strings
// OUTPUT: Image, Audio, Video, MediaPayload, MediaProvenance, MediaOperation
// POS: [L0 foundation] Explicit delivered multimodal payloads and truthful transformation provenance; stored references never auto-load.
package wire

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"strings"
)

// MediaPayload is explicit model delivery, not a stored reference. Providers must
// either encode the supported payload or reject it before sending a request.
// URL is an explicit remote provider fetch, never resolved by the wire library.
type MediaPayload struct {
	MediaType  string           `json:"media_type"`
	Data       []byte           `json:"data,omitempty"`
	URL        string           `json:"url,omitempty"`
	Provenance *MediaProvenance `json:"provenance,omitempty"`
}
type MediaProvenance struct {
	Original    Reference        `json:"original"`
	Operations  []MediaOperation `json:"operations,omitempty"`
	Description string           `json:"description"`
}
type MediaOperation struct {
	Kind             string          `json:"kind"`
	Description      string          `json:"description"`
	InputSHA256      string          `json:"input_sha256,omitempty"`
	OutputSHA256     string          `json:"output_sha256,omitempty"`
	FrameTimesMillis []int64         `json:"frame_times_millis,omitempty"`
	Parameters       json.RawMessage `json:"parameters,omitempty"`
}
type Image struct {
	Payload MediaPayload `json:"payload"`
	Detail  string       `json:"detail,omitempty"`
	Width   int          `json:"width,omitempty"`
	Height  int          `json:"height,omitempty"`
}
type Audio struct {
	Payload        MediaPayload `json:"payload"`
	DurationMillis int64        `json:"duration_millis,omitempty"`
	Transcript     string       `json:"transcript,omitempty"`
}
type VideoFrame struct {
	TimestampMillis int64 `json:"timestamp_millis"`
	Image           Image `json:"image"`
}
type Video struct {
	Payload        *MediaPayload `json:"payload,omitempty"`
	DurationMillis int64         `json:"duration_millis,omitempty"`
	Frames         []VideoFrame  `json:"frames,omitempty"`
}

func (payload MediaPayload) Validate() error {
	if !validStrings(payload.MediaType, payload.URL) {
		return errors.New("media metadata is not valid UTF-8")
	}
	if payload.MediaType == "" || (len(payload.Data) == 0) == (payload.URL == "") {
		return errors.New("media requires media type and exactly one explicit bytes or URL delivery source")
	}
	if payload.URL != "" {
		parsed, err := url.Parse(payload.URL)
		if err != nil || parsed.Host == "" || (parsed.Scheme != "http" && parsed.Scheme != "https") {
			return errors.New("media URL must be explicit HTTP(S)")
		}
	}
	if provenance := payload.Provenance; provenance != nil {
		if provenance.Original.Validate() != nil || provenance.Description == "" || !validStrings(provenance.Description) {
			return errors.New("media provenance requires original reference and honest description")
		}
		for _, operation := range provenance.Operations {
			if operation.Kind == "" || operation.Description == "" || !validStrings(operation.Kind, operation.Description, operation.InputSHA256, operation.OutputSHA256) || (len(operation.Parameters) > 0 && !ValidJSON(operation.Parameters)) {
				return errors.New("invalid media operation")
			}
			for _, timestamp := range operation.FrameTimesMillis {
				if timestamp < 0 {
					return errors.New("negative extracted frame time")
				}
			}
		}
	}
	return nil
}
func (image Image) Validate() error {
	if !strings.HasPrefix(image.Payload.MediaType, "image/") {
		return errors.New("image media type required")
	}
	if image.Width < 0 || image.Height < 0 {
		return errors.New("negative image dimensions")
	}
	if image.Detail != "" && image.Detail != "auto" && image.Detail != "low" && image.Detail != "high" && image.Detail != "original" {
		return fmt.Errorf("unknown image detail %q", image.Detail)
	}
	return image.Payload.Validate()
}
func (audio Audio) Validate() error {
	if !validStrings(audio.Transcript) {
		return errors.New("audio transcript is not valid UTF-8")
	}
	if !strings.HasPrefix(audio.Payload.MediaType, "audio/") {
		return errors.New("audio media type required")
	}
	if audio.DurationMillis < 0 {
		return errors.New("negative audio duration")
	}
	return audio.Payload.Validate()
}
func (video Video) Validate() error {
	if video.DurationMillis < 0 {
		return errors.New("negative video duration")
	}
	if (video.Payload == nil) == (len(video.Frames) == 0) {
		return errors.New("video requires exactly one explicit video payload or extracted frames")
	}
	if video.Payload != nil {
		if !strings.HasPrefix(video.Payload.MediaType, "video/") {
			return errors.New("video media type required")
		}
		return video.Payload.Validate()
	}
	previous := int64(-1)
	for _, frame := range video.Frames {
		if frame.TimestampMillis < 0 || frame.TimestampMillis < previous || (video.DurationMillis > 0 && frame.TimestampMillis > video.DurationMillis) {
			return errors.New("video frames must be ordered within source duration")
		}
		if err := frame.Image.Validate(); err != nil {
			return err
		}
		previous = frame.TimestampMillis
	}
	return nil
}
func cloneMedia(payload MediaPayload) MediaPayload {
	payload.Data = append([]byte(nil), payload.Data...)
	if payload.Provenance != nil {
		provenance := *payload.Provenance
		provenance.Operations = append([]MediaOperation(nil), provenance.Operations...)
		for i := range provenance.Operations {
			provenance.Operations[i].FrameTimesMillis = append([]int64(nil), provenance.Operations[i].FrameTimesMillis...)
			provenance.Operations[i].Parameters = append(json.RawMessage(nil), provenance.Operations[i].Parameters...)
		}
		payload.Provenance = &provenance
	}
	return payload
}
