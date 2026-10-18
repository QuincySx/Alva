// INPUT: encoding/json, testing and wire usage/message/stream ownership boundaries
// OUTPUT: Absent versus explicit-zero cache reporting and detached optional-counter regressions
// POS: [L0 foundation] Provider reporting presence survives pure values, JSON, accumulation and deep clones.
package wire

import (
	"encoding/json"
	"testing"
)

func TestUsageCachePresenceRoundTrip(t *testing.T) {
	for _, raw := range []string{`{"input_tokens":1,"output_tokens":2}`, `{"input_tokens":1,"output_tokens":2,"cached_input_tokens":0}`, `{"input_tokens":1,"output_tokens":2,"cache_write_input_tokens":0}`, `{"input_tokens":1,"output_tokens":2,"cached_input_tokens":0,"cache_write_input_tokens":0}`, `{"input_tokens":1,"output_tokens":2,"cached_input_tokens":3,"cache_write_input_tokens":4}`} {
		var usage Usage
		if err := json.Unmarshal([]byte(raw), &usage); err != nil {
			t.Fatal(err)
		}
		encoded, err := json.Marshal(usage)
		if err != nil || string(encoded) != raw {
			t.Fatal("cache reporting presence changed", string(encoded), err)
		}
	}
}

func TestUsageCacheClonesAndAccumulatorDoNotShareCounters(t *testing.T) {
	cached, written := uint64(0), uint64(2)
	usage := Usage{InputTokens: 3, CachedInputTokens: &cached, CacheWriteInputTokens: &written}
	cloned := CloneUsage(usage)
	message := CloneMessages([]Message{{Role: RoleAssistant, Usage: &usage}})[0]
	chunk := CloneStreamChunk(StreamChunk{Kind: ChunkUsage, Usage: &usage})
	accumulator := NewStreamAccumulator(1024)
	if err := accumulator.Push(StreamChunk{Kind: ChunkUsage, Usage: &usage}); err != nil {
		t.Fatal(err)
	}
	cached, written = 10, 20
	for _, detached := range []*Usage{&cloned, message.Usage, chunk.Usage, accumulator.Snapshot().Usage} {
		if detached == nil || detached.CachedInputTokens == nil || *detached.CachedInputTokens != 0 || detached.CacheWriteInputTokens == nil || *detached.CacheWriteInputTokens != 2 {
			t.Fatal("cache reporting counter was shared", detached)
		}
	}
	first := accumulator.Snapshot()
	*first.Usage.CachedInputTokens = 100
	*first.Usage.CacheWriteInputTokens = 200
	if *accumulator.Snapshot().Usage.CachedInputTokens != 0 || *accumulator.Snapshot().Usage.CacheWriteInputTokens != 2 {
		t.Fatal("detached snapshot mutated accumulator cache evidence")
	}
}
