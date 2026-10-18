// INPUT: Explicitly granted AEP callbacks and the currently in-flight event handle
// OUTPUT: Bounded read-only state, heuristic token counts and durable log/notification/metric facts
// POS: [L4 extension] Expiring native capability handles; never exposes mutable AgentState or ambient memory/approval authority
package loader

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"

	"github.com/QuincySx/Alva/go/integration/protocol/aep"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func decodeCallback(data []byte, out any) error {
	if len(data) == 0 || !wire.ValidJSON(data) || bytes.TrimSpace(data)[0] != '{' {
		return &aep.CallbackError{Code: aep.InvalidParams}
	}
	d := json.NewDecoder(bytes.NewReader(data))
	d.DisallowUnknownFields()
	if d.Decode(out) != nil || d.Decode(new(any)) != io.EOF {
		return &aep.CallbackError{Code: aep.InvalidParams}
	}
	return nil
}

func (p *Plugin) Handle(ctx context.Context, method string, params json.RawMessage) (json.RawMessage, error) {
	if err := p.healthy(); err != nil {
		return nil, err
	}
	switch method {
	case "host/log", "host/notify":
		var value struct {
			Level   string                     `json:"level"`
			Message string                     `json:"message"`
			Fields  map[string]json.RawMessage `json:"fields,omitempty"`
		}
		if err := decodeCallback(params, &value); err != nil {
			return nil, err
		}
		if len(value.Message) > 16<<10 || len(value.Fields) > 32 {
			return nil, &aep.CallbackError{Code: aep.InvalidParams}
		}
		switch value.Level {
		case "trace", "debug", "info", "warn", "error":
		default:
			return nil, &aep.CallbackError{Code: aep.InvalidParams}
		}
		p.mu.Lock()
		runID := p.runID
		p.mu.Unlock()
		if _, err := p.journal.record(ctx, runID, method, value); err != nil {
			return nil, p.poison(err)
		}
		return json.RawMessage(`{}`), nil
	case "host/emit_metric":
		var value struct {
			Name   string            `json:"name"`
			Value  float64           `json:"value"`
			Labels map[string]string `json:"labels,omitempty"`
		}
		if err := decodeCallback(params, &value); err != nil {
			return nil, err
		}
		if value.Name == "" || len(value.Name) > 128 || len(value.Labels) > 32 || math.IsNaN(value.Value) || math.IsInf(value.Value, 0) {
			return nil, &aep.CallbackError{Code: aep.InvalidParams}
		}
		p.mu.Lock()
		runID := p.runID
		p.mu.Unlock()
		if _, err := p.journal.record(ctx, runID, method, value); err != nil {
			return nil, p.poison(err)
		}
		return json.RawMessage(`{}`), nil
	case "host/state.read_reference", "host/state.search_reference":
		var value struct {
			Handle string `json:"handle"`
			URI    string `json:"uri"`
			Offset int64  `json:"offset,omitempty"`
			Limit  int    `json:"limit,omitempty"`
			Query  string `json:"query,omitempty"`
		}
		if err := decodeCallback(params, &value); err != nil {
			return nil, err
		}
		p.mu.Lock()
		handle := p.handle
		valid := handle != nil && handle.id == value.Handle && !p.closed
		p.mu.Unlock()
		if !valid {
			return nil, &aep.CallbackError{Code: aep.HandleExpired}
		}
		if len(value.URI) > 2048 || value.Offset < 0 {
			return nil, &aep.CallbackError{Code: aep.InvalidParams}
		}
		if method == "host/state.read_reference" {
			if value.Limit == 0 {
				value.Limit = 2048
			}
			if value.Limit < 1 || value.Limit > 4096 || value.Query != "" {
				return nil, &aep.CallbackError{Code: aep.InvalidParams}
			}
			page, err := p.refs.ReadText(ctx, value.URI, value.Offset, value.Limit)
			if err != nil {
				return nil, err
			}
			return p.handleResult(handle, page)
		}
		if value.Limit == 0 {
			value.Limit = 5
		}
		if value.Limit < 1 || value.Limit > 20 || value.Offset != 0 || value.Query == "" || len(value.Query) > 256 {
			return nil, &aep.CallbackError{Code: aep.InvalidParams}
		}
		matches, err := p.refs.SearchText(ctx, value.URI, value.Query, value.Limit)
		if err != nil {
			return nil, err
		}
		return p.handleResult(handle, matches)
	case "host/state.get_messages", "host/state.get_metadata", "host/state.count_tokens":
		var value struct {
			Handle string `json:"handle"`
			Limit  int    `json:"limit,omitempty"`
			Offset int    `json:"offset,omitempty"`
		}
		if err := decodeCallback(params, &value); err != nil {
			return nil, err
		}
		if value.Limit == 0 {
			value.Limit = 10
		}
		if value.Limit < 1 || value.Limit > 16 || value.Offset < 0 || value.Offset > 1<<20 {
			return nil, &aep.CallbackError{Code: aep.InvalidParams}
		}
		p.mu.Lock()
		handle := p.handle
		valid := handle != nil && handle.id == value.Handle && !p.closed
		p.mu.Unlock()
		if !valid {
			return nil, &aep.CallbackError{Code: aep.HandleExpired}
		}
		if method == "host/state.get_metadata" {
			return p.handleResult(handle, map[string]any{"metadata": map[string]any{"session_id": p.config.NativeID, "run_id": handle.runID, "workspace": p.config.Workspace, "source_notice": pluginSource}})
		}
		messages := handle.messages
		if messages == nil {
			var err error
			messages, err = p.log.Messages(ctx)
			if err != nil {
				return nil, p.poison(&abi.SessionError{Operation: "AEP state messages", SessionID: p.config.NativeID, Err: err})
			}
		}
		if method == "host/state.count_tokens" {
			tokens, err := (agentabi.SimpleTokenEstimator{}).EstimateMessages(messages)
			if err != nil {
				return nil, p.poison(err)
			}
			return p.handleResult(handle, map[string]any{"tokens": tokens, "processing": "Heuristic complete-content estimate, not a provider tokenizer or media billing formula. References were not opened."})
		}
		start := min(value.Offset, len(messages))
		end := min(start+value.Limit, len(messages))
		page := make([]aep.MessageDTO, 0, end-start)
		for _, message := range messages[start:end] {
			dto, err := aep.MessageToDTO(message)
			if err != nil {
				return nil, p.poison(err)
			}
			page = append(page, dto)
		}
		for i := range page {
			data, err := json.Marshal(page[i])
			if err != nil {
				return nil, p.poison(err)
			}
			if len(data) > 4096 {
				ref, err := p.refs.StoreText(ctx, string(data))
				if err != nil {
					return nil, p.poison(err)
				}
				page[i] = aep.MessageDTO{ID: page[i].ID, Role: page[i].Role, Content: []aep.ContentDTO{{Type: "reference", Reference: &ref}}}
			}
		}
		return p.handleResult(handle, map[string]any{"messages": page, "offset": start, "next_offset": end, "has_more": end < len(messages), "processing": "At most 16 event-scoped messages. Large messages are references, not truncated facts; read relevant references explicitly. Original files are not loaded."})
	default:
		return nil, &aep.CallbackError{Code: aep.MethodNotFound}
	}
}

func (p *Plugin) handleResult(handle *stateHandle, value any) (json.RawMessage, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	p.mu.Lock()
	valid := p.handle == handle && !p.closed
	p.mu.Unlock()
	if !valid {
		return nil, &aep.CallbackError{Code: aep.HandleExpired}
	}
	if len(data) > 80<<10 {
		return nil, errors.New("AEP state page exceeds output budget")
	}
	return data, nil
}
