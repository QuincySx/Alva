// INPUT: context, fmt, io, strings, testing, llm/wire.
// OUTPUT: Four actual raw protocol usage absent/null/explicit-zero/nonzero and cumulative cache-presence regressions.
// POS: [integration / model transport] Never infer reported cache usage from a normalized numeric zero.
package provider

import (
	"context"
	"fmt"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"io"
	"strings"
	"testing"
)

type parseFunc func(context.Context, io.Reader, int, func(wire.StreamChunk) error) error

func cacheFixture(kind Kind, fields string) string {
	switch kind {
	case OpenAIChatKind:
		return fmt.Sprintf("data: {\"id\":\"response\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1,\"prompt_tokens_details\":{%s}}}\n\ndata: [DONE]\n\n", fields)
	case OpenAIResponsesKind:
		return fmt.Sprintf("event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"response\",\"status\":\"completed\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"input_tokens_details\":{%s}}}}\n\n", fields)
	case AnthropicKind:
		return fmt.Sprintf("event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"response\",\"role\":\"assistant\",\"usage\":{\"input_tokens\":1,%s\"output_tokens\":0}}}\n\nevent: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\nevent: message_stop\ndata: {\"type\":\"message_stop\"}\n\n", func() string {
			if fields != "" {
				return fields + ","
			}
			return ""
		}())
	case GeminiKind:
		return fmt.Sprintf("data: {\"responseId\":\"response\",\"candidates\":[{\"index\":0,\"content\":{\"parts\":[{\"text\":\"ok\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":1,%s\"candidatesTokenCount\":1}}\n\n", func() string {
			if fields != "" {
				return fields + ","
			}
			return ""
		}())
	}
	return ""
}
func TestRawCacheUsagePresenceAcrossAllProtocols(t *testing.T) {
	parsers := map[Kind]parseFunc{OpenAIChatKind: parseChatFixture, OpenAIResponsesKind: parseResponsesFixture, AnthropicKind: parseAnthropicFixture, GeminiKind: parseGeminiFixture}
	keys := map[Kind]string{OpenAIChatKind: "cached_tokens", OpenAIResponsesKind: "cached_tokens", AnthropicKind: "cache_read_input_tokens", GeminiKind: "cachedContentTokenCount"}
	for kind, parser := range parsers {
		for _, value := range []string{"absent", "null", "0", "3"} {
			t.Run(string(kind)+"/"+value, func(t *testing.T) {
				fields := ""
				if value != "absent" {
					fields = fmt.Sprintf("%q:%s", keys[kind], value)
				}
				if kind == AnthropicKind && value != "absent" {
					fields += fmt.Sprintf(",\"cache_creation_input_tokens\":%s", value)
				}
				if kind == OpenAIResponsesKind && value != "absent" {
					fields += fmt.Sprintf(",\"cache_write_tokens\":%s", value)
				}
				acc := wire.NewStreamAccumulator(1 << 20)
				if err := parser(context.Background(), strings.NewReader(cacheFixture(kind, fields)), defaultEventLimit, acc.Push); err != nil {
					t.Fatal(err)
				}
				message, err := acc.Finish()
				if err != nil || message.Usage == nil {
					t.Fatal("usage/finish missing", err)
				}
				reported := value == "0" || value == "3"
				if (message.Usage.CachedInputTokens != nil) != reported {
					t.Fatalf("presence fabricated/lost: %#v", message.Usage)
				}
				if reported {
					want := uint64(0)
					if value == "3" {
						want = 3
					}
					if *message.Usage.CachedInputTokens != want {
						t.Fatal("cache count lost")
					}
				}
				writeSupported := kind == AnthropicKind || kind == OpenAIResponsesKind
				if (message.Usage.CacheWriteInputTokens != nil) != (writeSupported && reported) {
					t.Fatalf("cache write presence fabricated/lost: %#v", message.Usage)
				}
			})
		}
	}
}
func TestCumulativeExplicitZeroReplacesPriorReportedCache(t *testing.T) {
	for _, kind := range []Kind{AnthropicKind, GeminiKind} {
		t.Run(string(kind), func(t *testing.T) {
			var data string
			var parser parseFunc
			if kind == AnthropicKind {
				parser = parseAnthropicFixture
				data = cacheFixture(kind, `"cache_read_input_tokens":3,"cache_creation_input_tokens":5`)
				data = strings.Replace(data, `"usage":{"output_tokens":1}`, `"usage":{"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}`, 1)
			} else {
				parser = parseGeminiFixture
				data = cacheFixture(kind, `"cachedContentTokenCount":3`) + "data: {\"usageMetadata\":{\"cachedContentTokenCount\":0}}\n\n"
			}
			acc := wire.NewStreamAccumulator(1 << 20)
			if err := parser(context.Background(), strings.NewReader(data), defaultEventLimit, acc.Push); err != nil {
				t.Fatal(err)
			}
			message, err := acc.Finish()
			if err != nil {
				t.Fatal(err)
			}
			if message.Usage.CachedInputTokens == nil || *message.Usage.CachedInputTokens != 0 {
				t.Fatal("explicit cache0 retained old nonzero or became unknown")
			}
			if kind == AnthropicKind && (message.Usage.CacheWriteInputTokens == nil || *message.Usage.CacheWriteInputTokens != 0) {
				t.Fatal("explicit cachewrite0 lost")
			}
		})
	}
}
