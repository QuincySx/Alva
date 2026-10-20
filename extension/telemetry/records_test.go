// INPUT: Pure prepared requests, model identity metadata and explicit optional provider cache counters
// OUTPUT: Stable versus dynamic fingerprint and cache reporting presence regressions
// POS: [L4 optional telemetry] Audit metadata never guesses provider caching or stores request bodies.
package telemetry

import (
	"context"
	"encoding/json"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"testing"
)

type auditModel struct{ provider, model string }

func (m auditModel) ProviderID() string { return m.provider }
func (m auditModel) ModelID() string    { return m.model }
func (auditModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}
func TestCacheAuditFingerprintScopeAndActualIdentity(t *testing.T) {
	request := abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleSystem, "stable + volatile one"), wire.TextMessage(wire.RoleUser, "private history")}, Tools: []wire.ToolDefinition{{Name: "tool", Description: "fixture schema", Parameters: json.RawMessage(`{"type":"object"}`)}}}
	read := func(stable string, m abi.LanguageModel, q abi.ModelRequest) map[string]any {
		var out map[string]any
		record, err := modelStartAudit("run", "request", stable, m, q)
		if err != nil {
			t.Fatal(err)
		}
		if err = json.Unmarshal(record.Payload, &out); err != nil {
			t.Fatal(err)
		}
		return out
	}
	base := read("stable", auditModel{"provider", "model"}, request)
	if base["model_identity_known"] != true {
		t.Fatal("actual complete identity not marked known")
	}
	for _, m := range []auditModel{{}, {provider: "provider"}, {model: "model"}} {
		if read("stable", m, request)["model_identity_known"] != false {
			t.Fatal("empty identity falsely reported as known")
		}
	}
	request.Messages[0] = wire.TextMessage(wire.RoleSystem, "stable + volatile two")
	request.Messages[1] = wire.TextMessage(wire.RoleUser, "different history")
	next := read("stable", auditModel{"provider", "model"}, request)
	if base["cache_base_fingerprint"] != next["cache_base_fingerprint"] || base["prepared_system_fingerprint"] == next["prepared_system_fingerprint"] {
		t.Fatal("dynamic system/history mixed into stable cache base")
	}
	for _, changed := range []map[string]any{read("changed stable", auditModel{"provider", "model"}, request), read("stable", auditModel{"other provider", "model"}, request), read("stable", auditModel{"provider", "other model"}, request), read("stable", auditModel{"provider", "model"}, abi.ModelRequest{Messages: request.Messages})} {
		if changed["cache_base_fingerprint"] == base["cache_base_fingerprint"] {
			t.Fatal("stable/model/effective tool change did not alter fingerprint")
		}
	}
}
func TestFingerprintRejectsMalformedPostMiddlewareData(t *testing.T) {
	for _, request := range []abi.ModelRequest{
		{Tools: []wire.ToolDefinition{{Name: "invalid", Parameters: json.RawMessage(`{"type":`)}}},
		{Messages: []wire.Message{{Role: wire.RoleSystem, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &wire.ToolCall{ID: "bad", Name: "invalid", Arguments: json.RawMessage(`{"private malformed data":`)}}}}}},
	} {
		if record, err := modelStartAudit("run", "request", "stable", auditModel{}, request); err == nil || len(record.Payload) != 0 {
			t.Fatal("malformed request fingerprinted as an empty/valid value", err)
		}
	}
}
func TestCacheAuditDistinguishesAbsentAndExplicitZero(t *testing.T) {
	zero, read, write := uint64(0), uint64(3), uint64(4)
	for _, tc := range []struct {
		usage *wire.Usage
		want  string
	}{{nil, "unreported"}, {&wire.Usage{}, "unreported"}, {&wire.Usage{CachedInputTokens: &zero}, "miss"}, {&wire.Usage{CacheWriteInputTokens: &zero}, "miss"}, {&wire.Usage{CachedInputTokens: &read}, "hit"}, {&wire.Usage{CacheWriteInputTokens: &write}, "write"}, {&wire.Usage{CachedInputTokens: &read, CacheWriteInputTokens: &write}, "hit_and_write"}} {
		if got := cacheOutcome(tc.usage); got != tc.want {
			t.Fatal(got, tc.want)
		}
	}
}
