// INPUT: context, crypto/sha256, encoding/hex/json, errors, sort, kernel/abi, llm/wire
// OUTPUT: LoopDetectionMiddleware, NewLoopDetectionMiddleware
// POS: [L2 kernel runtime] Bounded repeated-call guardrail using actual AgentState extensions, never shared singleton session maps.
package core

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"sort"
)

type LoopDetectionMiddleware struct {
	warn, hard, window int
	OnWarning          func(abi.RuntimeContext, int)
}

func NewLoopDetectionMiddleware(warn, hard, window int) (*LoopDetectionMiddleware, error) {
	if warn < 1 || hard < warn || window < hard || window > 100000 {
		return nil, errors.New("loop detection requires 1 <= warn <= hard <= bounded window")
	}
	return &LoopDetectionMiddleware{warn: warn, hard: hard, window: window}, nil
}
func (*LoopDetectionMiddleware) Name() string { return "loop-detection" }

type loopHistory struct{ hashes []string }

func (*LoopDetectionMiddleware) OnRunStart(_ context.Context, runtime abi.RuntimeContext) error {
	abi.DeleteExtension[loopHistory](runtime.State.Extensions())
	return nil
}
func (*LoopDetectionMiddleware) OnRunEnd(_ context.Context, runtime abi.RuntimeContext, _ error) error {
	abi.DeleteExtension[loopHistory](runtime.State.Extensions())
	return nil
}
func (middleware *LoopDetectionMiddleware) AfterModel(_ context.Context, runtime abi.RuntimeContext, message *wire.Message) error {
	var calls []string
	for _, block := range message.Content {
		if block.ToolCall != nil {
			var canonical any
			if err := json.Unmarshal(block.ToolCall.Arguments, &canonical); err != nil {
				return err
			}
			encoded, _ := json.Marshal(canonical)
			calls = append(calls, block.ToolCall.Name+"\x00"+string(encoded))
		}
	}
	if len(calls) == 0 {
		return nil
	}
	sort.Strings(calls)
	encoded, _ := json.Marshal(calls)
	hash := sha256.Sum256(encoded)
	key := hex.EncodeToString(hash[:])
	history, _ := abi.GetExtension[loopHistory](runtime.State.Extensions())
	history.hashes = append(append([]string(nil), history.hashes...), key)
	if len(history.hashes) > middleware.window {
		history.hashes = history.hashes[len(history.hashes)-middleware.window:]
	}
	count := 0
	for _, candidate := range history.hashes {
		if candidate == key {
			count++
		}
	}
	abi.PutExtension(runtime.State.Extensions(), history)
	if count >= middleware.hard {
		var kept []wire.ContentBlock
		for _, block := range message.Content {
			if block.ToolCall == nil {
				kept = append(kept, block)
			}
		}
		message.Content = append(kept, wire.ContentBlock{Kind: wire.ContentText, Text: "[Loop detected: repeated identical tool calls were stripped. Respond with text or use a different approach.]"})
	} else if count >= middleware.warn && middleware.OnWarning != nil {
		middleware.OnWarning(runtime, count)
	}
	return nil
}
