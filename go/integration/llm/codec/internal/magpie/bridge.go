// INPUT: crypto/sha256, encoding/hex, encoding/json, errors, net/url, strings.
// OUTPUT: Frame, RequestShape, ParseRequest, BuildRequest, Render, CompletionEvents, Encoder, NewEncoder, FlatName, DrawsID.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	sha256 "crypto/sha256"
	hex "encoding/hex"
	json "encoding/json"
	errors "errors"
	url "net/url"
	strings "strings"
)

const (
	Chat      = "openai_chat"
	Responses = "openai_responses"
	Anthropic = "anthropic"
	Gemini    = "gemini"
)

type Frame struct {
	Name string
	Data []byte
}

func RequestShape(protocol string) any {
	switch protocol {
	case Chat:
		return &cRequest{}
	case Responses:
		return &rRequest{}
	case Anthropic:
		return &aRequest{}
	case Gemini:
		return &gRequest{}
	}
	return nil
}

func ParseRequest(protocol string, body []byte) (*Request, error) {
	switch protocol {
	case Chat:
		return parseChat(body)
	case Responses:
		return parseResponses(body)
	case Anthropic:
		return parseAnthropic(body)
	case Gemini:
		return parseGemini(body)
	}
	return nil, errors.New("unknown protocol")
}

func BuildRequest(protocol string, r *Request, model string, stream bool, maxTokens int) ([]byte, error) {
	r.Model = model
	r.Stream = stream
	if protocol == Anthropic && r.MaxTokens <= 0 {
		if maxTokens <= 0 {
			return nil, errors.New("Anthropic output ceiling required")
		}
		r.MaxTokens = maxTokens
	}
	switch protocol {
	case Chat:
		return buildChat(r, model, "", false), nil
	case Responses:
		return buildResponses(r, model, "", false), nil
	case Anthropic:
		return buildAnthropic(r, model), nil
	case Gemini:
		return buildGemini(r, model)
	}
	return nil, errors.New("unknown protocol")
}

func Render(protocol string, r Result, model string, request *Request) ([]byte, error) {
	switch protocol {
	case Chat:
		return renderChat(r, model), nil
	case Responses:
		return renderResponses(r, model, request.Namespaced), nil
	case Anthropic:
		return renderAnthropic(r, model), nil
	case Gemini:
		return renderGemini(r, model), nil
	}
	return nil, errors.New("unknown protocol")
}

func CompletionEvents(protocol string, body []byte) ([]Frame, error) {
	var evs []wholeEvent
	switch protocol {
	case Chat:
		evs = chatWholeEvents(body)
	case Responses:
		evs = responsesWholeEvents(body)
	case Anthropic:
		evs = anthropicWholeEvents(body)
	case Gemini:
		return []Frame{{Data: append([]byte(nil), body...)}}, nil
	}
	if len(evs) == 0 {
		return nil, errors.New("invalid native completion")
	}
	frames := make([]Frame, 0, len(evs))
	for _, ev := range evs {
		b, err := json.Marshal(ev.data)
		if err != nil {
			return nil, err
		}
		if s, ok := ev.data.(string); ok {
			b = []byte(s)
		}
		frames = append(frames, Frame{Name: ev.name, Data: b})
	}
	return frames, nil
}

type eventEncoder interface {
	event(Event)
	finish()
	keepalive()
}

type Encoder struct {
	encoder eventEncoder
	writer  *sseWriter
}

func NewEncoder(protocol, model string, request *Request, emit func(Frame) error) (*Encoder, error) {
	if emit == nil {
		return nil, errors.New("event callback required")
	}
	w := &sseWriter{emit: emit}
	var enc eventEncoder
	switch protocol {
	case Chat:
		enc = &chatEncoder{w: w, model: model}
	case Responses:
		enc = &responsesEncoder{w: w, model: model, named: request.Namespaced}
	case Anthropic:
		enc = &anthropicEncoder{w: w, model: model}
	case Gemini:
		enc = &geminiEncoder{w: w, model: model}
	default:
		return nil, errors.New("unknown protocol")
	}
	return &Encoder{encoder: enc, writer: w}, nil
}

func (e *Encoder) Push(ev Event) error {
	if e.writer.err != nil {
		return e.writer.err
	}
	e.encoder.event(ev)
	return e.writer.err
}

func (e *Encoder) Finish() error {
	if e.writer.err != nil {
		return e.writer.err
	}
	e.encoder.finish()
	return e.writer.err
}

// The fork's writer has no HTTP object, queues or goroutines. Once emit fails,
// subsequent upstream encoder writes are discarded and the original error wins.
type sseWriter struct {
	emit func(Frame) error
	err  error
}

func (w *sseWriter) event(name string, value any) {
	if w.err != nil {
		return
	}
	var data []byte
	switch v := value.(type) {
	case string:
		data = []byte(v)
	case []byte:
		data = append([]byte(nil), v...)
	default:
		data, w.err = json.Marshal(v)
	}
	if w.err == nil {
		w.err = w.emit(Frame{Name: name, Data: data})
	}
}

func (*sseWriter) comment(string) {}

func decodeJSON(raw []byte, value any) error { return json.Unmarshal(raw, value) }

// Keep schemas intact. The outer codec rejects unsupported schema semantics
// before conversion; the upstream's union-flattening fallback is not enabled.
func objectSchema(schema json.RawMessage) json.RawMessage {
	if len(schema) == 0 {
		return json.RawMessage(`{"type":"object","properties":{}}`)
	}
	return schema
}

// FlatName and DrawsID are copied from upstream provider/grok.go and
// catalog/catalog.go at the same pinned commit (see provenance.json).
func FlatName(namespace, name string) string {
	flat := namespace + "__" + name
	if len(flat) <= 64 {
		return flat
	}
	sum := sha256.Sum256([]byte(namespace + "\x00" + name))
	return flat[:55] + "_" + hex.EncodeToString(sum[:4])
}

func DrawsID(id string) bool {
	id = strings.ToLower(id)
	for _, w := range []string{"image", "imagen", "imagine", "dall-e", "flux", "seedream", "cogview", "stable-diffusion", "sdxl", "wanx", "kolors", "hidream", "banana"} {
		if strings.Contains(id, w) {
			return true
		}
	}
	return false
}

func (*sseWriter) begin() {}

func hostWithoutPort(host string) string {
	if u, err := url.Parse("https://" + host); err == nil && u.Hostname() != "" {
		return u.Hostname()
	}
	return host
}

// Fixed protocol hints avoid mime.TypeByExtension's ambient OS MIME database.
func fileMediaType(extension string) string {
	return map[string]string{".pdf": "application/pdf", ".txt": "text/plain", ".json": "application/json", ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif", ".webp": "image/webp", ".wav": "audio/wav", ".mp3": "audio/mpeg", ".mp4": "video/mp4", ".webm": "video/webm"}[extension]
}
