// INPUT: context, encoding/json, errors, reflect, strings, sync, testing, time, agent/core, kernel/abi, llm/wire
// OUTPUT: SDK retry isolation, policy exclusions, bounded protocol, admission and actual deadline join regressions
// POS: [L4 retry testing] Exercises real kernel settlement while failed model attempts remain private.
package modelretry

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, request, emit)
}
func config() Config {
	return Config{MaxAttempts: 3, MaxBufferedBytes: 64 << 10, MaxBufferedChunks: 128, Timeout: time.Second, Retryable: func(error) bool { return true }}
}
func build(t *testing.T, model abi.LanguageModel, settings Config, middleware ...abi.Middleware) (*core.Agent, *core.MemorySession) {
	t.Helper()
	plugin, err := New(settings)
	if err != nil {
		t.Fatal(err)
	}
	log := core.NewMemorySession("retry-fixture")
	builder := core.NewBuilder(model, log).Plugin(plugin)
	for _, item := range middleware {
		builder.Middleware(item)
	}
	agent, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	return agent, log
}
func discard(abi.Event) error { return nil }

func TestConfigRequiresExplicitBoundedPolicy(t *testing.T) {
	for _, change := range []func(*Config){
		func(c *Config) { c.MaxAttempts = 0 }, func(c *Config) { c.MaxAttempts = 9 },
		func(c *Config) { c.MaxBufferedBytes = 0 }, func(c *Config) { c.MaxBufferedBytes = 8<<20 + 1 },
		func(c *Config) { c.MaxBufferedChunks = 0 }, func(c *Config) { c.MaxBufferedChunks = 65537 },
		func(c *Config) { c.Timeout = 0 }, func(c *Config) { c.Timeout = 5*time.Minute + 1 },
		func(c *Config) { c.Retryable = nil },
	} {
		settings := config()
		change(&settings)
		if _, err := New(settings); err == nil {
			t.Fatal("invalid policy accepted", settings)
		}
	}
}

func TestPartialFailureOnlyPublishesAndCommitsSuccessfulStream(t *testing.T) {
	transient := errors.New("explicit transient fixture")
	attempts := 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		attempts++
		if attempts == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "abandoned-private"}); err != nil {
				return err
			}
			call := wire.ToolCall{ID: "abandoned-tool", Name: "must-not-execute", Arguments: json.RawMessage(`{}`)}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
				return err
			}
			return transient
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "selected-answer"}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish, ResponseID: "selected-response", FinishReason: "stop"})
	})
	settings := config()
	settings.Retryable = func(err error) bool { return errors.Is(err, transient) }
	agent, log := build(t, model, settings)
	var visible string
	if err := agent.Run(t.Context(), nil, func(event abi.Event) error {
		if event.Kind == abi.EventTextDelta {
			visible += event.Text
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	messages, err := log.Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if attempts != 2 || visible != "selected-answer" || len(messages) != 1 || messages[0].Text() != visible || messages[0].ResponseID != "selected-response" {
		t.Fatal("failed attempt escaped private buffer", attempts, visible, messages)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	for _, record := range records {
		if record.ToolIntent != nil || record.ToolOutcome != nil {
			t.Fatal("private failed tool declaration became executable evidence", record)
		}
	}
}

func TestRetryKeepsCapturedModelWhenLiveModelChanges(t *testing.T) {
	transient := errors.New("retry original model")
	attempts, replacementCalls := 0, 0
	replacement := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		replacementCalls++
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	var agent *core.Agent
	original := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		attempts++
		if attempts == 1 {
			if err := agent.State().SetModel(replacement); err != nil {
				return err
			}
			return transient
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	settings := config()
	settings.Retryable = func(err error) bool { return errors.Is(err, transient) }
	agent, _ = build(t, original, settings)
	if err := agent.Run(t.Context(), nil, discard); err != nil || attempts != 2 || replacementCalls != 0 {
		t.Fatal("retry switched model instead of retaining its captured invocation", attempts, replacementCalls, err)
	}
}

func TestClassifierMustPermitFailureAndCannotOverrideTerminalErrors(t *testing.T) {
	transient := errors.New("transport fixture")
	for _, test := range []struct {
		name       string
		failure    error
		allow      bool
		wantCalls  int
		classifier int
	}{
		{"not permitted", transient, false, 1, 1},
		{"explicitly permitted", transient, true, 2, 1},
		{"cancelled", context.Canceled, true, 1, 0},
		{"deadline", context.DeadlineExceeded, true, 1, 0},
		{"mixed cancellation", errors.Join(transient, context.Canceled), true, 1, 0},
		{"session", &abi.SessionError{Operation: "append_records", SessionID: "fixture", Err: transient}, true, 1, 0},
	} {
		t.Run(test.name, func(t *testing.T) {
			attempts, classified := 0, 0
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				attempts++
				if attempts == 1 {
					return test.failure
				}
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			settings := config()
			settings.Retryable = func(error) bool { classified++; return test.allow }
			agent, _ := build(t, model, settings)
			err := agent.Run(t.Context(), nil, discard)
			if attempts != test.wantCalls || classified != test.classifier || (err == nil) != (test.wantCalls == 2) {
				t.Fatal("classifier overrode explicit retry boundaries", attempts, classified, err)
			}
		})
	}
}

func TestMaximumAttemptsIsTotalAndDoesNotPublishFailedBodies(t *testing.T) {
	attempts := 0
	failure := errors.New("transient fixture")
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		attempts++
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "private"}); err != nil {
			return err
		}
		return failure
	})
	settings := config()
	settings.MaxAttempts = 2
	agent, log := build(t, model, settings)
	published := 0
	err := agent.Run(t.Context(), nil, func(event abi.Event) error {
		if event.Kind == abi.EventTextDelta {
			published++
		}
		return nil
	})
	messages, readErr := log.Messages(t.Context())
	if !errors.Is(err, failure) || readErr != nil || attempts != 2 || published != 0 || len(messages) != 0 {
		t.Fatal("retry count or private failure retention violated", attempts, published, messages, err, readErr)
	}
}

func TestEncodedBytesAndChunkCountRejectBeforePublicationEvenWhenModelIgnoresError(t *testing.T) {
	usageBytes, err := json.Marshal(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &wire.Usage{}})
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		name   string
		chunks []wire.StreamChunk
		bytes  int
		count  int
	}{
		{"large raw text", []wire.StreamChunk{{Kind: wire.ChunkText, Text: strings.Repeat("x", 2048)}}, 1024, 128},
		{"escaped encoded bytes", []wire.StreamChunk{{Kind: wire.ChunkText, Text: strings.Repeat("\x00", 200)}}, 1024, 128},
		{"cumulative usage bytes", []wire.StreamChunk{{Kind: wire.ChunkUsage, Usage: &wire.Usage{}}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{}}}, 2*len(usageBytes) - 1, 128},
		{"usage count", []wire.StreamChunk{{Kind: wire.ChunkUsage, Usage: &wire.Usage{}}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{}}, {Kind: wire.ChunkUsage, Usage: &wire.Usage{}}}, 64 << 10, 2},
		{"empty text count", []wire.StreamChunk{{Kind: wire.ChunkText}, {Kind: wire.ChunkText}, {Kind: wire.ChunkText}}, 64 << 10, 2},
	} {
		t.Run(test.name, func(t *testing.T) {
			attempts, classified, published := 0, 0, 0
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				attempts++
				for _, chunk := range test.chunks {
					_ = emit(chunk)
				}
				_ = emit(wire.StreamChunk{Kind: wire.ChunkFinish})
				return nil
			})
			settings := config()
			settings.MaxBufferedBytes, settings.MaxBufferedChunks = test.bytes, test.count
			settings.Retryable = func(error) bool { classified++; return true }
			agent, _ := build(t, model, settings)
			err := agent.Run(t.Context(), nil, func(event abi.Event) error {
				if event.Kind == abi.EventTextDelta || event.Kind == abi.EventUsage {
					published++
				}
				return nil
			})
			if !errors.Is(err, ErrBufferLimit) || attempts != 1 || classified != 0 || published != 0 {
				t.Fatal("buffer violation was retried or published", attempts, classified, published, err)
			}
		})
	}
}

func TestMalformedOrIncompleteStreamCannotBeClassifiedAsRetryable(t *testing.T) {
	for _, chunk := range []wire.StreamChunk{{Kind: "unknown"}, {Kind: wire.ChunkToolCall}, {Kind: wire.ChunkText, Text: "missing-finish"}} {
		attempts, classified := 0, 0
		model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			attempts++
			_ = emit(chunk)
			return nil
		})
		settings := config()
		settings.Retryable = func(error) bool { classified++; return true }
		agent, _ := build(t, model, settings)
		if err := agent.Run(t.Context(), nil, discard); !errors.Is(err, ErrProtocol) || attempts != 1 || classified != 0 {
			t.Fatal("local protocol error reached retry classifier", chunk.Kind, attempts, classified, err)
		}
	}
}

type admission struct {
	calls int
	deny  error
}

func (*admission) Name() string { return "retry-admission" }
func (a *admission) AdmitModel(_ context.Context, _ abi.RuntimeContext, _ abi.ModelRequest) error {
	a.calls++
	if a.calls == 1 {
		return a.deny
	}
	return nil
}
func TestEveryAttemptHasIndependentAdmissionIncludingRejectedAttempt(t *testing.T) {
	denied := errors.New("explicitly classified admission fixture")
	policy := &admission{deny: denied}
	attempts := 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		attempts++
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	settings := config()
	settings.Retryable = func(err error) bool { return errors.Is(err, denied) }
	agent, _ := build(t, model, settings, policy)
	if err := agent.Run(t.Context(), nil, discard); err != nil || attempts != 1 || policy.calls != 2 {
		t.Fatal("admission was bypassed or denial caused actual IO", attempts, policy.calls, err)
	}
}

func TestDeadlineWaitsForActuallyEnteredModelExit(t *testing.T) {
	cancelled, release, returned := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	var releaseOnce sync.Once
	unblock := func() { releaseOnce.Do(func() { close(release) }) }
	t.Cleanup(unblock)
	attempts, classified := 0, 0
	model := modelFunc(func(ctx context.Context, _ abi.ModelRequest, _ func(wire.StreamChunk) error) error {
		attempts++
		<-ctx.Done()
		close(cancelled)
		<-release // Deliberately ignores cancellation until actual resource exit.
		return nil
	})
	settings := config()
	settings.Timeout = 20 * time.Millisecond
	settings.Retryable = func(error) bool { classified++; return true }
	agent, _ := build(t, model, settings)
	go func() { returned <- agent.Run(context.Background(), nil, discard) }()
	select {
	case <-cancelled:
	case <-time.After(time.Second):
		t.Fatal("model did not receive total deadline")
	}
	select {
	case err := <-returned:
		t.Fatal("wrapper returned while actual model was still running", err)
	default:
	}
	unblock()
	select {
	case err := <-returned:
		if !errors.Is(err, context.DeadlineExceeded) || attempts != 1 || classified != 0 {
			t.Fatal("deadline was retried or hidden", attempts, classified, err)
		}
	case <-time.After(time.Second):
		t.Fatal("model was not joined after release")
	}
}

func TestSelectedChunksPreserveIndexesMetadataAndOwnNestedValues(t *testing.T) {
	plugin, err := New(config())
	if err != nil {
		t.Fatal(err)
	}
	index, cached := 3, uint64(0)
	// Use the actual Usage field type rather than converting through float64.
	usage := wire.Usage{}
	usage.CachedInputTokens = &cached
	block := wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: "signed", Signature: "signature"}}
	var selected []wire.StreamChunk
	late := (func(wire.StreamChunk) error)(nil)
	err = plugin.WrapModel(t.Context(), abi.RuntimeContext{}, abi.ModelRequest{}, func(chunk wire.StreamChunk) error {
		selected = append(selected, wire.CloneStreamChunk(chunk))
		if chunk.Usage != nil {
			*chunk.Usage.CachedInputTokens = 99
		}
		return nil
	}, func(_ context.Context, _ abi.RuntimeContext, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		late = emit
		for _, chunk := range []wire.StreamChunk{{Kind: wire.ChunkText, ContentIndex: &index, Text: "selected"}, {Kind: wire.ChunkReasoning, Text: "progress"}, {Kind: wire.ChunkContentBlock, ContentIndex: intPointer(1), ContentBlock: &block}, {Kind: wire.ChunkUsage, Usage: &usage}, {Kind: wire.ChunkFinish, ResponseID: "response", FinishReason: "stop"}} {
			if err := emit(chunk); err != nil {
				return err
			}
		}
		index, cached, block.Reasoning.Signature = 9, 42, "changed"
		return nil
	})
	if err != nil || len(selected) != 5 || *selected[0].ContentIndex != 3 || selected[1].Text != "progress" || selected[2].ContentBlock.Reasoning.Signature != "signature" || *selected[3].Usage.CachedInputTokens != 0 || selected[4].ResponseID != "response" {
		t.Fatal("selected metadata was changed or lost", selected, err)
	}
	before := append([]wire.StreamChunk(nil), selected...)
	if err := late(wire.StreamChunk{Kind: wire.ChunkText, Text: "late"}); !errors.Is(err, ErrProtocol) || !reflect.DeepEqual(before, selected) {
		t.Fatal("late private emit altered the selected stream", err)
	}
}

func TestRawPreflightRejectsOpaqueFieldsBeforeRetainingAnyChunk(t *testing.T) {
	for _, chunk := range []wire.StreamChunk{
		{Kind: wire.ChunkText, ToolCall: &wire.ToolCall{Arguments: json.RawMessage(strings.Repeat(" ", 2048))}},
		{Kind: wire.ChunkText, ContentBlock: &wire.ContentBlock{ProviderState: &wire.ProviderState{Data: json.RawMessage(strings.Repeat(" ", 2048))}}},
		{Kind: wire.ChunkText, ContentBlock: &wire.ContentBlock{Image: &wire.Image{Payload: wire.MediaPayload{Data: make([]byte, 2048)}}}},
	} {
		buffer := newAttemptBuffer(Config{MaxBufferedBytes: 1024, MaxBufferedChunks: 4})
		if err := buffer.push(chunk); !errors.Is(err, ErrBufferLimit) || len(buffer.chunks) != 0 || buffer.bytes != 0 {
			t.Fatal("unused oversized opaque fields escaped raw preflight", err, len(buffer.chunks), buffer.bytes)
		}
	}
}

func TestEachAttemptReceivesAnIndependentRequest(t *testing.T) {
	plugin, err := New(config())
	if err != nil {
		t.Fatal(err)
	}
	request := abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleUser, "original")}, Tools: []wire.ToolDefinition{{Name: "tool", Parameters: json.RawMessage(`{"type":"object"}`)}}, Config: wire.ModelConfig{ExtraBody: json.RawMessage(`{"a":1}`)}}
	attempts := 0
	err = plugin.WrapModel(t.Context(), abi.RuntimeContext{}, request, func(wire.StreamChunk) error { return nil }, func(_ context.Context, _ abi.RuntimeContext, candidate abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		attempts++
		if candidate.Messages[0].Text() != "original" || string(candidate.Tools[0].Parameters) != `{"type":"object"}` || string(candidate.Config.ExtraBody) != `{"a":1}` {
			t.Error("a failed attempt mutated the next request", candidate)
		}
		if attempts == 1 {
			candidate.Messages[0].Content[0].Text = "changed"
			candidate.Tools[0].Parameters[0] = 'x'
			candidate.Config.ExtraBody[0] = 'x'
			return errors.New("retry explicitly permitted")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	if err != nil || attempts != 2 || request.Messages[0].Text() != "original" {
		t.Fatal("failed request was retained for retry", attempts, request, err)
	}
}

func TestPublicationFailureIsNeverRetried(t *testing.T) {
	attempts, classified := 0, 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		attempts++
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "selected"}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	settings := config()
	settings.Retryable = func(error) bool { classified++; return true }
	agent, _ := build(t, model, settings)
	unavailable := errors.New("subscriber unavailable")
	err := agent.Run(t.Context(), nil, func(event abi.Event) error {
		if event.Kind == abi.EventTextDelta {
			return unavailable
		}
		return nil
	})
	if !errors.Is(err, unavailable) || attempts != 1 || classified != 0 {
		t.Fatal("publication failure was retried", attempts, classified, err)
	}
}
func intPointer(value int) *int { return &value }
