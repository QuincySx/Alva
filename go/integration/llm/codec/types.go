// INPUT: context, encoding/json and own llm/wire values only.
// OUTPUT: Protocol/Request/ProtocolAdapter, Event/Limits/StreamDecoder and protocol-neutral conversion diagnostics/results.
// POS: [integration / protocol codec] Portable protocol boundary exposes no kernel, product, HTTP, credential or Magpie business DTO authority.
package codec

import (
	"context"
	"encoding/json"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Protocol string

const (
	OpenAIChat      Protocol = "openai_chat"
	OpenAIResponses Protocol = "openai_responses"
	Anthropic       Protocol = "anthropic"
	Gemini          Protocol = "gemini"
)

type Request struct {
	Model           string
	Messages        []wire.Message
	Tools           []wire.ToolDefinition
	Config          wire.ModelConfig
	MaxOutputTokens int
	Stream          bool
}
type ProtocolAdapter interface {
	Protocol() Protocol
	EncodeRequest(context.Context, Request) (json.RawMessage, error)
	NewDecoder(Limits) (StreamDecoder, error)
}

// Event is one fully framed SSE event. Data deliberately remains bytes because
// Chat's terminal [DONE] sentinel is not JSON. Framing/HTTP stay outside codec.
type Event struct {
	Name string
	Data []byte
}
type Limits struct{ MaxEventBytes, MaxResponseBytes, MaxToolCalls, MaxArgumentBytes int }
type StreamDecoder interface {
	Push(context.Context, Event, func(wire.StreamChunk) error) error
	Finish(context.Context, func(wire.StreamChunk) error) error
	Terminal() bool
}
type Severity string

const (
	Warning Severity = "warning"
	Error   Severity = "error"
)

type Diagnostic struct {
	Code, Path, Message string
	Severity            Severity
	From, To            Protocol
}
type Step struct {
	Converter string
	From, To  Protocol
}
type Result struct {
	Body        json.RawMessage
	From, To    Protocol
	Converter   string
	Quality     string
	Steps       []Step
	Diagnostics []Diagnostic
}
type LossError struct{ Diagnostics []Diagnostic }

func (*LossError) Error() string { return "protocol conversion would lose required semantics" }
