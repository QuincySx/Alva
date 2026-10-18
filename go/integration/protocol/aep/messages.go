// INPUT: External AEP v0.1 message/action shapes and internal portable wire values
// OUTPUT: MessageDTO, ContentDTO, MessageToDTO, MessageFromDTO and ToolResultToDTO
// POS: [integration / external protocol] Actual protocol translation, not old config/data/API compatibility aliases
package aep

import (
	"encoding/base64"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

// AEP's Python author SDK uses type-tagged protocol blocks. Internal Go wire
// values use kind and nested payloads; exposing them directly would make the
// real SDK silently see empty text. This boundary has one canonical shape, not
// fallback parsing of both historical formats.
type MessageDTO struct {
	ID           string       `json:"id,omitempty"`
	Role         wire.Role    `json:"role"`
	Content      []ContentDTO `json:"content"`
	Usage        *wire.Usage  `json:"usage,omitempty"`
	ResponseID   string       `json:"response_id,omitempty"`
	FinishReason string       `json:"finish_reason,omitempty"`
	// A plugin-authored timestamp remains in the full I/O journal. Native fact
	// time is store-owned; this field never overrides it.
	Timestamp *int64 `json:"timestamp,omitempty"`
}
type ToolResultDTO struct {
	Content []ContentDTO `json:"content"`
	IsError bool         `json:"is_error"`
}
type ContentDTO struct {
	Type          string                `json:"type"`
	Text          string                `json:"text,omitempty"`
	ID            string                `json:"id,omitempty"`
	Name          string                `json:"name,omitempty"`
	Input         json.RawMessage       `json:"input,omitempty"`
	Content       []ContentDTO          `json:"content,omitempty"`
	IsError       bool                  `json:"is_error,omitempty"`
	Data          json.RawMessage       `json:"data,omitempty"`
	MediaType     string                `json:"media_type,omitempty"`
	URL           string                `json:"url,omitempty"`
	Detail        string                `json:"detail,omitempty"`
	Width         int                   `json:"width,omitempty"`
	Height        int                   `json:"height,omitempty"`
	Provenance    *wire.MediaProvenance `json:"provenance,omitempty"`
	Signature     string                `json:"signature,omitempty"`
	RedactedData  string                `json:"redacted_data,omitempty"`
	Provider      string                `json:"provider,omitempty"`
	ToolCallID    string                `json:"tool_call_id,omitempty"`
	ProviderState []wire.ProviderState  `json:"provider_state,omitempty"`
	Reference     *wire.Reference       `json:"reference,omitempty"`
	Audio         *wire.Audio           `json:"audio,omitempty"`
	Video         *wire.Video           `json:"video,omitempty"`
}

func MessageToDTO(m wire.Message) (MessageDTO, error) {
	if err := m.Validate(); err != nil {
		return MessageDTO{}, err
	}
	content, err := blocksToDTO(m.Content)
	if err != nil {
		return MessageDTO{}, err
	}
	return MessageDTO{ID: m.ID, Role: m.Role, Content: content, Usage: m.Usage, ResponseID: m.ResponseID, FinishReason: m.FinishReason}, nil
}
func MessageFromDTO(m MessageDTO) (wire.Message, error) {
	content, err := blocksFromDTO(m.Content)
	if err != nil {
		return wire.Message{}, err
	}
	message := wire.Message{ID: m.ID, Role: m.Role, Content: content, Usage: m.Usage, ResponseID: m.ResponseID, FinishReason: m.FinishReason}
	if err = message.Validate(); err != nil {
		return wire.Message{}, err
	}
	return wire.CloneMessages([]wire.Message{message})[0], nil
}
func ToolResultToDTO(result wire.ToolResult) (ToolResultDTO, error) {
	if err := (wire.ContentBlock{Kind: wire.ContentToolResult, ToolResult: &result}).Validate(); err != nil {
		return ToolResultDTO{}, err
	}
	content, err := blocksToDTO(result.Content)
	return ToolResultDTO{Content: content, IsError: result.IsError}, err
}
func blocksToDTO(blocks []wire.ContentBlock) ([]ContentDTO, error) {
	result := make([]ContentDTO, 0, len(blocks))
	for _, block := range blocks {
		value := ContentDTO{Type: string(block.Kind)}
		switch block.Kind {
		case wire.ContentText:
			value.Text = block.Text
		case wire.ContentToolCall:
			value.Type = "tool_use"
			value.ID = block.ToolCall.ID
			value.Name = block.ToolCall.Name
			value.Input = block.ToolCall.Arguments
			value.ProviderState = block.ToolCall.ProviderState
		case wire.ContentToolResult:
			var err error
			value.ID = block.ToolResult.CallID
			value.IsError = block.ToolResult.IsError
			value.Content, err = blocksToDTO(block.ToolResult.Content)
			if err != nil {
				return nil, err
			}
		case wire.ContentReasoning:
			value.Text = block.Reasoning.Text
			value.Signature = block.Reasoning.Signature
			value.RedactedData = block.Reasoning.RedactedData
		case wire.ContentProviderState:
			value.Provider = block.ProviderState.Provider
			value.ToolCallID = block.ProviderState.ToolCallID
			value.Data = block.ProviderState.Data
		case wire.ContentRefusal:
			value.Text = block.Refusal.Text
		case wire.ContentReference:
			value.Reference = block.Reference
		case wire.ContentImage:
			image := block.Image
			value.MediaType = image.Payload.MediaType
			value.URL = image.Payload.URL
			value.Detail = image.Detail
			value.Width = image.Width
			value.Height = image.Height
			value.Provenance = image.Payload.Provenance
			if image.Payload.Data != nil {
				value.Data, _ = json.Marshal(base64.StdEncoding.EncodeToString(image.Payload.Data))
			}
		case wire.ContentAudio:
			value.Audio = block.Audio
		case wire.ContentVideo:
			value.Video = block.Video
		default:
			return nil, errors.New("unsupported AEP outbound content")
		}
		result = append(result, value)
	}
	return result, nil
}
func blocksFromDTO(blocks []ContentDTO) ([]wire.ContentBlock, error) {
	result := make([]wire.ContentBlock, 0, len(blocks))
	for _, value := range blocks {
		if err := validateContentFields(value); err != nil {
			return nil, err
		}
		block := wire.ContentBlock{Kind: wire.ContentKind(value.Type)}
		switch value.Type {
		case "text":
			block.Text = value.Text
		case "tool_use":
			block.Kind = wire.ContentToolCall
			block.ToolCall = &wire.ToolCall{ID: value.ID, Name: value.Name, Arguments: value.Input, ProviderState: value.ProviderState}
		case "tool_result":
			content, err := blocksFromDTO(value.Content)
			if err != nil {
				return nil, err
			}
			block.ToolResult = &wire.ToolResult{CallID: value.ID, IsError: value.IsError, Content: content}
		case "reasoning":
			block.Reasoning = &wire.Reasoning{Text: value.Text, Signature: value.Signature, RedactedData: value.RedactedData}
		case "provider_state":
			block.ProviderState = &wire.ProviderState{Provider: value.Provider, ToolCallID: value.ToolCallID, Data: value.Data}
		case "refusal":
			block.Refusal = &wire.Refusal{Text: value.Text}
		case "reference":
			block.Reference = value.Reference
		case "image":
			payload := wire.MediaPayload{MediaType: value.MediaType, URL: value.URL, Provenance: value.Provenance}
			if value.Data != nil {
				var encoded string
				if json.Unmarshal(value.Data, &encoded) != nil {
					return nil, errors.New("AEP image data must be base64")
				}
				var err error
				payload.Data, err = base64.StdEncoding.DecodeString(encoded)
				if err != nil {
					return nil, errors.New("AEP image data invalid base64")
				}
			}
			block.Image = &wire.Image{Payload: payload, Detail: value.Detail, Width: value.Width, Height: value.Height}
		case "audio":
			block.Audio = value.Audio
		case "video":
			block.Video = value.Video
		default:
			return nil, errors.New("unsupported AEP content type")
		}
		if err := block.Validate(); err != nil {
			return nil, err
		}
		result = append(result, block)
	}
	return result, nil
}

func validateContentFields(value ContentDTO) error {
	allowed := map[string]bool{"type": true}
	var fields []string
	switch value.Type {
	case "text", "refusal":
		fields = []string{"text"}
	case "tool_use":
		fields = []string{"id", "name", "input", "provider_state"}
	case "tool_result":
		fields = []string{"id", "content", "is_error"}
	case "reasoning":
		fields = []string{"text", "signature", "redacted_data"}
	case "provider_state":
		fields = []string{"provider", "tool_call_id", "data"}
	case "reference":
		fields = []string{"reference"}
	case "image":
		fields = []string{"data", "media_type", "url", "detail", "width", "height", "provenance"}
	case "audio":
		fields = []string{"audio"}
	case "video":
		fields = []string{"video"}
	default:
		return errors.New("unsupported AEP content discriminator")
	}
	for _, field := range fields {
		allowed[field] = true
	}
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	var actual map[string]json.RawMessage
	if json.Unmarshal(data, &actual) != nil {
		return errors.New("invalid AEP content")
	}
	for field := range actual {
		if !allowed[field] {
			return errors.New("AEP block mixed incompatible content payloads")
		}
	}
	return nil
}
