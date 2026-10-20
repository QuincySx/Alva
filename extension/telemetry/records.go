// INPUT: Prepared logical requests, actual model identity and optional cache counters.
// OUTPUT: Versioned fingerprint/cache records for the optional telemetry observer.
// POS: [L4 extension] Owns diagnostic encoding policy independently from execution facts.
package telemetry

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"hash"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func framedHash(h hash.Hash, value []byte) {
	var size [8]byte
	binary.BigEndian.PutUint64(size[:], uint64(len(value)))
	h.Write(size[:])
	h.Write(value)
}
func llmAuditRecord(kind string, value any) abi.SessionRecord {
	data, _ := json.Marshal(value)
	return abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.kernel.audit", Type: kind, SchemaVersion: 1, Payload: data}
}
func modelStartAudit(runID, requestID, stable string, model abi.LanguageModel, request abi.ModelRequest) (abi.SessionRecord, error) {
	tools := sha256.New()
	framedHash(tools, []byte("alva-tool-schema-fingerprint-v1"))
	for _, definition := range request.Tools {
		data, err := json.Marshal(definition)
		if err != nil {
			return abi.SessionRecord{}, errors.New("cannot fingerprint malformed prepared tool schema")
		}
		framedHash(tools, data)
	}
	schema := hex.EncodeToString(tools.Sum(nil))
	provider, modelID := "", ""
	known := false
	if identity, ok := model.(abi.ModelIdentity); ok {
		provider, modelID = identity.ProviderID(), identity.ModelID()
		known = provider != "" && modelID != ""
	}
	base := sha256.New()
	framedHash(base, []byte("alva-go-cache-base-fingerprint-v1"))
	framedHash(base, []byte(provider))
	framedHash(base, []byte(modelID))
	framedHash(base, []byte(stable))
	framedHash(base, []byte(schema))
	// The prepared request may also have dynamic middleware/system additions.
	// Their separate hash is exact for this logical request; they are not
	// silently folded into the configured stable-prefix fingerprint.
	system := sha256.New()
	systemCount := 0
	for _, m := range request.Messages {
		if m.Role == wire.RoleSystem {
			data, err := json.Marshal(m.Content)
			if err != nil {
				return abi.SessionRecord{}, errors.New("cannot fingerprint malformed prepared system content")
			}
			framedHash(system, data)
			systemCount++
		}
	}
	return llmAuditRecord("llm_call_start", struct {
		RunID          string `json:"run_id"`
		RequestID      string `json:"request_id"`
		IdentityKnown  bool   `json:"model_identity_known"`
		Version        int    `json:"cache_base_fingerprint_version"`
		Base           string `json:"cache_base_fingerprint"`
		Schema         string `json:"tool_schema_fingerprint"`
		PreparedSystem string `json:"prepared_system_fingerprint"`
		SystemCount    int    `json:"system_message_count"`
		Messages       int    `json:"message_count"`
		Tools          int    `json:"tools_count_sent"`
		Scope          string `json:"scope"`
	}{runID, requestID, known, 1, hex.EncodeToString(base.Sum(nil)), schema, hex.EncodeToString(system.Sum(nil)), systemCount, len(request.Messages), len(request.Tools), "logical request after BeforeModel; cache base is configured stable system + captured model identity + ordered effective schemas; dynamic system/history excluded; wrappers may retry or transform transport"}), nil
}
func cacheOutcome(usage *wire.Usage) string {
	if usage == nil {
		return "unreported"
	}
	read, write := usage.CachedInputTokens, usage.CacheWriteInputTokens
	if read != nil && *read > 0 && write != nil && *write > 0 {
		return "hit_and_write"
	}
	if read != nil && *read > 0 {
		return "hit"
	}
	if write != nil && *write > 0 {
		return "write"
	}
	if read != nil || write != nil {
		return "miss"
	}
	return "unreported"
}

func streamRecords(o kernel.StreamObservation) []abi.SessionRecord {
	var out []abi.SessionRecord
	if o.FirstOutput != nil {
		out = append(out, llmAuditRecord("llm_first_output", struct {
			RunID     string `json:"run_id"`
			RequestID string `json:"request_id"`
			Kind      string `json:"output_kind"`
			Time      int64  `json:"time_to_first_output_ms"`
		}{o.RunID, o.RequestID, o.FirstKind, *o.FirstOutput}))
	}
	out = append(out, llmAuditRecord("llm_stream_summary", o), llmAuditRecord("llm_call_end", struct {
		RunID     string      `json:"run_id"`
		RequestID string      `json:"request_id"`
		Outcome   string      `json:"outcome"`
		Cache     string      `json:"cache_outcome"`
		Usage     *wire.Usage `json:"usage,omitempty"`
	}{o.RunID, o.RequestID, o.Outcome, cacheOutcome(o.Usage), o.Usage}))
	return out
}
