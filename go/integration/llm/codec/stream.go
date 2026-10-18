// INPUT: Validated native source events, private exchange metadata and the absorbed Magpie stream converters.
// OUTPUT: ResponseStream with loss-gated events and native-terminal-gated finalization.
// POS: [integration / protocol codec] Converted completion events are held until real source success, never synthesized from truncated EOF.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/QuincySx/Alva/go/integration/llm/codec/internal/magpie"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type ResponseStream struct {
	exchange          *Exchange
	decoder           StreamDecoder
	state             *magpie.Encoder
	accumulator       *wire.StreamAccumulator
	prefix            strings.Builder
	buffering         bool
	currentCtx        context.Context
	currentEmit       func(Event) error
	presence          cachePresence
	usage             *wire.Usage
	pending           []Event
	outputBytes       int
	holding, finished bool
	failure           error
}

// ResponseStream is single-consumer, as are its native decoder and callbacks.
func (s *ResponseStream) Push(ctx context.Context, event Event, emit func(Event) error) (err error) {
	if s.failure != nil {
		return s.failure
	}
	defer func() {
		if err != nil {
			s.failure = err
		}
	}()
	if emit == nil || s.finished {
		return errors.New("stream callback required and stream must remain open")
	}
	if string(event.Data) != "[DONE]" {
		if err = validateNativeJSON(event.Data, s.exchange.options.Limits.MaxEventBytes); err != nil {
			return err
		}
		if s.exchange.upstream != s.exchange.inbound {
			if err = checkDynamicNativeKeys(event.Data); err != nil {
				return err
			}
		}
		if err = continuationGuard(s.exchange.upstream, s.exchange.inbound, event.Data); err != nil {
			return err
		}
	}
	if s.state != nil {
		s.currentCtx, s.currentEmit = ctx, emit
		defer func() { s.currentCtx = nil; s.currentEmit = nil }()
	}
	if err = s.decoder.Push(ctx, event, func(chunk wire.StreamChunk) error {
		if chunk.ToolCall != nil {
			if err := argumentNumberGuard(s.exchange.upstream, s.exchange.inbound, chunk.ToolCall.Arguments); err != nil {
				return err
			}
		}
		if chunk.Kind == wire.ChunkUsage && chunk.Usage != nil {
			value := wire.CloneUsage(*chunk.Usage)
			s.usage = &value
		}
		if s.state != nil {
			return s.acceptConvertedChunk(chunk)
		}
		return nil
	}); err != nil {
		return err
	}
	if string(event.Data) != "[DONE]" {
		if err = s.presence.observe(s.exchange.upstream, event.Data); err != nil {
			return err
		}
	}
	if s.state == nil {
		if string(event.Data) != "[DONE]" {
			event.Data, err = patchModel(event.Data, s.exchange.inbound, s.exchange.options.ResponseModel, true)
			if err != nil {
				return err
			}
		}
		return s.deliver(ctx, event, emit)
	}
	return nil
}
func (s *ResponseStream) Terminal() bool { return s.failure == nil && s.decoder.Terminal() }
func (s *ResponseStream) Finish(ctx context.Context, emit func(Event) error) (err error) {
	if s.failure != nil {
		return s.failure
	}
	defer func() {
		if err != nil {
			s.failure = err
		}
	}()
	if s.finished || emit == nil {
		return errors.New("stream already finalized or callback missing")
	}
	if s.state != nil {
		s.currentCtx, s.currentEmit = ctx, emit
		defer func() { s.currentCtx = nil; s.currentEmit = nil }()
	}
	if err = s.decoder.Finish(ctx, func(chunk wire.StreamChunk) error {
		if s.state != nil {
			return s.acceptConvertedChunk(chunk)
		}
		return nil
	}); err != nil {
		return err
	}
	s.finished = true
	if s.state != nil {
		s.currentCtx, s.currentEmit = ctx, emit
		defer func() { s.currentCtx = nil; s.currentEmit = nil }()
		_, err = guarded(func() (struct{}, error) { return struct{}{}, s.finishConvertedContent() })
		if err != nil {
			return conversionError(err)
		}
	}
	for _, event := range s.pending {
		if err = ctx.Err(); err != nil {
			return err
		}
		if s.state != nil && string(event.Data) != "[DONE]" {
			previousBytes := len(event.Data)
			event.Data, err = s.presence.apply(s.exchange.inbound, event.Data)
			if err != nil {
				return err
			}
			if s.usage != nil {
				event.Data, err = projectUsage(s.exchange.inbound, event.Data, *s.usage)
				if err != nil {
					return err
				}
			}
			if len(event.Data) > s.exchange.options.Limits.MaxEventBytes || len(event.Data)-previousBytes > s.exchange.options.Limits.MaxResponseBytes-s.outputBytes {
				return errors.New("final converted usage exceeds stream bounds")
			}
			s.outputBytes += len(event.Data) - previousBytes
		}
		if err = emit(Event{Name: event.Name, Data: append([]byte(nil), event.Data...)}); err != nil {
			return err
		}
	}
	s.pending = nil
	return ctx.Err()
}
func (s *ResponseStream) deliverConverted(frame magpie.Frame) error {
	if s.currentCtx == nil || s.currentEmit == nil {
		return errors.New("converter emitted outside active call")
	}
	event := Event{Name: frame.Name, Data: append([]byte(nil), frame.Data...)}
	if string(event.Data) != "[DONE]" {
		var err error
		if err = validateNativeJSON(event.Data, s.exchange.options.Limits.MaxEventBytes); err != nil {
			return err
		}
		event.Data, err = s.presence.apply(s.exchange.inbound, event.Data)
		if err != nil {
			return err
		}
		if s.usage != nil {
			event.Data, err = projectUsage(s.exchange.inbound, event.Data, *s.usage)
			if err != nil {
				return err
			}
		}
		event.Data, err = foreignModel(event.Data, s.exchange.inbound, s.exchange.options.ResponseModel)
		if err != nil {
			return err
		}
	}
	return s.deliver(s.currentCtx, event, s.currentEmit)
}
func (s *ResponseStream) deliver(ctx context.Context, event Event, emit func(Event) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	limits := s.exchange.options.Limits
	if len(event.Data) > limits.MaxEventBytes || len(event.Data)+len(event.Name) > limits.MaxResponseBytes-s.outputBytes {
		return errors.New("converted stream output exceeds bounds")
	}
	s.outputBytes += len(event.Data) + len(event.Name)
	if s.holding || terminalEvent(s.exchange.inbound, event) {
		s.holding = true
		s.pending = append(s.pending, Event{Name: event.Name, Data: append([]byte(nil), event.Data...)})
		return nil
	}
	return emit(Event{Name: event.Name, Data: append([]byte(nil), event.Data...)})
}
func terminalEvent(protocol Protocol, event Event) bool {
	if string(event.Data) == "[DONE]" {
		return true
	}
	var value struct {
		Type    string
		Choices []struct {
			Finish *string `json:"finish_reason"`
		}
		Delta      json.RawMessage
		Candidates []struct {
			Finish string `json:"finishReason"`
		}
	}
	if json.Unmarshal(event.Data, &value) != nil {
		return true
	}
	switch protocol {
	case OpenAIChat:
		for _, choice := range value.Choices {
			if choice.Finish != nil {
				return true
			}
		}
	case OpenAIResponses:
		return value.Type == "response.completed" || value.Type == "response.failed" || value.Type == "response.incomplete"
	case Anthropic:
		var delta struct {
			Stop *string `json:"stop_reason"`
		}
		if len(value.Delta) > 0 && json.Unmarshal(value.Delta, &delta) != nil {
			return true
		}
		return value.Type == "message_stop" || value.Type == "message_delta" && delta.Stop != nil
	case Gemini:
		for _, candidate := range value.Candidates {
			if candidate.Finish != "" {
				return true
			}
		}
	}
	return false
}
