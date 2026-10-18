// INPUT: Explicit protocol, bounded complete native SSE data events and synchronous chunk consumer
// OUTPUT: NewDecoder with sticky validation failures and native-terminal-gated one-time Finish
// POS: [integration / protocol codec] Portable native JSON-to-wire decoding; no HTTP, SSE framing, credentials or execution policy.
package codec

import (
	"bytes"
	"context"
	"errors"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const (
	defaultEventBytes    = 1 << 20
	defaultResponseBytes = 8 << 20
	defaultToolCalls     = 128
	defaultArgumentBytes = 256 << 10
)

type eventConsumer func(context.Context, Event, func(wire.StreamChunk) error) (bool, error)

type nativeDecoder struct {
	protocol           Protocol
	limits             Limits
	consume            eventConsumer
	bytes              int
	terminal, finished bool
	failure            error
	finish             *wire.StreamChunk
	calls              map[string]bool
	slots              map[int]decoderSlot
	contentMode        int
	contentBlocks      int
	lastContentKind    wire.ContentKind
}

type decoderSlot struct {
	kind     wire.ContentKind
	complete bool
}

func decoderLimits(limits Limits) (Limits, error) {
	for _, bound := range []struct {
		value   *int
		maximum int
	}{{&limits.MaxEventBytes, defaultEventBytes}, {&limits.MaxResponseBytes, defaultResponseBytes}, {&limits.MaxToolCalls, defaultToolCalls}, {&limits.MaxArgumentBytes, defaultArgumentBytes}} {
		if *bound.value == 0 {
			*bound.value = bound.maximum
		}
		if *bound.value < 1 || *bound.value > bound.maximum {
			return Limits{}, errors.New("native decoder limits exceed supported hard bounds")
		}
	}
	return limits, nil
}

func NewDecoder(protocol Protocol, limits Limits) (StreamDecoder, error) {
	limits, err := decoderLimits(limits)
	if err != nil {
		return nil, err
	}
	d := &nativeDecoder{protocol: protocol, limits: limits, calls: make(map[string]bool), slots: make(map[int]decoderSlot)}
	switch protocol {
	case OpenAIChat:
		d.consume = newChatDecoder(limits)
	case OpenAIResponses:
		d.consume = newResponsesDecoder(limits)
	case Anthropic:
		d.consume = newAnthropicDecoder(limits)
	case Gemini:
		d.consume = newGeminiDecoder(limits)
	default:
		return nil, errors.New("unsupported native model protocol")
	}
	return d, nil
}

// Push and Finish are single-consumer operations. The caller owns framing and
// must wait for each synchronous callback; this decoder creates no goroutine.
func (d *nativeDecoder) Push(ctx context.Context, event Event, emit func(wire.StreamChunk) error) (err error) {
	if d.failure != nil {
		return d.failure
	}
	defer func() {
		if err != nil {
			d.failure = err
		}
	}()
	if err = ctx.Err(); err != nil {
		return err
	}
	if emit == nil {
		return errors.New("native decoder callback required")
	}
	if d.finished || d.terminal && d.protocol != Gemini {
		return errors.New("native event followed a completed model stream")
	}
	if len(event.Name) > 256 || !utf8.ValidString(event.Name) || len(event.Data) > d.limits.MaxEventBytes {
		return errors.New("native model event exceeds bounds or contains invalid UTF-8")
	}
	if len(event.Data)+len(event.Name) > d.limits.MaxResponseBytes-d.bytes {
		return errors.New("native model response byte limit exceeded")
	}
	d.bytes += len(event.Data) + len(event.Name)
	if !(d.protocol == OpenAIChat && string(event.Data) == "[DONE]") {
		trimmed := bytes.TrimSpace(event.Data)
		if validateNativeJSON(event.Data, d.limits.MaxEventBytes) != nil || len(trimmed) == 0 || trimmed[0] != '{' {
			return errors.New("native model event requires one valid JSON/Unicode object")
		}
	}
	var callbackErr error
	done, consumeErr := d.consume(ctx, event, func(chunk wire.StreamChunk) error {
		if err := ctx.Err(); err != nil {
			callbackErr = errors.Join(callbackErr, err)
			return err
		}
		if err := d.validateChunk(chunk); err != nil {
			callbackErr = errors.Join(callbackErr, err)
			return err
		}
		if chunk.Kind == wire.ChunkFinish {
			if d.finish != nil {
				// Gemini may send a final usage-only event after STOP, including
				// the response identity's first appearance. Revise only held
				// metadata; public Finish still emits exactly once after framing.
				if d.protocol != Gemini || d.finish.FinishReason != chunk.FinishReason || d.finish.ResponseID != "" && d.finish.ResponseID != chunk.ResponseID {
					return errors.New("native model supplied duplicate or conflicting completion")
				}
			}
			value := wire.CloneStreamChunk(chunk)
			d.finish = &value
			return nil
		}
		err := emit(wire.CloneStreamChunk(chunk))
		callbackErr = errors.Join(callbackErr, err)
		return err
	})
	if err = errors.Join(consumeErr, callbackErr, ctx.Err()); err != nil {
		return err
	}
	if done {
		if d.finish == nil {
			return errors.New("native terminal did not provide a complete outcome")
		}
		d.terminal = true
	}
	return nil
}

// Validate emitted authority even when a host deliberately discards all chunks
// to validate a Magpie input stream. Keep only small identities/slot metadata,
// not a second complete transcript or hidden tool/context body.
func (d *nativeDecoder) validateChunk(chunk wire.StreamChunk) error {
	var block wire.ContentBlock
	switch chunk.Kind {
	case wire.ChunkFinish:
		if !utf8.ValidString(chunk.ResponseID) || !utf8.ValidString(chunk.FinishReason) || chunk.FinishReason == "" {
			return errors.New("invalid native completion metadata")
		}
		return nil
	case wire.ChunkUsage:
		if chunk.Usage == nil {
			return errors.New("native usage payload missing")
		}
		return nil
	case wire.ChunkReasoning:
		if !utf8.ValidString(chunk.Text) {
			return errors.New("invalid native reasoning progress")
		}
		return nil
	case wire.ChunkText:
		block = wire.ContentBlock{Kind: wire.ContentText, Text: chunk.Text}
	case wire.ChunkToolCall:
		block = wire.ContentBlock{Kind: wire.ContentToolCall, ToolCall: chunk.ToolCall}
	case wire.ChunkContentBlock:
		if chunk.ContentBlock == nil {
			return errors.New("native content block missing")
		}
		block = *chunk.ContentBlock
	default:
		return errors.New("unsupported native wire chunk")
	}
	if err := block.Validate(); err != nil {
		return err
	}
	if block.Kind == wire.ContentToolResult {
		return errors.New("native model cannot emit a tool result")
	}
	if chunk.ContentIndex != nil {
		index := *chunk.ContentIndex
		if index < 0 || index >= wire.MaxStreamContentIndex || d.contentMode == 1 {
			return errors.New("native content index out of bounds or mixed ordering")
		}
		d.contentMode = 2
		if previous, ok := d.slots[index]; ok {
			if chunk.Kind != wire.ChunkText || previous.kind != wire.ContentText || previous.complete {
				return errors.New("native content slot completed more than once")
			}
		} else {
			if len(d.slots) >= wire.MaxStreamContentBlocks {
				return errors.New("native content block count exceeds limit")
			}
			d.slots[index] = decoderSlot{kind: block.Kind, complete: chunk.Kind != wire.ChunkText}
		}
	} else {
		if d.contentMode == 2 {
			return errors.New("native content mixed indexed and unindexed ordering")
		}
		d.contentMode = 1
		if chunk.Kind != wire.ChunkText || d.lastContentKind != wire.ContentText {
			if d.contentBlocks >= wire.MaxStreamContentBlocks {
				return errors.New("native content block count exceeds limit")
			}
			d.contentBlocks++
		}
		d.lastContentKind = block.Kind
	}
	if block.ToolCall != nil {
		call := block.ToolCall
		if d.calls[call.ID] || len(d.calls) >= d.limits.MaxToolCalls || len(call.Arguments) > d.limits.MaxArgumentBytes {
			return errors.New("duplicate or oversized native tool call")
		}
		d.calls[call.ID] = true
	}
	return nil
}

func (d *nativeDecoder) Terminal() bool { return d.terminal && d.failure == nil }

func (d *nativeDecoder) Finish(ctx context.Context, emit func(wire.StreamChunk) error) (err error) {
	if d.failure != nil {
		return d.failure
	}
	defer func() {
		if err != nil {
			d.failure = err
		}
	}()
	if err = ctx.Err(); err != nil {
		return err
	}
	if d.finished || !d.terminal || d.finish == nil {
		return errors.New("native stream is incomplete or already finalized")
	}
	if emit == nil {
		return errors.New("native decoder callback required")
	}
	d.finished = true
	return emit(wire.CloneStreamChunk(*d.finish))
}

func contentIndex(value int) *int { return &value }
