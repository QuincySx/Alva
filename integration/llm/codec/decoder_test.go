// INPUT: Pure complete native SSE data fixtures, cancellation and bounded synchronous consumers
// OUTPUT: Four-protocol terminal, failure, budgets, cache reporting and ownership regressions
// POS: [integration / protocol codec] No network/provider/credentials; EOF never manufactures a successful model finish.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func decoderEvent(raw string) Event { return Event{Data: []byte(raw)} }
func basicDecoderEvents(protocol Protocol, cache string) []Event {
	switch protocol {
	case OpenAIChat:
		usage := `{"prompt_tokens":2,"completion_tokens":1}`
		if cache != "" {
			usage = `{"prompt_tokens":2,"completion_tokens":1,"prompt_tokens_details":{"cached_tokens":` + cache + `}}`
		}
		return []Event{decoderEvent(`{"id":"reply","choices":[{"index":0,"delta":{"content":"hello"}}]}`), decoderEvent(`{"choices":[{"index":0,"finish_reason":"stop"}],"usage":` + usage + `}`), decoderEvent(`[DONE]`)}
	case OpenAIResponses:
		usage := `{"input_tokens":2,"output_tokens":1}`
		if cache != "" {
			usage = `{"input_tokens":2,"output_tokens":1,"input_tokens_details":{"cached_tokens":` + cache + `,"cache_write_tokens":` + cache + `}}`
		}
		return []Event{decoderEvent(`{"type":"response.output_text.done","text":"hello"}`), decoderEvent(`{"type":"response.completed","response":{"id":"reply","status":"completed","usage":` + usage + `}}`)}
	case Anthropic:
		usage := `{"input_tokens":2,"output_tokens":1}`
		if cache != "" {
			usage = `{"input_tokens":2,"output_tokens":1,"cache_read_input_tokens":` + cache + `,"cache_creation_input_tokens":` + cache + `}`
		}
		return []Event{decoderEvent(`{"type":"message_start","message":{"id":"reply","role":"assistant","usage":` + usage + `}}`), decoderEvent(`{"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hello"}}`), decoderEvent(`{"type":"content_block_stop","index":0}`), decoderEvent(`{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}`), decoderEvent(`{"type":"message_stop"}`)}
	case Gemini:
		usage := `{"promptTokenCount":2,"candidatesTokenCount":1}`
		if cache != "" {
			usage = `{"promptTokenCount":2,"candidatesTokenCount":1,"cachedContentTokenCount":` + cache + `}`
		}
		return []Event{decoderEvent(`{"responseId":"reply","candidates":[{"index":0,"content":{"parts":[{"text":"hello"}]},"finishReason":"STOP"}],"usageMetadata":` + usage + `}`)}
	}
	panic("unsupported fixture protocol")
}

func TestDecoderNativeTerminalsHoldFinishUntilExplicitFinalization(t *testing.T) {
	for _, protocol := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(protocol), func(t *testing.T) {
			d, err := NewDecoder(protocol, Limits{})
			if err != nil {
				t.Fatal(err)
			}
			a := wire.NewStreamAccumulator(1 << 20)
			finishes := 0
			emit := func(c wire.StreamChunk) error {
				if c.Kind == wire.ChunkFinish {
					finishes++
				}
				return a.Push(c)
			}
			for _, e := range basicDecoderEvents(protocol, "") {
				if err = d.Push(t.Context(), e, emit); err != nil {
					t.Fatal(err)
				}
			}
			if !d.Terminal() || finishes != 0 {
				t.Fatal("Push manufactured finish or missed native terminal", d.Terminal(), finishes)
			}
			if err = d.Finish(t.Context(), emit); err != nil {
				t.Fatal(err)
			}
			message, err := a.Finish()
			if err != nil || message.Text() != "hello" || message.ResponseID != "reply" || finishes != 1 {
				t.Fatal("lost native response", message, finishes, err)
			}
			if d.Finish(t.Context(), emit) == nil || finishes != 1 {
				t.Fatal("finish replayed")
			}
		})
	}
}

func TestDecoderCacheReportingPresenceIsNativeNotNormalizedZero(t *testing.T) {
	for _, protocol := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		for _, presence := range []string{"", "0", "3"} {
			t.Run(string(protocol)+"/"+presence, func(t *testing.T) {
				d, _ := NewDecoder(protocol, Limits{})
				var usage *wire.Usage
				emit := func(c wire.StreamChunk) error {
					if c.Usage != nil {
						v := wire.CloneUsage(*c.Usage)
						usage = &v
					}
					return nil
				}
				for _, e := range basicDecoderEvents(protocol, presence) {
					if err := d.Push(t.Context(), e, emit); err != nil {
						t.Fatal(err)
					}
				}
				if err := d.Finish(t.Context(), emit); err != nil {
					t.Fatal(err)
				}
				if usage == nil || (usage.CachedInputTokens == nil) != (presence == "") {
					t.Fatal("cache presence lost", usage)
				}
				if presence != "" {
					var value uint64
					_ = json.Unmarshal([]byte(presence), &value)
					if *usage.CachedInputTokens != value {
						t.Fatal("cache count changed", usage)
					}
				}
				if protocol == Anthropic || protocol == OpenAIResponses {
					if (usage.CacheWriteInputTokens == nil) != (presence == "") {
						t.Fatal("write presence lost", usage)
					}
				} else if usage.CacheWriteInputTokens != nil {
					t.Fatal("unreported writes invented", usage)
				}
			})
		}
	}
}

func TestDecoderMissingNativeTerminalAndFailureAreSticky(t *testing.T) {
	for _, protocol := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(protocol), func(t *testing.T) {
			d, _ := NewDecoder(protocol, Limits{})
			finishes := 0
			emit := func(c wire.StreamChunk) error {
				if c.Kind == wire.ChunkFinish {
					finishes++
				}
				return nil
			}
			events := basicDecoderEvents(protocol, "")
			for _, e := range events[:len(events)-1] {
				if err := d.Push(t.Context(), e, emit); err != nil {
					t.Fatal(err)
				}
			}
			failure := d.Finish(t.Context(), emit)
			if failure == nil || finishes != 0 || d.Terminal() {
				t.Fatal("EOF became successful finish")
			}
			if got := d.Push(t.Context(), events[len(events)-1], emit); got != failure {
				t.Fatal("failed stream revived", failure, got)
			}
		})
	}
}

func TestDecoderLimitsUnicodeCancellationAndConsumerFailure(t *testing.T) {
	for _, limits := range []Limits{{MaxEventBytes: -1}, {MaxEventBytes: defaultEventBytes + 1}, {MaxResponseBytes: defaultResponseBytes + 1}, {MaxToolCalls: defaultToolCalls + 1}, {MaxArgumentBytes: defaultArgumentBytes + 1}} {
		if _, err := NewDecoder(OpenAIChat, limits); err == nil {
			t.Fatal("limits enlarged", limits)
		}
	}
	if _, err := NewDecoder("not-a-protocol", Limits{}); err == nil {
		t.Fatal("unknown protocol")
	}
	for _, protocol := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		for _, raw := range []string{`null`, `[]`, `"not an event"`} {
			d, _ := NewDecoder(protocol, Limits{})
			if err := d.Push(t.Context(), decoderEvent(raw), func(wire.StreamChunk) error { return nil }); err == nil {
				t.Fatal("non-object native event accepted", protocol, raw)
			}
		}
	}
	for _, raw := range []string{"{\"bad\":\"\xff\"}", `{"bad":"\ud800"}`, `{"duplicate":1,"duplicate":2}`, "[DONE]"} {
		d, _ := NewDecoder(Anthropic, Limits{})
		if err := d.Push(t.Context(), decoderEvent(raw), func(wire.StreamChunk) error { return nil }); err == nil {
			t.Fatal("invalid JSON accepted", raw)
		}
	}
	d, _ := NewDecoder(OpenAIChat, Limits{MaxEventBytes: 8})
	if d.Push(t.Context(), decoderEvent(strings.Repeat("x", 9)), func(wire.StreamChunk) error { return nil }) == nil {
		t.Fatal("event budget ignored")
	}
	d, _ = NewDecoder(Gemini, Limits{MaxResponseBytes: 8})
	if d.Push(t.Context(), decoderEvent(`{"too":"large"}`), func(wire.StreamChunk) error { return nil }) == nil {
		t.Fatal("response budget ignored")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	d, _ = NewDecoder(OpenAIChat, Limits{})
	if err := d.Push(ctx, basicDecoderEvents(OpenAIChat, "")[0], func(wire.StreamChunk) error { t.Error("cancelled admission emitted"); return nil }); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	d, _ = NewDecoder(OpenAIChat, Limits{})
	sentinel := errors.New("consumer failed")
	if err := d.Push(t.Context(), basicDecoderEvents(OpenAIChat, "")[0], func(wire.StreamChunk) error { return sentinel }); !errors.Is(err, sentinel) {
		t.Fatal("consumer failure lost", err)
	}
	if err := d.Finish(t.Context(), func(wire.StreamChunk) error { t.Error("failed stream emitted finish"); return nil }); !errors.Is(err, sentinel) {
		t.Fatal(err)
	}
}

func TestGeminiUsageOnlyAfterSTOPUpdatesExplicitZeroWithoutNewFinish(t *testing.T) {
	d, _ := NewDecoder(Gemini, Limits{})
	var usage *wire.Usage
	finishes := 0
	emit := func(c wire.StreamChunk) error {
		if c.Usage != nil {
			usage = c.Usage
		}
		if c.Kind == wire.ChunkFinish {
			finishes++
		}
		return nil
	}
	if err := d.Push(t.Context(), basicDecoderEvents(Gemini, "3")[0], emit); err != nil {
		t.Fatal(err)
	}
	if err := d.Push(t.Context(), decoderEvent(`{"usageMetadata":{"cachedContentTokenCount":0}}`), emit); err != nil {
		t.Fatal(err)
	}
	if usage == nil || usage.CachedInputTokens == nil || *usage.CachedInputTokens != 0 || finishes != 0 {
		t.Fatal("explicit zero lost", usage, finishes)
	}
	if err := d.Finish(t.Context(), emit); err != nil || finishes != 1 {
		t.Fatal(err, finishes)
	}
}

func TestDecoderGeminiUsageTailCanSupplyFirstResponseIdentity(t *testing.T) {
	d, _ := NewDecoder(Gemini, Limits{})
	var chunks []wire.StreamChunk
	emit := func(c wire.StreamChunk) error { chunks = append(chunks, c); return nil }
	for _, raw := range []string{`{"candidates":[{"content":{"parts":[{"text":"answer"}]},"finishReason":"STOP"}]}`, `{"responseId":"tail-id","usageMetadata":{"promptTokenCount":2}}`} {
		if err := d.Push(t.Context(), decoderEvent(raw), emit); err != nil {
			t.Fatal(err)
		}
	}
	if err := d.Finish(t.Context(), emit); err != nil {
		t.Fatal(err)
	}
	finishes := 0
	for _, c := range chunks {
		if c.Kind == wire.ChunkFinish {
			finishes++
			if c.ResponseID != "tail-id" {
				t.Fatal("late response identity lost", c.ResponseID)
			}
		}
	}
	if finishes != 1 {
		t.Fatal("tail metadata replayed finish", finishes)
	}
}

func TestDecoderRejectsDuplicateKeysAndDeepNativeEventsBeforeEmission(t *testing.T) {
	for _, raw := range []string{
		`{"choices":[{"delta":{"content":"first","content":"last"},"finish_reason":"stop"}]}`,
		`{"choices":[{"finish_reason":"stop"}],"metadata":` + strings.Repeat("[", 130) + "0" + strings.Repeat("]", 130) + `}`,
	} {
		d, _ := NewDecoder(OpenAIChat, Limits{})
		if err := d.Push(t.Context(), decoderEvent(raw), func(wire.StreamChunk) error { t.Error("ambiguous/deep native event reached consumer"); return nil }); err == nil {
			t.Fatal("ambiguous/deep native event accepted")
		}
	}
}
