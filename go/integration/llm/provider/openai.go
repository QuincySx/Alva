// INPUT: Explicit model Config, portable codec ProtocolAdapter and kernel model request.
// OUTPUT: OpenAIChat, NewOpenAIChat and SDK Stream using the shared raw transport.
// POS: [integration / model transport] Thin native model bridge keeps pure encoding/decoding outside HTTP and SDK contracts.
package provider

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/integration/llm/codec"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type OpenAIChat struct {
	kind      Kind
	config    Config
	transport *RawTransport
	adapter   codec.ProtocolAdapter
}

func (m *OpenAIChat) ProviderID() string               { return string(m.kind) }
func (m *OpenAIChat) ModelID() string                  { return m.config.Model }
func NewOpenAIChat(config Config) (*OpenAIChat, error) { return newModel(OpenAIChatKind, config) }
func newModel(kind Kind, config Config) (*OpenAIChat, error) {
	transport, err := BuildTransport(kind, config)
	if err != nil {
		return nil, err
	}
	adapter, err := codec.NewProtocolAdapter(codec.Protocol(kind))
	if err != nil {
		return nil, err
	}
	return &OpenAIChat{kind: kind, config: transport.config, transport: transport, adapter: adapter}, nil
}
func (m *OpenAIChat) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if emit == nil {
		return errors.New("stream callback required")
	}
	body, err := m.adapter.EncodeRequest(ctx, codec.Request{Model: m.config.Model, Messages: request.Messages, Tools: request.Tools, Config: request.Config, MaxOutputTokens: m.config.MaxOutputTokens, Stream: true})
	if err != nil {
		return err
	}
	decoder, err := m.adapter.NewDecoder(codec.Limits{MaxEventBytes: m.config.MaxEventBytes})
	if err != nil {
		return err
	}
	err = m.transport.StreamRaw(ctx, body, func(event codec.Event) (bool, error) {
		err := decoder.Push(ctx, event, emit)
		// Gemini permits a following usage-only event. Its official stream
		// closes normally; do not discard a later explicitly reported zero.
		return decoder.Terminal() && m.kind != GeminiKind, err
	})
	if err != nil {
		return err
	}
	return decoder.Finish(ctx, emit)
}
