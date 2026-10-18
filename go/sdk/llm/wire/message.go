// INPUT: encoding/json, errors, fmt
// OUTPUT: Message, ContentBlock, ToolCall, ToolResult, StreamChunk, Usage, CloneUsage, TextMessage
// POS: [L0 foundation] Provider-neutral typed model messages and bounded-stream vocabulary without runtime policy.
package wire

import (
	"encoding/json"
	"errors"
	"fmt"
)

type Role string

const (
	RoleSystem    Role = "system"
	RoleUser      Role = "user"
	RoleAssistant Role = "assistant"
	RoleTool      Role = "tool"
)

type ContentKind string

const (
	ContentText          ContentKind = "text"
	ContentToolCall      ContentKind = "tool_call"
	ContentToolResult    ContentKind = "tool_result"
	ContentReference     ContentKind = "reference"
	ContentReasoning     ContentKind = "reasoning"
	ContentProviderState ContentKind = "provider_state"
	ContentRefusal       ContentKind = "refusal"
	ContentImage         ContentKind = "image"
	ContentAudio         ContentKind = "audio"
	ContentVideo         ContentKind = "video"
)

// Reference describes stored data; it never implies loading or delivering its bytes.
type Reference struct {
	URI        string `json:"uri"`
	MediaType  string `json:"media_type,omitempty"`
	SHA256     string `json:"sha256,omitempty"`
	Bytes      int64  `json:"bytes,omitempty"`
	Processing string `json:"processing,omitempty"`
}

type ToolCall struct {
	ID            string          `json:"id"`
	Name          string          `json:"name"`
	Arguments     json.RawMessage `json:"arguments"`
	ProviderState []ProviderState `json:"provider_state,omitempty"`
}

// Reasoning is not answer text. RedactedData is opaque provider data, never decoded.
type Reasoning struct {
	Text         string `json:"text,omitempty"`
	Signature    string `json:"signature,omitempty"`
	RedactedData string `json:"redacted_data,omitempty"`
}

// ProviderState is replayed only by a matching provider; ToolCallID preserves association.
type ProviderState struct {
	Provider   string          `json:"provider"`
	ToolCallID string          `json:"tool_call_id,omitempty"`
	Data       json.RawMessage `json:"data"`
}
type Refusal struct {
	Text string `json:"text"`
}

type ToolResult struct {
	CallID  string         `json:"call_id"`
	Content []ContentBlock `json:"content"`
	IsError bool           `json:"is_error,omitempty"`
}

type ContentBlock struct {
	Kind          ContentKind    `json:"kind"`
	Text          string         `json:"text,omitempty"`
	ToolCall      *ToolCall      `json:"tool_call,omitempty"`
	ToolResult    *ToolResult    `json:"tool_result,omitempty"`
	Reference     *Reference     `json:"reference,omitempty"`
	Reasoning     *Reasoning     `json:"reasoning,omitempty"`
	ProviderState *ProviderState `json:"provider_state,omitempty"`
	Refusal       *Refusal       `json:"refusal,omitempty"`
	Image         *Image         `json:"image,omitempty"`
	Audio         *Audio         `json:"audio,omitempty"`
	Video         *Video         `json:"video,omitempty"`
}

type Message struct {
	ID           string         `json:"id,omitempty"`
	Role         Role           `json:"role"`
	Content      []ContentBlock `json:"content"`
	Usage        *Usage         `json:"usage,omitempty"`
	ResponseID   string         `json:"response_id,omitempty"`
	FinishReason string         `json:"finish_reason,omitempty"`
}

func TextMessage(role Role, text string) Message {
	return Message{Role: role, Content: []ContentBlock{{Kind: ContentText, Text: text}}}
}

func (m Message) Text() string {
	var result string
	for _, block := range m.Content {
		if block.Kind == ContentText {
			result += block.Text
		}
	}
	return result
}

func (m Message) Validate() error {
	if !validStrings(m.ID, m.ResponseID, m.FinishReason) {
		return errors.New("message metadata is not valid UTF-8")
	}
	switch m.Role {
	case RoleSystem, RoleUser, RoleAssistant, RoleTool:
	default:
		return fmt.Errorf("invalid message role %q", m.Role)
	}
	for _, block := range m.Content {
		if err := block.Validate(); err != nil {
			return err
		}
	}
	return nil
}

func (b ContentBlock) Validate() error {
	if !validStrings(b.Text) {
		return errors.New("content text is not valid UTF-8")
	}
	payloads := 0
	if b.ToolCall != nil {
		payloads++
	}
	if b.ToolResult != nil {
		payloads++
	}
	if b.Reference != nil {
		payloads++
	}
	if b.Reasoning != nil {
		payloads++
	}
	if b.ProviderState != nil {
		payloads++
	}
	if b.Refusal != nil {
		payloads++
	}
	if b.Image != nil {
		payloads++
	}
	if b.Audio != nil {
		payloads++
	}
	if b.Video != nil {
		payloads++
	}
	switch b.Kind {
	case ContentText:
		if payloads != 0 {
			return errors.New("text block contains a non-text payload")
		}
	case ContentToolCall:
		if payloads != 1 || b.ToolCall == nil || b.Text != "" || b.ToolCall.ID == "" || b.ToolCall.Name == "" || !validStrings(b.ToolCall.ID, b.ToolCall.Name) || !ValidJSON(b.ToolCall.Arguments) {
			return errors.New("invalid tool call block")
		}
		for _, state := range b.ToolCall.ProviderState {
			if err := state.Validate(); err != nil {
				return err
			}
		}
	case ContentToolResult:
		if payloads != 1 || b.ToolResult == nil || b.Text != "" || b.ToolResult.CallID == "" || !validStrings(b.ToolResult.CallID) {
			return errors.New("invalid tool result block")
		}
		for _, child := range b.ToolResult.Content {
			if child.Kind == ContentToolCall || child.Kind == ContentToolResult {
				return errors.New("nested tool protocol blocks are not allowed")
			}
			if err := child.Validate(); err != nil {
				return err
			}
		}
	case ContentReference:
		if payloads != 1 || b.Reference == nil || b.Text != "" || b.Reference.URI == "" || b.Reference.Bytes < 0 {
			return errors.New("invalid reference block")
		}
		return b.Reference.Validate()
	case ContentReasoning:
		if payloads != 1 || b.Reasoning == nil || b.Text != "" || (b.Reasoning.RedactedData != "" && (b.Reasoning.Text != "" || b.Reasoning.Signature != "")) {
			return errors.New("invalid reasoning block")
		}
		if !validStrings(b.Reasoning.Text, b.Reasoning.Signature, b.Reasoning.RedactedData) {
			return errors.New("reasoning is not valid UTF-8")
		}
	case ContentProviderState:
		if payloads != 1 || b.ProviderState == nil || b.Text != "" {
			return errors.New("invalid provider state block")
		}
		return b.ProviderState.Validate()
	case ContentRefusal:
		if payloads != 1 || b.Refusal == nil || b.Text != "" {
			return errors.New("invalid refusal block")
		}
		if !validStrings(b.Refusal.Text) {
			return errors.New("refusal is not valid UTF-8")
		}
	case ContentImage:
		if payloads != 1 || b.Image == nil || b.Text != "" {
			return errors.New("invalid image block")
		}
		return b.Image.Validate()
	case ContentAudio:
		if payloads != 1 || b.Audio == nil || b.Text != "" {
			return errors.New("invalid audio block")
		}
		return b.Audio.Validate()
	case ContentVideo:
		if payloads != 1 || b.Video == nil || b.Text != "" {
			return errors.New("invalid video block")
		}
		return b.Video.Validate()
	default:
		return fmt.Errorf("invalid content kind %q", b.Kind)
	}
	return nil
}

func (state ProviderState) Validate() error {
	if state.Provider == "" || !validStrings(state.Provider, state.ToolCallID) || !ValidJSON(state.Data) {
		return errors.New("invalid provider state")
	}
	return nil
}
func (reference Reference) Validate() error {
	if reference.URI == "" || reference.Bytes < 0 || !validStrings(reference.URI, reference.MediaType, reference.SHA256, reference.Processing) {
		return errors.New("invalid reference metadata")
	}
	return nil
}
func CloneToolCall(call ToolCall) ToolCall {
	call.Arguments = append(json.RawMessage(nil), call.Arguments...)
	call.ProviderState = append([]ProviderState(nil), call.ProviderState...)
	for i := range call.ProviderState {
		call.ProviderState[i].Data = append(json.RawMessage(nil), call.ProviderState[i].Data...)
	}
	return call
}

// CloneMessages prevents middleware/providers from mutating the durable transcript.
func CloneMessages(messages []Message) []Message {
	if messages == nil {
		return nil
	}
	out := make([]Message, len(messages))
	for i, message := range messages {
		out[i] = message
		out[i].Content = CloneContent(message.Content)
		if message.Usage != nil {
			usage := CloneUsage(*message.Usage)
			out[i].Usage = &usage
		}
	}
	return out
}

func CloneContent(content []ContentBlock) []ContentBlock {
	if content == nil {
		return nil
	}
	out := make([]ContentBlock, len(content))
	for i, block := range content {
		out[i] = block
		if block.ToolCall != nil {
			call := CloneToolCall(*block.ToolCall)
			out[i].ToolCall = &call
		}
		if block.ToolResult != nil {
			result := *block.ToolResult
			result.Content = CloneContent(result.Content)
			out[i].ToolResult = &result
		}
		if block.Reference != nil {
			reference := *block.Reference
			out[i].Reference = &reference
		}
		if block.Reasoning != nil {
			value := *block.Reasoning
			out[i].Reasoning = &value
		}
		if block.Refusal != nil {
			value := *block.Refusal
			out[i].Refusal = &value
		}
		if block.ProviderState != nil {
			value := *block.ProviderState
			value.Data = append(json.RawMessage(nil), value.Data...)
			out[i].ProviderState = &value
		}
		if block.Image != nil {
			value := *block.Image
			value.Payload = cloneMedia(value.Payload)
			out[i].Image = &value
		}
		if block.Audio != nil {
			value := *block.Audio
			value.Payload = cloneMedia(value.Payload)
			out[i].Audio = &value
		}
		if block.Video != nil {
			value := *block.Video
			if value.Payload != nil {
				payload := cloneMedia(*value.Payload)
				value.Payload = &payload
			}
			value.Frames = append([]VideoFrame(nil), value.Frames...)
			for j := range value.Frames {
				value.Frames[j].Image.Payload = cloneMedia(value.Frames[j].Image.Payload)
			}
			out[i].Video = &value
		}
	}
	return out
}

type Usage struct {
	// Normalized inclusive counts: cached/cache-write input remains part of
	// input; reasoning remains part of output, never an additional billable sum.
	InputTokens  uint64 `json:"input_tokens"`
	OutputTokens uint64 `json:"output_tokens"`
	// Cache counters distinguish absent provider reporting from an explicit
	// zero. They must not be inferred from normalized usage that lost presence.
	CachedInputTokens     *uint64 `json:"cached_input_tokens,omitempty"`
	TotalTokens           uint64  `json:"total_tokens,omitempty"`
	CacheWriteInputTokens *uint64 `json:"cache_write_input_tokens,omitempty"`
	ReasoningTokens       uint64  `json:"reasoning_tokens,omitempty"` // A subset of OutputTokens when reported.
}

func CloneUsage(usage Usage) Usage {
	if usage.CachedInputTokens != nil {
		value := *usage.CachedInputTokens
		usage.CachedInputTokens = &value
	}
	if usage.CacheWriteInputTokens != nil {
		value := *usage.CacheWriteInputTokens
		usage.CacheWriteInputTokens = &value
	}
	return usage
}

type ChunkKind string

const (
	ChunkText         ChunkKind = "text"
	ChunkToolCall     ChunkKind = "tool_call"
	ChunkUsage        ChunkKind = "usage"
	ChunkFinish       ChunkKind = "finish"
	ChunkContentBlock ChunkKind = "content_block"
	ChunkReasoning    ChunkKind = "reasoning"
)

// ToolCall chunks contain one complete call; providers own bounded delta assembly.
type StreamChunk struct {
	Kind ChunkKind
	// ContentIndex identifies a canonical block slot, not delta arrival order.
	// One response cannot mix indexed and unindexed authoritative content.
	ContentIndex *int
	Text         string
	ToolCall     *ToolCall
	Usage        *Usage
	FinishReason string
	ContentBlock *ContentBlock
	ResponseID   string
}

func CloneStreamChunk(chunk StreamChunk) StreamChunk {
	if chunk.ContentIndex != nil {
		index := *chunk.ContentIndex
		chunk.ContentIndex = &index
	}
	if chunk.ToolCall != nil {
		value := CloneToolCall(*chunk.ToolCall)
		chunk.ToolCall = &value
	}
	if chunk.ContentBlock != nil {
		value := CloneContent([]ContentBlock{*chunk.ContentBlock})[0]
		chunk.ContentBlock = &value
	}
	if chunk.Usage != nil {
		value := CloneUsage(*chunk.Usage)
		chunk.Usage = &value
	}
	return chunk
}
