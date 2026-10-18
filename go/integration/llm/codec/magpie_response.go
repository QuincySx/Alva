// INPUT: Authoritative native-decoded wire messages, ordered content and inclusive usage.
// OUTPUT: Private bounded Magpie IR projection and validated incremental-text/complete-tool emission.
// POS: Cross-protocol responses reuse Magpie renderers while native decoding remains the single content authority.
package codec

import (
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/go/integration/llm/codec/internal/magpie"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func magpieMessage(message wire.Message) (magpie.Result, error) {
	result := magpie.Result{ID: message.ResponseID, Stop: "stop"}
	for _, block := range message.Content {
		switch block.Kind {
		case wire.ContentText:
			result.Parts = append(result.Parts, magpie.Part{Kind: magpie.Text, Text: block.Text})
		case wire.ContentToolCall:
			if block.ToolCall == nil {
				return result, errors.New("missing tool call")
			}
			call := block.ToolCall
			result.Parts = append(result.Parts, magpie.Part{Kind: magpie.ToolCall, ID: call.ID, Name: call.Name, Args: append(json.RawMessage(nil), call.Arguments...)})
			result.Stop = "tool"
		case wire.ContentReasoning:
			if block.Reasoning == nil || block.Reasoning.Signature != "" || block.Reasoning.RedactedData != "" {
				return result, errors.New("unportable reasoning state")
			}
			result.Parts = append(result.Parts, magpie.Part{Kind: magpie.Thinking, Text: block.Reasoning.Text})
		default:
			return result, &LossError{Diagnostics: []Diagnostic{{Code: "response_semantics_lost", Path: "content", Message: "native response content has no portable representation", Severity: Error}}}
		}
	}
	if message.Usage != nil {
		usage, err := magpieUsage(*message.Usage)
		if err != nil {
			return result, err
		}
		result.Usage = usage
	}
	return result, nil
}
func magpieUsage(source wire.Usage) (magpie.Usage, error) {
	var out magpie.Usage
	maximum := uint64(^uint(0) >> 1)
	for _, count := range []uint64{source.InputTokens, source.OutputTokens, source.ReasoningTokens, source.TotalTokens} {
		if count > maximum {
			return out, errors.New("usage exceeds portable conversion range")
		}
	}
	input := source.InputTokens
	for _, field := range []struct {
		source *uint64
		target *int
	}{{source.CachedInputTokens, &out.CacheRead}, {source.CacheWriteInputTokens, &out.CacheWrite}} {
		if field.source != nil {
			if *field.source > input || *field.source > maximum {
				return out, errors.New("cache usage exceeds inclusive input")
			}
			input -= *field.source
			*field.target = int(*field.source)
		}
	}
	out.Input = int(input)
	out.Output = int(source.OutputTokens)
	out.Reasoning = int(source.ReasoningTokens)
	return out, nil
}
func (s *ResponseStream) acceptConvertedChunk(chunk wire.StreamChunk) error {
	if err := s.accumulator.Push(chunk); err != nil {
		return err
	}
	if chunk.Kind == wire.ChunkText && !s.buffering && (chunk.ContentIndex == nil || *chunk.ContentIndex == 0) {
		s.prefix.WriteString(chunk.Text)
		return s.state.Push(magpie.Event{Kind: magpie.KText, Text: chunk.Text})
	}
	if chunk.Kind != wire.ChunkUsage && chunk.Kind != wire.ChunkFinish {
		s.buffering = true
	}
	return nil
}
func (s *ResponseStream) finishConvertedContent() error {
	message, err := s.accumulator.Finish()
	if err != nil {
		return err
	}
	result, err := magpieMessage(message)
	if err != nil {
		return err
	}
	prefix := s.prefix.String()
	for _, part := range result.Parts {
		if prefix != "" {
			if part.Kind != magpie.Text {
				return errors.New("source reordered previously streamed text")
			}
			n := min(len(prefix), len(part.Text))
			if prefix[:n] != part.Text[:n] {
				return errors.New("source changed previously streamed text")
			}
			prefix = prefix[n:]
			part.Text = part.Text[n:]
			if part.Text == "" {
				continue
			}
		}
		switch part.Kind {
		case magpie.Text:
			err = s.state.Push(magpie.Event{Kind: magpie.KText, Text: part.Text})
		case magpie.Thinking:
			if err = s.state.Push(magpie.Event{Kind: magpie.KThinkStart}); err == nil {
				err = s.state.Push(magpie.Event{Kind: magpie.KThink, Text: part.Text})
			}
		case magpie.ToolCall:
			if err = s.state.Push(magpie.Event{Kind: magpie.KToolStart, ID: part.ID, Name: part.Name}); err == nil {
				err = s.state.Push(magpie.Event{Kind: magpie.KToolArgs, Text: string(part.Args)})
			}
		}
		if err != nil {
			return err
		}
	}
	if prefix != "" {
		return errors.New("source omitted previously streamed text")
	}
	if err = s.state.Push(magpie.Event{Kind: magpie.KUsage, Usage: result.Usage}); err != nil {
		return err
	}
	if err = s.state.Push(magpie.Event{Kind: magpie.KStop, Stop: result.Stop}); err != nil {
		return err
	}
	return s.state.Finish()
}

// Source text is always literal. Neither <think> markup nor call-looking prose
// is interpreted as reasoning or a tool invocation by this bridge.
