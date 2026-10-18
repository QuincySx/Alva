// INPUT: errors, math, time, unicode/utf8, llm/wire
// OUTPUT: AnalyticsEvent, AnalyticsSink, NoopAnalyticsSink and typed telemetry payloads
// POS: [L2.3 agent contracts] Opt-in product telemetry contract above kernel, never a durable conversation transport.
package abi

import (
	"errors"
	"math"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type AnalyticsKind string

const (
	AnalyticsSessionStart AnalyticsKind = "session_start"
	AnalyticsSessionEnd   AnalyticsKind = "session_end"
	AnalyticsToolStart    AnalyticsKind = "tool_call_start"
	AnalyticsToolEnd      AnalyticsKind = "tool_call_end"
	AnalyticsLLMCall      AnalyticsKind = "llm_call"
)

type AnalyticsSessionStarted struct {
	Workspace string `json:"workspace"`
}
type AnalyticsSessionEnded struct {
	DurationMS uint64 `json:"duration_ms"`
}
type AnalyticsToolStarted struct {
	Tool   string `json:"tool"`
	CallID string `json:"call_id"`
}
type AnalyticsToolEnded struct {
	Tool      string `json:"tool"`
	CallID    string `json:"call_id"`
	LatencyMS uint64 `json:"latency_ms"`
	OK        bool   `json:"ok"`
	Error     string `json:"error,omitempty"`
}
type AnalyticsModelCall struct {
	Provider  string      `json:"provider"`
	Model     string      `json:"model"`
	Usage     *wire.Usage `json:"usage,omitempty"`
	CostUSD   *float64    `json:"cost_usd,omitempty"`
	LatencyMS uint64      `json:"latency_ms"`
}

// Exactly one typed payload corresponds to Type. Unknown cost/provider/model
// remain absent/empty; no guessed price, arguments or output bodies are added.
type AnalyticsEvent struct {
	Type         AnalyticsKind            `json:"type"`
	SessionID    string                   `json:"session_id"`
	RunID        string                   `json:"run_id,omitempty"`
	Time         time.Time                `json:"time"`
	SessionStart *AnalyticsSessionStarted `json:"session_start,omitempty"`
	SessionEnd   *AnalyticsSessionEnded   `json:"session_end,omitempty"`
	ToolStart    *AnalyticsToolStarted    `json:"tool_start,omitempty"`
	ToolEnd      *AnalyticsToolEnded      `json:"tool_end,omitempty"`
	LLMCall      *AnalyticsModelCall      `json:"llm_call,omitempty"`
}

func (event AnalyticsEvent) Validate() error {
	if event.SessionID == "" || event.Time.IsZero() {
		return errors.New("analytics identity/time missing")
	}
	texts := []string{event.SessionID, event.RunID}
	n := 0
	if event.SessionStart != nil {
		n++
		texts = append(texts, event.SessionStart.Workspace)
	}
	if event.SessionEnd != nil {
		n++
	}
	if event.ToolStart != nil {
		n++
		if event.ToolStart.Tool == "" || event.ToolStart.CallID == "" {
			return errors.New("analytics tool identity missing")
		}
		texts = append(texts, event.ToolStart.Tool, event.ToolStart.CallID)
	}
	if event.ToolEnd != nil {
		n++
		if event.ToolEnd.Tool == "" || event.ToolEnd.CallID == "" {
			return errors.New("analytics tool identity missing")
		}
		texts = append(texts, event.ToolEnd.Tool, event.ToolEnd.CallID, event.ToolEnd.Error)
	}
	if event.LLMCall != nil {
		n++
		texts = append(texts, event.LLMCall.Provider, event.LLMCall.Model)
		if cost := event.LLMCall.CostUSD; cost != nil && (*cost < 0 || math.IsNaN(*cost) || math.IsInf(*cost, 0)) {
			return errors.New("invalid analytics cost")
		}
	}
	if n != 1 {
		return errors.New("analytics event requires exactly one payload")
	}
	valid := event.Type == AnalyticsSessionStart && event.SessionStart != nil || event.Type == AnalyticsSessionEnd && event.SessionEnd != nil || event.Type == AnalyticsToolStart && event.ToolStart != nil || event.Type == AnalyticsToolEnd && event.ToolEnd != nil || event.Type == AnalyticsLLMCall && event.LLMCall != nil
	if !valid {
		return errors.New("analytics kind/payload mismatch")
	}
	bytes := 0
	for _, text := range texts {
		bytes += len(text)
		if bytes > 4096 || !utf8.ValidString(text) {
			return errors.New("analytics metadata exceeds bounded UTF-8 limit")
		}
	}
	return nil
}

func CloneAnalyticsEvent(event AnalyticsEvent) AnalyticsEvent {
	if event.SessionStart != nil {
		value := *event.SessionStart
		event.SessionStart = &value
	}
	if event.SessionEnd != nil {
		value := *event.SessionEnd
		event.SessionEnd = &value
	}
	if event.ToolStart != nil {
		value := *event.ToolStart
		event.ToolStart = &value
	}
	if event.ToolEnd != nil {
		value := *event.ToolEnd
		event.ToolEnd = &value
	}
	if event.LLMCall != nil {
		value := *event.LLMCall
		if value.Usage != nil {
			usage := wire.CloneUsage(*value.Usage)
			value.Usage = &usage
		}
		if value.CostUSD != nil {
			cost := *value.CostUSD
			value.CostUSD = &cost
		}
		event.LLMCall = &value
	}
	return event
}

// Record MUST be synchronous and nonblocking, without caller-thread IO. Sinks
// absorb/report their own loss/errors: telemetry must never abort execution.
type AnalyticsSink interface{ Record(AnalyticsEvent) }
type NoopAnalyticsSink struct{}

func (NoopAnalyticsSink) Record(AnalyticsEvent) {}
