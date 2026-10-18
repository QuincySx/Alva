// INPUT: context, errors, io, sync/atomic, testing, time, agent/core, kernel/abi, kernel/bus, kernel/core, llm/wire
// OUTPUT: Duplex input/output, joined cancellation and unsupported-middleware rejection probes
// POS: [architecture / executable probes] Demonstrates an optional transport plus replacement loop and explicitly exposes default-loop semantics that are not inherited.
package extensibility_test

import (
	"context"
	"errors"
	"io"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type duplexFactory interface {
	Open(context.Context) *duplexSession
}
type duplexInput struct{ frames <-chan string }

// Transport owns its goroutine. Frames are opaque strings, not an audio codec.
type localDuplex struct {
	accepted        chan string
	closed          chan struct{}
	completionCalls atomic.Int32
	openCalls       atomic.Int32
}

func newLocalDuplex() *localDuplex {
	return &localDuplex{accepted: make(chan string, 4), closed: make(chan struct{})}
}
func (p *localDuplex) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	p.completionCalls.Add(1)
	return finishText(emit, "ordinary request")
}

type duplexSession struct {
	input  chan string
	output chan string
	cancel context.CancelFunc
	done   chan struct{}
}

func (p *localDuplex) Open(parent context.Context) *duplexSession {
	p.openCalls.Add(1)
	ctx, cancel := context.WithCancel(parent)
	session := &duplexSession{input: make(chan string), output: make(chan string), cancel: cancel, done: make(chan struct{})}
	go func() {
		defer close(session.done)
		defer close(p.closed)
		defer close(session.output)
		for {
			select {
			case <-ctx.Done():
				return
			case frame, ok := <-session.input:
				if !ok {
					return
				}
				select {
				case p.accepted <- frame:
				case <-ctx.Done():
					return
				}
				select {
				case session.output <- "echo:" + frame:
				case <-ctx.Done():
					return
				}
			}
		}
	}()
	return session
}

func (s *duplexSession) Send(ctx context.Context, frame string) error {
	select {
	case s.input <- frame:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (s *duplexSession) Receive(ctx context.Context) (string, error) {
	select {
	case <-ctx.Done():
		return "", ctx.Err()
	case frame, ok := <-s.output:
		if !ok {
			return "", io.EOF
		}
		return frame, nil
	}
}
func (s *duplexSession) Close() { s.cancel(); <-s.done }

var errDuplexMiddleware = errors.New("duplex fixture has no standard middleware lifecycle implementation")

type duplexLoop struct{}

func (duplexLoop) Run(parent context.Context, config kernel.RunConfig, inputMessages []wire.Message, emit func(abi.Event) error) error {
	// Replacement of AgentLoop does not inherit the default dispatcher, durable
	// intent/outcome or tool execution. Fail rather than silently skip a policy.
	if len(config.Middleware) != 0 {
		return errDuplexMiddleware
	}
	if len(config.State.Tools().Snapshot().Definitions) != 0 {
		return errors.New("duplex fixture has no standard tool dispatcher")
	}
	if len(inputMessages) != 0 || config.SystemPrompt != "" || config.ContextSystem != nil || config.ContextSource != nil {
		return errors.New("duplex fixture does not implement request context preparation")
	}
	factory, ok := bus.Resolve[duplexFactory](config.Bus)
	if !ok {
		return errors.New("duplex transport missing")
	}
	input, ok := bus.Resolve[duplexInput](config.Bus)
	if !ok {
		return errors.New("duplex input missing")
	}
	ctx, cancel := context.WithCancel(parent)
	session := factory.Open(ctx)
	inputDone := make(chan struct{})
	go func() {
		defer close(inputDone)
		defer close(session.input)
		for {
			select {
			case <-ctx.Done():
				return
			case frame, ok := <-input.frames:
				if !ok {
					return
				}
				if session.Send(ctx, frame) != nil {
					return
				}
			}
		}
	}()
	defer func() { cancel(); <-inputDone; session.Close() }()
	for {
		frame, err := session.Receive(ctx)
		if errors.Is(err, io.EOF) {
			return ctx.Err()
		}
		if err != nil {
			return err
		}
		if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: frame}); err != nil {
			return err
		}
	}
}

func duplexPlugin(provider *localDuplex, frames <-chan string) plugin {
	return plugin{name: "duplex-transport", register: func(_ context.Context, r *core.Registrar) error {
		if err := bus.Provide[duplexFactory](r.BusWriter(), provider); err != nil {
			return err
		}
		return bus.Provide(r.BusWriter(), duplexInput{frames: frames})
	}}
}

func TestDuplexReceivesInputWhileOutputIsBlockedAndJoinsCancel(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	provider := newLocalDuplex()
	// The provider genuinely implements the mandatory request API as well as
	// the optional transport. A duplex-only provider would need an adapter.
	ordinary := build(t, core.NewBuilder(provider, core.NewMemorySession("ordinary")))
	if err := ordinary.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "ordinary")}, discard); err != nil {
		t.Fatal(err)
	}
	frames := make(chan string)
	agent := build(t, core.NewBuilder(provider, core.NewMemorySession("duplex")).Plugin(duplexPlugin(provider, frames)).Loop(duplexLoop{}))
	outputEntered := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		done <- agent.Run(ctx, nil, func(event abi.Event) error {
			if event.Text != "echo:first" {
				return errors.New("unexpected first output")
			}
			close(outputEntered)
			<-ctx.Done()
			return ctx.Err()
		})
	}()
	select {
	case frames <- "first":
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	select {
	case <-outputEntered:
	case <-ctx.Done():
		t.Fatal(ctx.Err())
	}
	select {
	case frames <- "second":
	case <-ctx.Done():
		t.Fatal("input was blocked by output", ctx.Err())
	}
	for _, expected := range []string{"first", "second"} {
		select {
		case actual := <-provider.accepted:
			if actual != expected {
				t.Fatal(actual)
			}
		case <-ctx.Done():
			t.Fatal("transport did not consume concurrent input", ctx.Err())
		}
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("duplex work was not joined")
	}
	select {
	case <-provider.closed:
	default:
		t.Fatal("Run returned while transport was active")
	}
	if provider.completionCalls.Load() != 1 || provider.openCalls.Load() != 1 {
		t.Fatal("duplex was flattened into request calls")
	}
}

func TestDuplexCompletesWithoutDroppingOutput(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	provider := newLocalDuplex()
	frames := make(chan string, 2)
	frames <- "one"
	frames <- "two"
	close(frames)
	agent := build(t, core.NewBuilder(provider, core.NewMemorySession("duplex-complete")).Plugin(duplexPlugin(provider, frames)).Loop(duplexLoop{}))
	var output []string
	if err := agent.Run(ctx, nil, func(event abi.Event) error { output = append(output, event.Text); return nil }); err != nil {
		t.Fatal(err)
	}
	if len(output) != 2 || output[0] != "echo:one" || output[1] != "echo:two" {
		t.Fatal(output)
	}
}

func TestDuplexRejectsUnsupportedPolicyInsteadOfBypassingIt(t *testing.T) {
	provider := newLocalDuplex()
	agent := build(t, core.NewBuilder(provider, core.NewMemorySession("duplex-policy")).Plugin(duplexPlugin(provider, make(chan string))).Loop(duplexLoop{}).Middleware(core.BeforeModel("approval", func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error { return errors.New("denied") })))
	if err := agent.Run(context.Background(), nil, discard); !errors.Is(err, errDuplexMiddleware) {
		t.Fatal(err)
	}
	if provider.openCalls.Load() != 0 {
		t.Fatal("unsupported policy was bypassed")
	}
}

func TestDuplexRejectsUnsupportedRequestFeaturesBeforeOpeningTransport(t *testing.T) {
	for _, name := range []string{"input", "tool", "context-source", "system-prompt"} {
		t.Run(name, func(t *testing.T) {
			provider := newLocalDuplex()
			builder := core.NewBuilder(provider, core.NewMemorySession("duplex-guard")).Plugin(duplexPlugin(provider, make(chan string))).Loop(duplexLoop{})
			var input []wire.Message
			switch name {
			case "input":
				input = []wire.Message{wire.TextMessage(wire.RoleUser, "must not be silently ignored")}
			case "tool":
				builder.Tool(tool{name: "effect", execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
					t.Error("unsupported tool executed")
					return wire.ToolResult{}, nil
				}})
			case "context-source":
				builder.Plugin(viewPlugin(indexedView{}))
			case "system-prompt":
				builder.SystemPrompt("required policy")
			}
			if err := build(t, builder).Run(context.Background(), input, discard); err == nil {
				t.Fatal("unsupported configuration accepted")
			}
			if provider.openCalls.Load() != 0 {
				t.Fatal("transport opened before unsupported configuration was rejected")
			}
		})
	}
}
