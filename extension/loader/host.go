// INPUT: Explicitly granted AEP callbacks, the currently in-flight event handle and bounded native conversation tail/artifact reads
// OUTPUT: Bounded read-only state over the newest conversation window, scoped heuristic token counts and durable log/notification/metric facts
// POS: [L4 extension] Expiring native capability handles; never reads the whole session, exposes mutable AgentState or ambient memory/approval authority
package loader

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"

	"github.com/QuincySx/Alva/integration/protocol/aep"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
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
		view, err := p.stateMessages(ctx, handle)
		if err != nil {
			return nil, p.poison(&abi.SessionError{Operation: "AEP state messages", SessionID: p.config.NativeID, Err: err})
		}
		messages := view.messages
		if method == "host/state.count_tokens" {
			tokens, err := (agentabi.SimpleTokenEstimator{}).EstimateMessages(messages)
			if err != nil {
				return nil, p.poison(err)
			}
			return p.handleResult(handle, map[string]any{"tokens": tokens, "counted_messages": len(messages), "truncated": view.truncated, "omitted_at_least": view.omittedAtLeast, "processing": "Heuristic estimate over only counted_messages: the event's model request, or else the newest committed conversation window (at most 512 messages / 8 MiB). Older omitted history is not counted. Not a provider tokenizer or media billing formula. References were not opened."})
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
		return p.handleResult(handle, map[string]any{"messages": page, "offset": start, "next_offset": end, "has_more": end < len(messages), "truncated": view.truncated, "omitted_at_least": view.omittedAtLeast, "processing": "At most 16 messages per page from the event's model request, or else only the newest committed conversation window (at most 512 messages / 8 MiB). Offsets index that window oldest-first; truncated/omitted_at_least report older history that is not included. Large messages are references, not truncated facts; read relevant references explicitly. Original files are not loaded."})
	default:
		return nil, &aep.CallbackError{Code: aep.MethodNotFound}
	}
}

const (
	stateTailMessages = 512
	stateTailBytes    = 8 << 20
)

// stateMessages is the message view one state handle serves to AEP reads.
// truncated/omittedAtLeast describe older committed history left out; the
// exact older count is not computed (that would read the whole session).
type stateMessages struct {
	messages       []wire.Message
	truncated      bool
	omittedAtLeast int
}

func (p *Plugin) stateMessages(ctx context.Context, handle *stateHandle) (stateMessages, error) {
	if handle.messages != nil {
		return stateMessages{messages: handle.messages}, nil
	}
	handle.mu.Lock()
	defer handle.mu.Unlock()
	if handle.recent == nil {
		recent, err := p.recentMessages(ctx)
		if err != nil {
			return stateMessages{}, err
		}
		handle.recent = &recent
	}
	return *handle.recent, nil
}

// recentMessages restores only the newest committed conversation messages from
// one fixed-water BoundConversationTail snapshot: at most 512 messages and
// 8 MiB of restore cost. Bodies are read one by one rather than through
// ConversationTail.Read, which enforces a complete model-context tool protocol:
// an AEP event may legitimately observe an in-flight tool call (after_tool_call)
// or a window that begins inside an earlier tool group.
func (p *Plugin) recentMessages(ctx context.Context) (stateMessages, error) {
	repository := p.config.Repository
	tail, err := repository.BoundConversationTail(ctx, p.config.Owner, p.config.NativeID, stateTailMessages)
	if err != nil {
		return stateMessages{}, err
	}
	if tail.SessionID() != p.config.NativeID {
		return stateMessages{}, errors.New("AEP state tail belongs to a different native session")
	}
	metadata, err := tail.Metadata()
	if err != nil {
		return stateMessages{}, err
	}
	// Metadata is newest first; keep the newest messages that fit the budget.
	count, remaining := 0, int64(stateTailBytes)
	for count < len(metadata) && count < stateTailMessages && metadata[count].RestoreBytes <= remaining {
		remaining -= metadata[count].RestoreBytes
		count++
	}
	messages := make([]wire.Message, count)
	for i, meta := range metadata[:count] {
		_, data, err := repository.ReadArtifactFull(ctx, p.config.Owner, p.config.NativeID, meta.Reference.URI, stateTailBytes)
		if err != nil {
			return stateMessages{}, err
		}
		var record abi.SessionRecord
		if json.Unmarshal(data, &record) != nil {
			return stateMessages{}, errors.New("AEP state conversation record decoding failed")
		}
		if err := record.Validate(); err != nil {
			return stateMessages{}, err
		}
		if record.Kind != abi.RecordConversation || record.Message == nil || record.ID != meta.Record.RecordID || record.Sequence != meta.Record.Sequence || record.Message.Role != meta.Role {
			return stateMessages{}, errors.New("AEP state conversation metadata does not match committed body")
		}
		messages[count-1-i] = *record.Message
	}
	omitted := len(metadata) - count
	return stateMessages{messages: messages, truncated: omitted > 0, omittedAtLeast: omitted}, nil
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
