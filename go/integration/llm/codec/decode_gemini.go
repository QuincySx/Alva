// INPUT: Bounded Gemini native candidate/usage events and explicit STOP
// OUTPUT: Native thought signatures, exact/generated call identities and terminal-held finish
// POS: [integration / protocol codec] Pure native codec never executes code, opens media or falls back through Chat conversion.
package codec

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"math"
)

type geminiChunk struct {
	ResponseID string `json:"responseId"`
	Candidates []struct {
		Index   int `json:"index"`
		Content struct {
			Parts []struct {
				Text             string `json:"text"`
				Thought          bool   `json:"thought"`
				ThoughtSignature string `json:"thoughtSignature"`
				Call             *struct {
					ID   string          `json:"id"`
					Name string          `json:"name"`
					Args json.RawMessage `json:"args"`
				} `json:"functionCall"`
				InlineData     json.RawMessage `json:"inlineData"`
				FileData       json.RawMessage `json:"fileData"`
				ExecutableCode json.RawMessage `json:"executableCode"`
				CodeResult     json.RawMessage `json:"codeExecutionResult"`
			} `json:"parts"`
		} `json:"content"`
		Finish string `json:"finishReason"`
	} `json:"candidates"`
	Feedback struct {
		Block string `json:"blockReason"`
	} `json:"promptFeedback"`
	Usage struct {
		Input     *uint64 `json:"promptTokenCount"`
		Output    *uint64 `json:"candidatesTokenCount"`
		ToolInput *uint64 `json:"toolUsePromptTokenCount"`
		Cached    *uint64 `json:"cachedContentTokenCount"`
		Reasoning *uint64 `json:"thoughtsTokenCount"`
		Total     *uint64 `json:"totalTokenCount"`
	} `json:"usageMetadata"`
	Error json.RawMessage `json:"error"`
}

func newGeminiDecoder(limits Limits) eventConsumer {
	prefix := ""
	var calls []wire.ToolCall
	ids := map[string]bool{}
	finished := false
	var input, output, toolInput, thoughts uint64
	var finalUsage wire.Usage
	responseID := ""
	return func(_ context.Context, incoming Event, emit func(wire.StreamChunk) error) (bool, error) {
		data := incoming.Data
		var chunk geminiChunk
		if json.Unmarshal(data, &chunk) != nil {
			return false, errors.New("invalid Gemini SSE JSON")
		}
		if finished {
			var fields map[string]json.RawMessage
			_ = json.Unmarshal(data, &fields)
			if len(chunk.Candidates) > 0 || len(fields["usageMetadata"]) == 0 || string(fields["usageMetadata"]) == "null" {
				return false, errors.New("Gemini event followed final STOP without usage-only metadata")
			}
		}
		if len(chunk.Error) > 0 && string(chunk.Error) != "null" || chunk.Feedback.Block != "" {
			return false, errors.New("Gemini response failed or was blocked")
		}
		if len(chunk.Candidates) > 1 {
			return false, errors.New("multiple Gemini candidates unsupported")
		}
		if chunk.Usage.Input != nil {
			input = *chunk.Usage.Input
		}
		if chunk.Usage.Output != nil {
			output = *chunk.Usage.Output
		}
		if chunk.Usage.ToolInput != nil {
			toolInput = *chunk.Usage.ToolInput
		}
		if chunk.ResponseID != "" {
			if responseID != "" && responseID != chunk.ResponseID {
				return false, errors.New("Gemini response identity changed mid-stream")
			}
			responseID = chunk.ResponseID
		}
		if chunk.Usage.Cached != nil {
			finalUsage.CachedInputTokens = chunk.Usage.Cached
		}
		if chunk.Usage.Reasoning != nil {
			thoughts = *chunk.Usage.Reasoning
			finalUsage.ReasoningTokens = thoughts
		}
		if chunk.Usage.Total != nil {
			finalUsage.TotalTokens = *chunk.Usage.Total
		}
		if input > math.MaxUint64-toolInput || output > math.MaxUint64-thoughts {
			return false, errors.New("Gemini usage addition overflow")
		}
		if input+toolInput > math.MaxUint64-(output+thoughts) {
			return false, errors.New("Gemini total usage addition overflow")
		}
		for _, candidate := range chunk.Candidates {
			if candidate.Index != 0 {
				return false, errors.New("multiple Gemini candidates unsupported")
			}
			if finished {
				return false, errors.New("Gemini candidate followed final STOP")
			}
			for _, part := range candidate.Content.Parts {
				if part.Call != nil && (part.Text != "" || part.Thought) {
					return false, errors.New("Gemini part must have one content payload")
				}
				if len(part.InlineData) > 0 || len(part.FileData) > 0 || len(part.ExecutableCode) > 0 || len(part.CodeResult) > 0 {
					return false, errors.New("unsupported Gemini output part; refusing to silently discard output")
				}
				if part.Thought {
					value := wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: part.Text, Signature: part.ThoughtSignature}}
					if err := emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &value}); err != nil {
						return false, err
					}
					continue
				}
				if part.ThoughtSignature != "" && part.Call == nil {
					return false, errors.New("Gemini non-call thought signature requires explicit association")
				}
				if part.Text == "" && part.Call == nil {
					return false, errors.New("empty or unsupported Gemini output part")
				}
				if part.Text != "" {
					if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: part.Text}); err != nil {
						return false, err
					}
				}
				if part.Call != nil {
					if len(calls) >= limits.MaxToolCalls || part.Call.Name == "" || len(part.Call.Args) > limits.MaxArgumentBytes {
						return false, errors.New("invalid Gemini function call")
					}
					var object map[string]json.RawMessage
					if json.Unmarshal(part.Call.Args, &object) != nil || object == nil {
						return false, errors.New("Gemini function arguments must be an object")
					}
					id := part.Call.ID
					if id == "" {
						if prefix == "" {
							var random [12]byte
							if _, err := rand.Read(random[:]); err != nil {
								return false, err
							}
							prefix = hex.EncodeToString(random[:])
						}
						id = fmt.Sprintf("gemini_%s_%d", prefix, len(calls))
					}
					if ids[id] {
						return false, errors.New("duplicate Gemini function call identity")
					}
					ids[id] = true
					call := wire.ToolCall{ID: id, Name: part.Call.Name, Arguments: append(json.RawMessage(nil), part.Call.Args...)}
					if part.ThoughtSignature != "" {
						state, _ := json.Marshal(map[string]string{"thoughtSignature": part.ThoughtSignature})
						call.ProviderState = []wire.ProviderState{{Provider: "gemini", ToolCallID: id, Data: state}}
					}
					calls = append(calls, call)
					if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
						return false, err
					}
				}
			}
			if candidate.Finish != "" {
				if candidate.Finish != "STOP" {
					return false, errors.New("Gemini generation ended incomplete or blocked")
				}
				finished = true
			}
		}
		if finished {
			finalUsage.InputTokens = input + toolInput
			finalUsage.OutputTokens = output + thoughts
			if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &finalUsage}); err != nil {
				return false, err
			}
			reason := "stop"
			if len(calls) > 0 {
				reason = "tool_calls"
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: reason, ResponseID: responseID}); err != nil {
				return false, err
			}
		}
		return finished, nil

	}
}
