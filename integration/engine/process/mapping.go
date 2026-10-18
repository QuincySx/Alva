// INPUT: encoding/json, errors, strings and engine JSONL events
// OUTPUT: Stateful vendor frame projection with native session/result provenance
// POS: [integration / native external-engine adapter] Claude/Codex/Pi dialect mapping without hidden reasoning or raw tool bodies in product events
package process

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/integration/engine"
)

type mapper struct {
	vendor, session, journal, failure, pending, current string
	done                                                bool
	tools                                               map[string]string
	texts                                               map[string]string
	messageNo                                           int
	usage                                               engine.Usage
}

func newMapper(vendor, journal string) *mapper {
	return &mapper{vendor: vendor, journal: journal, tools: make(map[string]string), texts: make(map[string]string)}
}
func (m *mapper) reference() *engine.Reference {
	return &engine.Reference{SessionID: m.session, URI: m.journal}
}
func decode(data []byte) map[string]json.RawMessage {
	var value map[string]json.RawMessage
	_ = json.Unmarshal(data, &value)
	return value
}
func str(value map[string]json.RawMessage, key string) string {
	var text string
	_ = json.Unmarshal(value[key], &text)
	return text
}
func boolean(value map[string]json.RawMessage, key string) bool {
	var result bool
	_ = json.Unmarshal(value[key], &result)
	return result
}
func number(value map[string]json.RawMessage, key string) uint64 {
	var result uint64
	_ = json.Unmarshal(value[key], &result)
	return result
}
func (m *mapper) start(id string) ([]engine.Event, error) {
	if !nativeID.MatchString(id) {
		return nil, errors.New("vendor returned invalid native session identity")
	}
	if m.session != "" {
		if m.session != id {
			return nil, errors.New("vendor changed native session identity")
		}
		return nil, nil
	}
	m.session = id
	return []engine.Event{{Kind: engine.SessionStarted, SessionID: id}}, nil
}
func (m *mapper) messageID() string {
	if m.current == "" {
		m.messageNo++
		m.current = fmt.Sprintf("assistant-%d", m.messageNo)
	}
	return m.current
}
func (m *mapper) text(id, text string, complete bool) ([]engine.Event, error) {
	if id == "" {
		id = m.messageID()
	}
	if len(id) > 256 || len(text) > 1<<20 {
		return nil, errors.New("vendor message exceeds identity/text budget")
	}
	if complete {
		if len(text) > 16<<10 {
			return m.text(id, text, false)
		}
		m.texts[id] = text
		return chunkText(engine.Message, id, text), nil
	}
	previous := m.texts[id]
	if !strings.HasPrefix(text, previous) {
		return nil, errors.New("vendor cumulative message rewrote committed text")
	}
	m.texts[id] = text
	return chunkText(engine.MessageDelta, id, text[len(previous):]), nil
}
func chunkText(kind engine.Kind, id, text string) []engine.Event {
	if kind == engine.Message && len(text) > 16<<10 {
		kind = engine.MessageDelta
	}
	var result []engine.Event
	for len(text) > 0 {
		end := len(text)
		if end > 16<<10 {
			end = 16 << 10
			for end > 0 && (text[end]&0xc0) == 0x80 {
				end--
			}
		}
		result = append(result, engine.Event{Kind: kind, ID: id, Text: text[:end]})
		text = text[end:]
		if kind == engine.Message {
			break
		}
	}
	return result
}
func (m *mapper) toolStart(id, name string, input json.RawMessage) ([]engine.Event, error) {
	if id == "" || len(id) > 256 || name == "" || len(name) > 256 {
		return nil, errors.New("vendor tool identity missing or oversized")
	}
	if _, exists := m.tools[id]; exists {
		return nil, nil
	}
	m.tools[id] = name
	if len(input) > 16<<10 || !json.Valid(input) {
		input = nil
	}
	return []engine.Event{{Kind: engine.ToolStart, ID: id, Name: name, Input: input, Reference: m.reference(), Effect: engine.EffectUnknown}}, nil
}
func (m *mapper) toolEnd(id, name string, isError bool) ([]engine.Event, error) {
	var result []engine.Event
	if _, exists := m.tools[id]; !exists {
		start, err := m.toolStart(id, name, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, start...)
	}
	name = m.tools[id]
	delete(m.tools, id)
	return append(result, engine.Event{Kind: engine.ToolEnd, ID: id, Name: name, IsError: isError, Reference: m.reference(), Text: "Complete external tool I/O retained in private engine journal; raw body not copied into product history."}), nil
}
func (m *mapper) mapLine(data []byte) ([]engine.Event, error) {
	if !json.Valid(data) {
		return nil, errors.New("external engine emitted malformed JSONL")
	}
	frame := decode(data)
	kind := str(frame, "type")
	if m.failure != "" {
		return nil, nil
	}
	switch m.vendor {
	case "codex":
		return m.codex(kind, frame)
	case "claude":
		return m.claude(kind, frame)
	case "pi":
		return m.pi(kind, frame)
	}
	return nil, nil
}
func (m *mapper) codex(kind string, frame map[string]json.RawMessage) ([]engine.Event, error) {
	switch kind {
	case "thread.started":
		return m.start(str(frame, "thread_id"))
	case "turn.completed":
		m.done = true
		usage := decode(frame["usage"])
		m.usage.InputTokens = number(usage, "input_tokens")
		m.usage.OutputTokens = number(usage, "output_tokens")
	case "turn.failed", "error":
		message := str(frame, "message")
		if message == "" {
			message = str(decode(frame["error"]), "message")
		}
		if message == "" {
			message = "External engine reported an error"
		}
		if kind == "error" && (strings.Contains(strings.ToLower(message), "retry") || strings.Contains(strings.ToLower(message), "reconnect")) {
			return []engine.Event{{Kind: engine.Error, Text: boundedText(message, 4096), Recoverable: true}}, nil
		}
		m.failure = boundedText(message, 4096)
	case "item.started", "item.updated", "item.completed":
		item := decode(frame["item"])
		id := str(item, "id")
		itemKind := str(item, "type")
		if itemKind == "agent_message" {
			text := str(item, "text")
			if kind == "item.completed" {
				return m.text(id, text, true)
			}
			return m.text(id, text, false)
		}
		if itemKind == "command_execution" || itemKind == "file_change" || itemKind == "mcp_tool_call" {
			name := itemKind
			if custom := str(item, "name"); custom != "" {
				name = custom
			}
			if kind == "item.completed" {
				failed := str(item, "status") == "failed"
				var code *int
				_ = json.Unmarshal(item["exit_code"], &code)
				if code != nil && *code != 0 {
					failed = true
				}
				return m.toolEnd(id, name, failed)
			}
			if kind == "item.started" {
				return m.toolStart(id, name, item["arguments"])
			}
		}
	}
	return nil, nil
}
func (m *mapper) claude(kind string, frame map[string]json.RawMessage) ([]engine.Event, error) {
	switch kind {
	case "system":
		if str(frame, "subtype") == "init" {
			return m.start(str(frame, "session_id"))
		}
	case "stream_event":
		event := decode(frame["event"])
		switch str(event, "type") {
		case "message_start":
			m.current = ""
			_ = m.messageID()
		case "content_block_delta":
			delta := decode(event["delta"])
			if str(delta, "type") == "text_delta" {
				id := m.messageID()
				text := m.texts[id] + str(delta, "text")
				return m.text(id, text, false)
			}
		}
	case "assistant", "user":
		message := decode(frame["message"])
		var content []map[string]json.RawMessage
		if err := json.Unmarshal(message["content"], &content); err != nil {
			return nil, errors.New("Claude message content is invalid")
		}
		var result []engine.Event
		var text strings.Builder
		for _, block := range content {
			switch str(block, "type") {
			case "text":
				if kind == "assistant" {
					text.WriteString(str(block, "text"))
				}
			case "tool_use":
				events, err := m.toolStart(str(block, "id"), str(block, "name"), block["input"])
				if err != nil {
					return nil, err
				}
				result = append(result, events...)
			case "tool_result":
				id := str(block, "tool_use_id")
				events, err := m.toolEnd(id, m.tools[id], boolean(block, "is_error"))
				if err != nil {
					return nil, err
				}
				result = append(result, events...)
			}
		}
		if text.Len() > 0 {
			events, err := m.text(m.messageID(), text.String(), true)
			if err != nil {
				return nil, err
			}
			result = append(result, events...)
		}
		return result, nil
	case "result":
		m.done = true
		subtype := str(frame, "subtype")
		if subtype != "success" {
			m.failure = "Claude returned a non-success native result"
		}
		usage := decode(frame["usage"])
		m.usage.InputTokens = number(usage, "input_tokens")
		m.usage.OutputTokens = number(usage, "output_tokens")
		if m.session == "" && str(frame, "session_id") != "" {
			return m.start(str(frame, "session_id"))
		}
	case "error":
		m.failure = "Claude returned a fatal native error"
	}
	return nil, nil
}
func (m *mapper) pi(kind string, frame map[string]json.RawMessage) ([]engine.Event, error) {
	switch kind {
	case "session":
		return m.start(str(frame, "id"))
	case "message_start":
		if str(decode(frame["message"]), "role") == "assistant" {
			m.current = ""
			_ = m.messageID()
		}
	case "message_update":
		event := decode(frame["assistantMessageEvent"])
		switch str(event, "type") {
		case "text_delta":
			id := m.messageID()
			return m.text(id, m.texts[id]+str(event, "delta"), false)
		case "error":
			m.pending = str(decode(event["error"]), "errorMessage")
			if m.pending == "" {
				m.pending = "Pi assistant stream failed"
			}
		}
	case "message_end":
		message := decode(frame["message"])
		if str(message, "role") != "assistant" {
			return nil, nil
		}
		if str(message, "stopReason") == "error" {
			m.pending = str(message, "errorMessage")
			if m.pending == "" {
				m.pending = "Pi assistant failed"
			}
			return nil, nil
		}
		m.pending = ""
		var content []map[string]json.RawMessage
		_ = json.Unmarshal(message["content"], &content)
		var text strings.Builder
		for _, block := range content {
			if str(block, "type") == "text" {
				text.WriteString(str(block, "text"))
			}
		}
		usage := decode(message["usage"])
		m.usage.InputTokens += number(usage, "input")
		m.usage.OutputTokens += number(usage, "output")
		return m.text(m.messageID(), text.String(), true)
	case "tool_execution_start":
		return m.toolStart(str(frame, "toolCallId"), str(frame, "toolName"), frame["args"])
	case "tool_execution_end":
		return m.toolEnd(str(frame, "toolCallId"), str(frame, "toolName"), boolean(frame, "isError"))
	case "auto_retry_start":
		return []engine.Event{{Kind: engine.Error, Recoverable: true, Text: "Pi is retrying; execution has not ended"}}, nil
	case "auto_retry_end":
		if !boolean(frame, "success") {
			m.failure = "Pi retry exhausted"
			m.pending = ""
		}
	case "agent_end":
		if !boolean(frame, "willRetry") {
			m.done = true
		}
	}
	return nil, nil
}
func boundedText(text string, n int) string {
	if len(text) <= n {
		return text
	}
	return "External error exceeded metadata budget; inspect engine journal"
}
