// INPUT: Four native SSE fixture sequences and real request-scoped Magpie stream state.
// OUTPUT: Native-terminal-gated conversion, truncation/callback/cancellation and tail-usage regressions.
// POS: [integration / protocol codec] Successful forwarding must survive an actual destination decoder, not only produce a header or connection.
package codec

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

var streamFixtures = map[Protocol][]string{
	OpenAIChat:      {`{"id":"r","model":"actual","choices":[{"index":0,"delta":{"role":"assistant","content":"reply"},"finish_reason":null}]}`, `{"id":"r","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}`, `{"id":"r","choices":[],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}`, `[DONE]`},
	OpenAIResponses: {`{"type":"response.created","response":{"id":"r","model":"actual","status":"in_progress","output":[]}}`, `{"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"m","role":"assistant","content":[]}}`, `{"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"reply"}`, `{"type":"response.output_text.done","output_index":0,"content_index":0,"text":"reply"}`, `{"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":"reply","annotations":[]}]}}`, `{"type":"response.completed","response":{"id":"r","model":"actual","status":"completed","output":[{"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":"reply","annotations":[]}]}],"usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5}}}`},
	Anthropic:       {`{"type":"message_start","message":{"id":"r","type":"message","model":"actual","role":"assistant","content":[],"usage":{"input_tokens":3,"output_tokens":0}}}`, `{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}`, `{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"reply"}}`, `{"type":"content_block_stop","index":0}`, `{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}`, `{"type":"message_stop"}`},
	Gemini:          {`{"responseId":"r","modelVersion":"actual-version","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"reply"}]}}]}`, `{"candidates":[{"index":0,"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"totalTokenCount":5}}`},
}

func newStreamFixture(t *testing.T, inbound, upstream Protocol) *ResponseStream {
	t.Helper()
	exchange, err := NewExchange(inbound, upstream, Options{Model: "actual", ResponseModel: "alias", DefaultMaxOutputTokens: 1024, Stream: true})
	if err != nil {
		t.Fatal(err)
	}
	request := requestFixtures[inbound]
	if inbound != Gemini {
		request = strings.TrimSuffix(request, "}") + `,"stream":true}`
	}
	if _, err = exchange.Request(context.Background(), []byte(request)); err != nil {
		t.Fatal(err)
	}
	stream, err := exchange.NewResponseStream()
	if err != nil {
		t.Fatal(err)
	}
	return stream
}
func TestConvertedStreamNativeDestinationActuallyCompletes(t *testing.T) {
	for inbound := range streamFixtures {
		for upstream, fixtures := range streamFixtures {
			t.Run(string(inbound)+"_"+string(upstream), func(t *testing.T) {
				stream := newStreamFixture(t, inbound, upstream)
				var events []Event
				emit := func(event Event) error { events = append(events, event); return nil }
				for _, raw := range fixtures {
					if err := stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
						t.Fatal(err)
					}
				}
				if !stream.Terminal() {
					t.Fatal("no real source terminal")
				}
				for _, event := range events {
					if terminalEvent(inbound, event) {
						t.Fatal("destination terminal emitted before Finish")
					}
				}
				if err := stream.Finish(context.Background(), emit); err != nil {
					t.Fatal(err)
				}
				if inbound == Gemini && upstream != Gemini {
					for _, event := range events {
						if strings.Contains(string(event.Data), `"modelVersion"`) {
							t.Fatalf("cross stream invented Gemini modelVersion: %s", event.Data)
						}
					}
				}
				decoder, _ := NewDecoder(inbound, Limits{})
				acc := wire.NewStreamAccumulator(responseLimit)
				for _, event := range events {
					if err := decoder.Push(context.Background(), event, acc.Push); err != nil {
						t.Fatalf("invalid destination %s: %v", event.Data, err)
					}
				}
				if err := decoder.Finish(context.Background(), acc.Push); err != nil {
					t.Fatalf("destination incomplete: %v events=%#v", err, events)
				}
				message, err := acc.Finish()
				if err != nil {
					t.Fatal(err)
				}
				if len(message.Content) == 0 || message.Content[0].Text != "reply" {
					t.Fatalf("reply changed: %#v", message)
				}
				if err = stream.Finish(context.Background(), emit); err == nil {
					t.Fatal("stream finalized twice")
				}
			})
		}
	}
}
func TestConvertedStreamNeverFinalizesTruncatedSource(t *testing.T) {
	for source, fixtures := range streamFixtures {
		t.Run(string(source), func(t *testing.T) {
			stream := newStreamFixture(t, OpenAIChat, source)
			var events []Event
			emit := func(event Event) error { events = append(events, event); return nil }
			for _, raw := range fixtures[:len(fixtures)-1] {
				if err := stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
					t.Fatal(err)
				}
			}
			if err := stream.Finish(context.Background(), emit); err == nil {
				t.Fatal("EOF synthesized success")
			}
			for _, event := range events {
				if terminalEvent(OpenAIChat, event) {
					t.Fatalf("truncated stream emitted success: %s", event.Data)
				}
			}
		})
	}
}
func TestConvertedStreamCallbackAndCancelRemainSticky(t *testing.T) {
	stream := newStreamFixture(t, OpenAIChat, OpenAIChat)
	failure := errors.New("consumer unavailable")
	if err := stream.Push(context.Background(), Event{Data: []byte(streamFixtures[OpenAIChat][0])}, func(Event) error { return failure }); !errors.Is(err, failure) {
		t.Fatal(err)
	}
	if err := stream.Finish(context.Background(), func(Event) error { return nil }); !errors.Is(err, failure) {
		t.Fatal("consumer error lost", err)
	}
	stream = newStreamFixture(t, OpenAIChat, Gemini)
	for _, raw := range streamFixtures[Gemini] {
		if err := stream.Push(context.Background(), Event{Data: []byte(raw)}, func(Event) error { return nil }); err != nil {
			t.Fatal(err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := stream.Finish(ctx, func(Event) error { t.Fatal("cancelled finish emitted output"); return nil }); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
func TestConvertedGeminiTailUsageRemainsAuthoritative(t *testing.T) {
	for _, destination := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(destination), func(t *testing.T) {
			stream := newStreamFixture(t, destination, Gemini)
			var events []Event
			emit := func(event Event) error { events = append(events, event); return nil }
			for _, raw := range streamFixtures[Gemini] {
				raw = strings.Replace(raw, `"responseId":"r",`, "", 1)
				raw = strings.Replace(raw, `"promptTokenCount":3`, `"cachedContentTokenCount":7,"promptTokenCount":3`, 1)
				if err := stream.Push(context.Background(), Event{Data: []byte(raw)}, emit); err != nil {
					t.Fatal(err)
				}
			}
			if err := stream.Push(context.Background(), Event{Data: []byte(`{"responseId":"tail-id","usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2,"totalTokenCount":5,"cachedContentTokenCount":0}}`)}, emit); err != nil {
				t.Fatal(err)
			}
			if err := stream.Finish(context.Background(), emit); err != nil {
				t.Fatal(err)
			}
			decoder, _ := NewDecoder(destination, Limits{})
			var usage *wire.Usage
			consume := func(chunk wire.StreamChunk) error {
				if chunk.Usage != nil {
					copy := wire.CloneUsage(*chunk.Usage)
					usage = &copy
				}
				return nil
			}
			for _, event := range events {
				if err := decoder.Push(context.Background(), event, consume); err != nil {
					t.Fatalf("invalid tail destination: %v %s", err, event.Data)
				}
			}
			if err := decoder.Finish(context.Background(), consume); err != nil {
				t.Fatal(err)
			}
			if usage == nil || usage.CachedInputTokens == nil || *usage.CachedInputTokens != 0 {
				t.Fatalf("tail zero disappeared: %#v events=%#v", usage, events)
			}
		})
	}
}
func TestCrossSignedStateFailsBeforePrivateBodyOrTerminalDelivery(t *testing.T) {
	for _, raw := range []string{`{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"private-thinking","signature":"private-signature"}}`, `{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"private-signature"}}`} {
		stream := newStreamFixture(t, OpenAIChat, Anthropic)
		if err := stream.Push(context.Background(), Event{Data: []byte(streamFixtures[Anthropic][0])}, func(Event) error { return nil }); err != nil {
			t.Fatal(err)
		}
		var delivered bool
		err := stream.Push(context.Background(), Event{Data: []byte(raw)}, func(Event) error { delivered = true; return nil })
		var loss *LossError
		if !errors.As(err, &loss) || delivered || strings.Contains(err.Error(), "private") {
			t.Fatalf("signed state was accepted/disclosed: %v %v", err, delivered)
		}
		if err := stream.Finish(context.Background(), func(Event) error { t.Fatal("loss emitted terminal"); return nil }); !errors.As(err, &loss) {
			t.Fatal("loss error was forgotten", err)
		}
	}
}
