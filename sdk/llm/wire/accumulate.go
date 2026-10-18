// INPUT: encoding/json, errors, fmt, sort, llm/wire message types
// OUTPUT: StreamAccumulator, NewStreamAccumulator
// POS: [L0 foundation] Shared bounded ordered stream assembly preserving authoritative non-text blocks and metadata.
package wire

import (
	"encoding/json"
	"errors"
	"fmt"
	"sort"
)

const MaxStreamContentIndex = 1 << 20
const MaxStreamContentBlocks = 4096

type indexedContent struct {
	block    ContentBlock
	complete bool
}

// StreamAccumulator is single-consumer. Completed blocks are authoritative;
// ChunkReasoning is progress only and must be followed by a complete reasoning
// block when provider continuation needs it. Providers assemble tool deltas.
type StreamAccumulator struct {
	message         Message
	maxBytes, bytes int
	finished        bool
	calls           map[string]bool
	failure         error
	contentMode     int // 0=not established, 1=arrival ordered, 2=index ordered
	indexed         map[int]indexedContent
}

func NewStreamAccumulator(maxBytes int) *StreamAccumulator {
	if maxBytes <= 0 {
		maxBytes = 4 << 20
	}
	return &StreamAccumulator{message: Message{Role: RoleAssistant}, maxBytes: maxBytes, calls: make(map[string]bool), indexed: make(map[int]indexedContent)}
}
func (accumulator *StreamAccumulator) Push(chunk StreamChunk) (err error) {
	if accumulator.failure != nil {
		return accumulator.failure
	}
	defer func() {
		if err != nil {
			accumulator.failure = err
		}
	}()
	if !validStrings(chunk.Text, chunk.ResponseID, chunk.FinishReason) {
		return errors.New("stream chunk text is not valid UTF-8")
	}
	if (chunk.Text != "" && chunk.Kind != ChunkText && chunk.Kind != ChunkReasoning) ||
		(chunk.ToolCall != nil && chunk.Kind != ChunkToolCall) ||
		(chunk.ContentBlock != nil && chunk.Kind != ChunkContentBlock) ||
		(chunk.Usage != nil && chunk.Kind != ChunkUsage) ||
		((chunk.ResponseID != "" || chunk.FinishReason != "") && chunk.Kind != ChunkFinish) {
		return errors.New("stream chunk payload does not match kind")
	}
	if accumulator.finished && chunk.Kind != ChunkUsage {
		return errors.New("model emitted content after finish")
	}
	var block *ContentBlock
	switch chunk.Kind {
	case ChunkText:
		value := ContentBlock{Kind: ContentText, Text: chunk.Text}
		block = &value
	case ChunkToolCall:
		value := ContentBlock{Kind: ContentToolCall, ToolCall: chunk.ToolCall}
		block = &value
	case ChunkContentBlock:
		if chunk.ContentBlock == nil {
			return errors.New("model content block missing")
		}
		block = chunk.ContentBlock
	case ChunkReasoning:
		if len(chunk.Text) > accumulator.maxBytes-accumulator.bytes {
			return errors.New("model response byte limit exceeded")
		}
		accumulator.bytes += len(chunk.Text)
	case ChunkUsage:
		if chunk.Usage == nil {
			return errors.New("model usage payload missing")
		}
		usage := CloneUsage(*chunk.Usage)
		accumulator.message.Usage = &usage
	case ChunkFinish:
		metadataBytes := len(chunk.ResponseID) + len(chunk.FinishReason)
		if metadataBytes > accumulator.maxBytes-accumulator.bytes {
			return errors.New("model response byte limit exceeded")
		}
		accumulator.bytes += metadataBytes
		accumulator.finished = true
		accumulator.message.FinishReason = chunk.FinishReason
		accumulator.message.ResponseID = chunk.ResponseID
	default:
		return fmt.Errorf("unknown stream chunk %q", chunk.Kind)
	}
	if block != nil {
		if err := block.Validate(); err != nil {
			return err
		}
		if block.Kind == ContentToolResult {
			return errors.New("model cannot emit tool result")
		}
		index := -1
		if chunk.ContentIndex != nil {
			index = *chunk.ContentIndex
			if index < 0 || index >= MaxStreamContentIndex {
				return errors.New("stream content index exceeds bounds")
			}
			if accumulator.contentMode == 1 {
				return errors.New("model mixed indexed and unindexed content")
			}
			accumulator.contentMode = 2
			if existing, exists := accumulator.indexed[index]; exists {
				if chunk.Kind != ChunkText || existing.block.Kind != ContentText || existing.complete {
					return errors.New("duplicate or incompatible completed stream content slot")
				}
			} else if len(accumulator.indexed) >= MaxStreamContentBlocks {
				return errors.New("model content block count limit exceeded")
			}
		} else {
			if accumulator.contentMode == 2 {
				return errors.New("model mixed indexed and unindexed content")
			}
			accumulator.contentMode = 1
		}
		// Reject the largest raw field before JSON allocates an escaped/base64
		// representation, and never retain a rejected over-budget block.
		remaining := accumulator.maxBytes - accumulator.bytes
		if len(block.Text) > remaining || (block.ToolCall != nil && len(block.ToolCall.Arguments) > remaining) || (block.Reasoning != nil && (len(block.Reasoning.Text) > remaining || len(block.Reasoning.Signature) > remaining || len(block.Reasoning.RedactedData) > remaining)) || (block.ProviderState != nil && len(block.ProviderState.Data) > remaining) || (block.Refusal != nil && len(block.Refusal.Text) > remaining) {
			return errors.New("model response byte limit exceeded")
		}
		if block.ToolCall != nil {
			id := block.ToolCall.ID
			if accumulator.calls[id] {
				return fmt.Errorf("duplicate model tool call %q", id)
			}
			accumulator.calls[id] = true
		}
		encoded, err := json.Marshal(block)
		if err != nil {
			return err
		}
		if len(encoded) > remaining {
			return errors.New("model response byte limit exceeded")
		}
		accumulator.bytes += len(encoded)
		if index >= 0 {
			existing, exists := accumulator.indexed[index]
			if exists {
				existing.block.Text += block.Text
				accumulator.indexed[index] = existing
			} else {
				accumulator.indexed[index] = indexedContent{block: CloneContent([]ContentBlock{*block})[0], complete: chunk.Kind != ChunkText}
			}
		} else if chunk.Kind == ChunkText && len(accumulator.message.Content) > 0 && accumulator.message.Content[len(accumulator.message.Content)-1].Kind == ContentText {
			accumulator.message.Content[len(accumulator.message.Content)-1].Text += block.Text
		} else {
			if len(accumulator.message.Content) >= MaxStreamContentBlocks {
				return errors.New("model content block count limit exceeded")
			}
			accumulator.message.Content = append(accumulator.message.Content, CloneContent([]ContentBlock{*block})[0])
		}
	}
	if accumulator.bytes > accumulator.maxBytes {
		return errors.New("model response byte limit exceeded")
	}
	return nil
}

// Snapshot includes partial blocks for diagnostics; Finish is mandatory for success.
func (accumulator *StreamAccumulator) Snapshot() Message {
	message := accumulator.message
	if accumulator.contentMode == 2 {
		indices := make([]int, 0, len(accumulator.indexed))
		for index := range accumulator.indexed {
			indices = append(indices, index)
		}
		sort.Ints(indices)
		message.Content = make([]ContentBlock, 0, len(indices))
		for _, index := range indices {
			message.Content = append(message.Content, accumulator.indexed[index].block)
		}
	}
	return CloneMessages([]Message{message})[0]
}
func (accumulator *StreamAccumulator) Finish() (Message, error) {
	if accumulator.failure != nil {
		return accumulator.Snapshot(), accumulator.failure
	}
	if !accumulator.finished {
		return accumulator.Snapshot(), errors.New("model stream ended without finish")
	}
	return accumulator.Snapshot(), nil
}
