// INPUT: context, encoding/json, errors/strings/utf8 and own wire-native encoder/decoder boundaries.
// OUTPUT: NewProtocolAdapter providing independent native protocol encoding and validated stream decoder construction.
// POS: [integration / protocol codec] The agent's own-wire transport path never passes signed native state through a lossy cross-protocol Chat route.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const responseLimit = 8 << 20

type nativeAdapter struct{ protocol Protocol }

func NewProtocolAdapter(protocol Protocol) (ProtocolAdapter, error) {
	switch protocol {
	case OpenAIChat, OpenAIResponses, Anthropic, Gemini:
		return nativeAdapter{protocol}, nil
	default:
		return nil, errors.New("unsupported native protocol")
	}
}
func (a nativeAdapter) Protocol() Protocol { return a.protocol }
func (a nativeAdapter) NewDecoder(limits Limits) (StreamDecoder, error) {
	return NewDecoder(a.protocol, limits)
}
func (a nativeAdapter) EncodeRequest(ctx context.Context, request Request) (json.RawMessage, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if request.Model == "" || len(request.Model) > 256 || !utf8.ValidString(request.Model) || strings.ContainsRune(request.Model, 0) || request.MaxOutputTokens < 0 || request.MaxOutputTokens > 1<<20 {
		return nil, errors.New("invalid explicit native model/output budget")
	}
	for _, tool := range request.Tools {
		if tool.Name == "" || !utf8.ValidString(tool.Name) || !utf8.ValidString(tool.Description) || !wire.ValidJSON(tool.Parameters) {
			return nil, errors.New("invalid native tool definition Unicode or JSON")
		}
		var schema map[string]json.RawMessage
		if json.Unmarshal(tool.Parameters, &schema) != nil || schema == nil {
			return nil, errors.New("native tool parameters object required")
		}
	}
	if a.protocol == OpenAIChat {
		return encodeChat(request)
	}
	payload, err := encodeNative(a.protocol, request)
	if err != nil {
		return nil, err
	}
	raw, err := json.Marshal(payload)
	if err != nil || len(raw) > responseLimit {
		return nil, errors.New("native request exceeds 8 MiB or cannot encode")
	}
	if err = ctx.Err(); err != nil {
		return nil, err
	}
	return raw, nil
}
