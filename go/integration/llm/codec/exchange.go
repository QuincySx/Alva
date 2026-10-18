// INPUT: Explicit native protocols/options, bounded raw JSON and the absorbed Magpie request/response conversion contracts.
// OUTPUT: Options, Exchange, NewExchange and one-request conversion with private request-scoped metadata.
// POS: [integration / protocol codec] Raw protocol conversion calls the pinned Magpie fork without canonical Chat round trips or exposing third-party business DTOs.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/integration/llm/codec/internal/magpie"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

// Model is the actual upstream model; ResponseModel is the caller-visible
// route alias. Neither is a credential or a provider/account identifier.
type Options struct {
	Model, ResponseModel   string
	DefaultMaxOutputTokens int
	Stream                 bool
	Limits                 Limits
}

type Exchange struct {
	mu                   sync.Mutex
	inbound, upstream    Protocol
	options              Options
	request              *magpie.Request
	requested, responded bool
	requestFailure       error
}

func NewExchange(inbound, upstream Protocol, options Options) (*Exchange, error) {
	if _, err := NewProtocolAdapter(inbound); err != nil {
		return nil, err
	}
	if _, err := NewProtocolAdapter(upstream); err != nil {
		return nil, err
	}
	if options.ResponseModel == "" {
		options.ResponseModel = options.Model
	}
	for _, model := range []string{options.Model, options.ResponseModel} {
		if strings.TrimSpace(model) == "" || len(model) > 256 || !utf8.ValidString(model) || strings.ContainsRune(model, 0) {
			return nil, errors.New("explicit bounded conversion model required")
		}
	}
	if options.DefaultMaxOutputTokens < 0 || options.DefaultMaxOutputTokens > 1<<20 {
		return nil, errors.New("conversion output ceiling exceeds supported bounds")
	}
	limits, err := decoderLimits(options.Limits)
	if err != nil {
		return nil, err
	}
	options.Limits = limits
	return &Exchange{inbound: inbound, upstream: upstream, options: options}, nil
}

// Request is single-use even on failure; response state must never leak across
// attempts or requests. Same-protocol JSON keeps opaque native fields intact.
func (e *Exchange) Request(ctx context.Context, raw json.RawMessage) (result Result, err error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.requested {
		return Result{}, errors.New("conversion request already consumed")
	}
	e.requested = true
	defer func() {
		if err != nil {
			e.requestFailure = err
		}
	}()
	if err = ctx.Err(); err != nil {
		return Result{}, err
	}
	object, err := rawObject(raw, e.options.Limits.MaxResponseBytes)
	if err != nil {
		return Result{}, err
	}
	if err = continuationGuard(e.inbound, e.upstream, raw); err != nil {
		return Result{}, err
	}
	if e.inbound != Gemini {
		for key := range object {
			if key != "stream" && strings.EqualFold(key, "stream") {
				return Result{}, errors.New("noncanonical native stream control key")
			}
		}
		stream := false
		if value, ok := object["stream"]; ok {
			if string(value) == "null" || json.Unmarshal(value, &stream) != nil {
				return Result{}, errors.New("native request stream field must be boolean")
			}
		}
		if stream != e.options.Stream {
			return Result{}, errors.New("native request stream mode differs from exchange")
		}
	}
	if e.inbound == e.upstream {
		body, err := patchModel(raw, e.upstream, e.options.Model, false)
		return Result{Body: body, From: e.inbound, To: e.upstream, Converter: "native_identity", Quality: "identity"}, err
	}
	if err = validateRequestTools(e.inbound, e.upstream, raw, e.options.Limits); err != nil {
		return Result{}, err
	}
	if err = checkDTOKeys(raw, reflect.TypeOf(magpie.RequestShape(string(e.inbound)))); err != nil {
		return Result{}, err
	}
	request, err := guarded(func() (*magpie.Request, error) { return magpie.ParseRequest(string(e.inbound), raw) })
	if err != nil {
		return Result{}, conversionError(err)
	}
	e.request = request
	sourceIntent := *request
	body, err := guarded(func() ([]byte, error) {
		return magpie.BuildRequest(string(e.upstream), request, e.options.Model, e.options.Stream, e.options.DefaultMaxOutputTokens)
	})
	if err != nil {
		return Result{}, conversionError(err)
	}
	if err = validateNativeJSON(body, e.options.Limits.MaxResponseBytes); err != nil {
		return Result{}, err
	}
	if _, err = guarded(func() (struct{}, error) {
		return struct{}{}, validateMagpieRequest(e.inbound, e.upstream, raw, body, &sourceIntent)
	}); err != nil {
		return Result{}, conversionError(err)
	}
	result = conversionResult(e.inbound, e.upstream)
	if err = validateNativeJSON(body, e.options.Limits.MaxResponseBytes); err != nil {
		return Result{}, err
	}
	result.Body, err = patchModel(body, e.upstream, e.options.Model, false)
	if err != nil {
		return Result{}, err
	}
	if err = ctx.Err(); err != nil {
		return Result{}, err
	}
	return result, nil
}

func (e *Exchange) Response(ctx context.Context, raw json.RawMessage) (Result, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if !e.requested || e.requestFailure != nil || e.responded || e.options.Stream {
		return Result{}, errors.New("conversion response has no eligible request")
	}
	e.responded = true
	if err := ctx.Err(); err != nil {
		return Result{}, err
	}
	if err := validateComplete(e.upstream, e.inbound, raw, e.options.Limits); err != nil {
		return Result{}, err
	}
	if err := continuationGuard(e.upstream, e.inbound, raw); err != nil {
		return Result{}, err
	}
	if e.inbound == e.upstream {
		body, err := patchModel(raw, e.inbound, e.options.ResponseModel, true)
		return Result{Body: body, From: e.upstream, To: e.inbound, Converter: "native_identity", Quality: "identity"}, err
	}
	if err := checkDynamicNativeKeys(raw); err != nil {
		return Result{}, err
	}
	frames, err := guarded(func() ([]magpie.Frame, error) { return magpie.CompletionEvents(string(e.upstream), raw) })
	if err != nil {
		return Result{}, conversionError(err)
	}
	decoder, err := NewDecoder(e.upstream, e.options.Limits)
	if err != nil {
		return Result{}, err
	}
	// A complete body has no SSE event limit. The derived native events repeat
	// some envelope/items; original JSON and assembled authority retain 8 MiB.
	completeDecoder := decoder.(*nativeDecoder)
	completeDecoder.limits.MaxEventBytes = e.options.Limits.MaxResponseBytes
	completeDecoder.limits.MaxResponseBytes = 4 * e.options.Limits.MaxResponseBytes
	accumulator := wire.NewStreamAccumulator(e.options.Limits.MaxResponseBytes)
	var usage *wire.Usage
	collect := func(chunk wire.StreamChunk) error {
		if err := accumulator.Push(chunk); err != nil {
			return err
		}
		if chunk.Kind == wire.ChunkUsage && chunk.Usage != nil {
			value := wire.CloneUsage(*chunk.Usage)
			usage = &value
		}
		return nil
	}
	for _, frame := range frames {
		if err = decoder.Push(ctx, Event{Name: frame.Name, Data: frame.Data}, collect); err != nil {
			return Result{}, err
		}
	}
	if err = decoder.Finish(ctx, collect); err != nil {
		return Result{}, err
	}
	message, err := accumulator.Finish()
	if err != nil {
		return Result{}, err
	}
	converted, err := magpieMessage(message)
	if err != nil {
		return Result{}, err
	}
	body, err := guarded(func() ([]byte, error) {
		return magpie.Render(string(e.inbound), converted, e.options.ResponseModel, e.request)
	})
	if err != nil {
		return Result{}, conversionError(err)
	}
	result := conversionResult(e.upstream, e.inbound)
	if err = validateNativeJSON(body, e.options.Limits.MaxResponseBytes); err != nil {
		return Result{}, err
	}
	result.Body = body
	if usage != nil {
		result.Body, err = projectUsage(e.inbound, result.Body, *usage)
		if err != nil {
			return Result{}, err
		}
	}
	var presence cachePresence
	if err = presence.observe(e.upstream, raw); err != nil {
		return Result{}, err
	}
	result.Body, err = presence.apply(e.inbound, result.Body)
	if err != nil {
		return Result{}, err
	}
	result.Body, err = foreignModel(result.Body, e.inbound, e.options.ResponseModel)
	if err != nil {
		return Result{}, err
	}
	if err := ctx.Err(); err != nil {
		return Result{}, err
	}
	return result, nil
}

func (e *Exchange) NewResponseStream() (*ResponseStream, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if !e.requested || e.requestFailure != nil || e.responded || !e.options.Stream {
		return nil, errors.New("conversion stream has no eligible request")
	}
	e.responded = true
	decoder, err := NewDecoder(e.upstream, e.options.Limits)
	if err != nil {
		return nil, err
	}
	stream := &ResponseStream{exchange: e, decoder: decoder, accumulator: wire.NewStreamAccumulator(e.options.Limits.MaxResponseBytes)}
	if e.inbound != e.upstream {

		stream.state, err = magpie.NewEncoder(string(e.inbound), e.options.ResponseModel, e.request, func(frame magpie.Frame) error { return stream.deliverConverted(frame) })
		if err != nil {
			return nil, conversionError(err)
		}
	}
	return stream, nil
}
