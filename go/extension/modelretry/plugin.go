// INPUT: context, encoding/json, errors, fmt, reflect, sync, time, agent/core, kernel/abi, llm/wire
// OUTPUT: Config, Plugin, New, ErrBufferLimit and ErrProtocol for explicit bounded model retries
// POS: [L4 extension] Buffers failed attempts privately and publishes only one complete stream from the captured model.
package modelretry

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

var ErrBufferLimit = errors.New("model retry buffer limit exceeded")
var ErrProtocol = errors.New("invalid buffered model stream")

// Config is explicit policy; Retryable must opt individual failures into retry.
// Timeout covers every attempt and final publication, not each attempt anew.
// Byte accounting is the sum of JSON-encoded chunks, not a physical heap limit.
type Config struct {
	MaxAttempts       int
	MaxBufferedBytes  int
	MaxBufferedChunks int
	Timeout           time.Duration
	Retryable         func(error) bool
}

type Plugin struct{ config Config }

func New(config Config) (*Plugin, error) {
	if err := validateConfig(config); err != nil {
		return nil, err
	}
	return &Plugin{config: config}, nil
}

func validateConfig(config Config) error {
	if config.MaxAttempts < 1 || config.MaxAttempts > 8 || config.MaxBufferedBytes < 1 || config.MaxBufferedBytes > 8<<20 || config.MaxBufferedChunks < 1 || config.MaxBufferedChunks > 65536 || config.Timeout <= 0 || config.Timeout > 5*time.Minute || config.Retryable == nil {
		return errors.New("model retry requires attempts 1..8, bytes 1..8 MiB, chunks 1..65536, timeout within five minutes and an explicit classifier")
	}
	return nil
}
func (*Plugin) Name() string { return "model-retry" }
func (p *Plugin) Register(_ context.Context, registrar *core.Registrar) error {
	if err := validateConfig(p.config); err != nil {
		return err
	}
	return registrar.Middleware(p)
}

// WrapModel never replaces runtime.InvocationModel or executes work in detached
// goroutines. A deadline requests cancellation; an entered next/emit must return
// before this wrapper returns. Publication failure is never retried.
func (p *Plugin) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if err := validateConfig(p.config); err != nil {
		return err
	}
	deadline, cancel := context.WithTimeout(ctx, p.config.Timeout)
	defer cancel()
	for attempt := 0; attempt < p.config.MaxAttempts; attempt++ {
		if err := deadline.Err(); err != nil {
			return err
		}
		buffer := newAttemptBuffer(p.config)
		err := next(deadline, runtime, cloneRequest(request), buffer.push)
		chunks, validationErr := buffer.close(err == nil)
		err = errors.Join(err, validationErr, deadline.Err())
		if err == nil {
			for _, chunk := range chunks {
				if err := deadline.Err(); err != nil {
					return err
				}
				if err := emit(wire.CloneStreamChunk(chunk)); err != nil {
					return err
				}
			}
			return deadline.Err()
		}
		if attempt+1 == p.config.MaxAttempts || terminal(err) || !p.config.Retryable(err) {
			return err
		}
	}
	return errors.New("model retry exhausted")
}

func terminal(err error) bool {
	var session *abi.SessionError
	return errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) || errors.As(err, &session) || errors.Is(err, ErrBufferLimit) || errors.Is(err, ErrProtocol)
}
func cloneRequest(request abi.ModelRequest) abi.ModelRequest {
	request.Messages = wire.CloneMessages(request.Messages)
	request.Config = wire.CloneModelConfig(request.Config)
	request.Tools = append([]wire.ToolDefinition(nil), request.Tools...)
	for i := range request.Tools {
		request.Tools[i].Parameters = append(json.RawMessage(nil), request.Tools[i].Parameters...)
	}
	return request
}

type attemptBuffer struct {
	mu          sync.Mutex
	open        bool
	bytes       int
	config      Config
	chunks      []wire.StreamChunk
	accumulator *wire.StreamAccumulator
	err         error
}

func newAttemptBuffer(config Config) *attemptBuffer {
	return &attemptBuffer{open: true, config: config, accumulator: wire.NewStreamAccumulator(config.MaxBufferedBytes)}
}
func (buffer *attemptBuffer) push(chunk wire.StreamChunk) error {
	buffer.mu.Lock()
	defer buffer.mu.Unlock()
	if !buffer.open {
		return fmt.Errorf("%w: callback after attempt returned", ErrProtocol)
	}
	if buffer.err != nil {
		return buffer.err
	}
	remaining := buffer.config.MaxBufferedBytes - buffer.bytes
	if len(buffer.chunks) >= buffer.config.MaxBufferedChunks || !fitsRawBudget(chunk, remaining) {
		buffer.err = ErrBufferLimit
		return buffer.err
	}
	encoded, err := json.Marshal(chunk)
	if err != nil {
		buffer.err = fmt.Errorf("%w: chunk encoding failed", ErrProtocol)
		return buffer.err
	}
	if len(encoded) > remaining {
		buffer.err = ErrBufferLimit
		return buffer.err
	}
	if err = buffer.accumulator.Push(chunk); err != nil {
		buffer.err = fmt.Errorf("%w: %v", ErrProtocol, err)
		return buffer.err
	}
	buffer.bytes += len(encoded)
	buffer.chunks = append(buffer.chunks, wire.CloneStreamChunk(chunk))
	return nil
}
func (buffer *attemptBuffer) close(complete bool) ([]wire.StreamChunk, error) {
	buffer.mu.Lock()
	defer buffer.mu.Unlock()
	buffer.open = false
	if buffer.err == nil && complete {
		if _, err := buffer.accumulator.Finish(); err != nil {
			buffer.err = fmt.Errorf("%w: %v", ErrProtocol, err)
		}
	}
	buffer.accumulator = nil
	chunks := buffer.chunks
	buffer.chunks = nil // A retained late callback must not retain response bodies.
	if buffer.err != nil || !complete {
		return nil, buffer.err
	}
	return chunks, nil
}

// Reject oversized raw fields, excessive nesting and large empty collections
// before encoding or retaining a deep clone. JSON escaping/base64 expansion is
// checked separately by the encoded-byte budget. This includes fields a chunk
// kind may not use, so opaque/provider/media payloads cannot bypass the bound.
func fitsRawBudget(chunk wire.StreamChunk, limit int) bool {
	nodes := 65536
	var walk func(reflect.Value, int) bool
	walk = func(value reflect.Value, depth int) bool {
		if !value.IsValid() {
			return true
		}
		nodes--
		if nodes < 0 || depth > 64 {
			return false
		}
		switch value.Kind() {
		case reflect.Pointer, reflect.Interface:
			return value.IsNil() || walk(value.Elem(), depth+1)
		case reflect.String:
			limit -= value.Len()
		case reflect.Slice:
			if value.Type().Elem().Kind() == reflect.Uint8 {
				limit -= value.Len()
				break
			}
			if value.Len() > nodes {
				return false
			}
			for i := 0; i < value.Len(); i++ {
				if !walk(value.Index(i), depth+1) {
					return false
				}
			}
		case reflect.Struct:
			for i := 0; i < value.NumField(); i++ {
				if !walk(value.Field(i), depth+1) {
					return false
				}
			}
		case reflect.Map:
			if value.Len() > nodes {
				return false
			}
			entries := value.MapRange()
			for entries.Next() {
				if !walk(entries.Key(), depth+1) || !walk(entries.Value(), depth+1) {
					return false
				}
			}
		}
		return limit >= 0
	}
	return walk(reflect.ValueOf(chunk), 0)
}
