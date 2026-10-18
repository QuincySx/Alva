// INPUT: Portable codec, provider SSE framing and deterministic native fixture text.
// OUTPUT: Test-only native decoder and request adapter helpers for transport integration regressions.
// POS: [integration / model transport] Old regression fixtures exercise the real extracted codec without production compatibility shims.
package provider

import (
	"context"
	"encoding/json"
	"io"

	"github.com/QuincySx/Alva/integration/llm/codec"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func parseFixture(protocol codec.Protocol, ctx context.Context, reader io.Reader, limit int, emit func(wire.StreamChunk) error) error {
	decoder, err := codec.NewDecoder(protocol, codec.Limits{MaxEventBytes: limit})
	if err != nil {
		return err
	}
	err = readSSE(ctx, reader, limit, func(event codec.Event) (bool, error) {
		err := decoder.Push(ctx, event, emit)
		return decoder.Terminal() && protocol != codec.Gemini, err
	})
	if err != nil {
		return err
	}
	return decoder.Finish(ctx, emit)
}
func parseChatFixture(ctx context.Context, r io.Reader, n int, emit func(wire.StreamChunk) error) error {
	return parseFixture(codec.OpenAIChat, ctx, r, n, emit)
}
func parseResponsesFixture(ctx context.Context, r io.Reader, n int, emit func(wire.StreamChunk) error) error {
	return parseFixture(codec.OpenAIResponses, ctx, r, n, emit)
}
func parseAnthropicFixture(ctx context.Context, r io.Reader, n int, emit func(wire.StreamChunk) error) error {
	return parseFixture(codec.Anthropic, ctx, r, n, emit)
}
func parseGeminiFixture(ctx context.Context, r io.Reader, n int, emit func(wire.StreamChunk) error) error {
	return parseFixture(codec.Gemini, ctx, r, n, emit)
}
func encodeProtocolFixture(kind Kind, config Config, request abi.ModelRequest) (map[string]any, error) {
	adapter, err := codec.NewProtocolAdapter(codec.Protocol(kind))
	if err != nil {
		return nil, err
	}
	raw, err := adapter.EncodeRequest(context.Background(), codec.Request{Model: config.Model, Messages: request.Messages, Tools: request.Tools, Config: request.Config, MaxOutputTokens: config.MaxOutputTokens, Stream: true})
	if err != nil {
		return nil, err
	}
	var object map[string]any
	err = json.Unmarshal(raw, &object)
	return object, err
}
