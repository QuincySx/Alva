// INPUT: encoding/json, errors, strings and engine JSONL events
// OUTPUT: Stateful vendor frames, tool displays, inclusive cache-aware usage and journal references with separately verified resume provenance
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
	piAborted                                           bool
	// Pi RPC must redact a bounded completed aggregate before publishing any
	// derived chunks. Print keeps the shared mapper text contract.
	piWholeMessage bool
	tools          map[string]string
	displays       map[string]*engine.ToolDisplay
	texts          map[string]string
	messageNo      int
	usage          engine.Usage

	// referenceSession is an authorized retained identity, not a native start.
	// It qualifies early failure/cancellation journals before a CLI header arrives.
	referenceSession string
}

func newMapper(vendor, journal string) *mapper {
	return &mapper{vendor: vendor, journal: journal, tools: make(map[string]string), displays: make(map[string]*engine.ToolDisplay), texts: make(map[string]string)}
}
func (m *mapper) reference() *engine.Reference {
	session := m.session
	if session == "" {
		session = m.referenceSession
	}
	return &engine.Reference{SessionID: session, URI: m.journal}
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

// optionalNumber is a count the vendor may leave out: nil when the key is
// absent or not a count, so "not reported" stays distinct from zero.
func optionalNumber(value map[string]json.RawMessage, key string) *uint64 {
	raw, exists := value[key]
	if !exists {
		return nil
	}
	var result uint64
	if json.Unmarshal(raw, &result) != nil {
		return nil
	}
	return &result
}

// inclusiveUsage folds vendor counts whose input excludes cache reads and
// writes (Claude, Pi) into the engine's inclusive usage. An overflowing sum
// keeps the plain counts and reports the cache as unknown.
func inclusiveUsage(input, output uint64, read, write *uint64) engine.Usage {
	usage := engine.Usage{InputTokens: input, OutputTokens: output}
	total := input
	for _, count := range []*uint64{read, write} {
		if count != nil {
			if ^uint64(0)-total < *count {
				return usage
			}
			total += *count
		}
	}
	usage.InputTokens, usage.CachedInputTokens, usage.CacheWriteInputTokens = total, read, write
	return usage
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

// toolStart opens a call with the vendor's display of it (nil: other).
func (m *mapper) toolStart(id, name string, input json.RawMessage, display *engine.ToolDisplay) ([]engine.Event, error) {
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
	if display == nil {
		display = other()
	}
	m.displays[id] = display
	return []engine.Event{{Kind: engine.ToolStart, ID: id, Name: name, Input: input, Reference: m.reference(), Effect: engine.EffectUnknown, Display: engine.CloneToolDisplay(display)}}, nil
}

// toolEnd settles a call. Without a completed-item display it repeats the
// start display (result bodies are never copied into product history).
func (m *mapper) toolEnd(id, name string, isError bool, display *engine.ToolDisplay) ([]engine.Event, error) {
	var result []engine.Event
	if _, exists := m.tools[id]; !exists {
		start, err := m.toolStart(id, name, nil, nil)
		if err != nil {
			return nil, err
		}
		result = append(result, start...)
	}
	name = m.tools[id]
	if display == nil {
		display = settledDisplay(m.displays[id], isError)
	}
	delete(m.tools, id)
	delete(m.displays, id)
	return append(result, engine.Event{Kind: engine.ToolEnd, ID: id, Name: name, IsError: isError, Reference: m.reference(), Text: "Complete external tool I/O retained in private engine journal; raw body not copied into product history.", Display: display}), nil
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
		// Codex counts cached input inside input_tokens.
		usage := decode(frame["usage"])
		m.usage = engine.Usage{InputTokens: number(usage, "input_tokens"), OutputTokens: number(usage, "output_tokens"), CachedInputTokens: optionalNumber(usage, "cached_input_tokens")}
		if m.usage.Validate() != nil {
			m.usage.CachedInputTokens = nil
		}
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
				return m.toolEnd(id, name, failed, codexExecDisplay(itemKind, item))
			}
			if kind == "item.started" {
				return m.toolStart(id, name, item["arguments"], codexExecDisplay(itemKind, item))
			}
		}
	}
	return nil, nil
}
func (m *mapper) claude(kind string, frame map[string]json.RawMessage) ([]engine.Event, error) {
	// A print run owns exactly one native turn. Informational system frames may
	// follow its result, but execution output cannot reopen or replace that turn.
	if m.done {
		switch kind {
		case "assistant", "user", "stream_event", "result", "error":
			return nil, errors.New("Claude emitted execution output after its native result")
		case "system":
			if str(frame, "subtype") == "init" {
				return nil, errors.New("Claude restarted after its native result")
			}
		}
	}
	if id := str(frame, "session_id"); id != "" && m.session != "" && id != m.session {
		return nil, errors.New("Claude changed native session identity")
	}
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
				events, err := m.toolStart(str(block, "id"), str(block, "name"), block["input"], claudeDisplay(str(block, "name"), block["input"]))
				if err != nil {
					return nil, err
				}
				result = append(result, events...)
			case "tool_result":
				id := str(block, "tool_use_id")
				events, err := m.toolEnd(id, m.tools[id], boolean(block, "is_error"), nil)
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
		if subtype != "success" || boolean(frame, "is_error") {
			m.failure = "Claude returned a non-success native result"
		}
		// Claude's input_tokens leave out cache reads and cache creation.
		usage := decode(frame["usage"])
		m.usage = inclusiveUsage(number(usage, "input_tokens"), number(usage, "output_tokens"), optionalNumber(usage, "cache_read_input_tokens"), optionalNumber(usage, "cache_creation_input_tokens"))
		if m.session == "" && str(frame, "session_id") != "" {
			return m.start(str(frame, "session_id"))
		}
	case "error":
		m.failure = "Claude returned a fatal native error"
	}
	return nil, nil
}
func (m *mapper) pi(kind string, frame map[string]json.RawMessage) ([]engine.Event, error) {
	if m.done {
		switch kind {
		case "session", "agent_start", "agent_end", "agent_settled", "turn_start", "turn_end", "message_start", "message_update", "message_end", "tool_execution_start", "tool_execution_update", "tool_execution_end", "auto_retry_start", "auto_retry_end", "compaction_start", "compaction_end":
			return nil, errors.New("Pi emitted execution output after its settled result")
		}
	}
	switch kind {
	case "session":
		return m.start(str(frame, "id"))
	case "message_start":
		if str(decode(frame["message"]), "role") == "assistant" {
			delete(m.texts, m.current)
			m.current = ""
			_ = m.messageID()
		}
	case "message_update":
		event := decode(frame["assistantMessageEvent"])
		switch str(event, "type") {
		case "text_delta":
			id := m.messageID()
			delta := str(event, "delta")
			if len(delta) > (1<<20)-len(m.texts[id]) {
				return nil, errors.New("Pi active message exceeds 1 MiB")
			}
			return m.text(id, m.texts[id]+delta, false)
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
		id := m.messageID()
		defer func() { delete(m.texts, id); m.current = "" }()
		switch str(message, "stopReason") {
		case "error", "aborted", "length":
			m.piAborted = str(message, "stopReason") == "aborted"
			m.pending = str(message, "errorMessage")
			if m.pending == "" {
				m.pending = "Pi assistant ended with " + str(message, "stopReason")
			}
			return nil, nil
		}
		m.pending = ""
		m.piAborted = false
		var content []map[string]json.RawMessage
		_ = json.Unmarshal(message["content"], &content)
		var text strings.Builder
		for _, block := range content {
			if str(block, "type") == "text" {
				part := str(block, "text")
				if len(part) > (1<<20)-text.Len() {
					return nil, errors.New("Pi completed message exceeds 1 MiB")
				}
				text.WriteString(part)
			}
		}
		// Pi's input leaves out cacheRead and cacheWrite. pi-ai starts both at
		// 0 for every provider and only some providers fill them, so 0 and 0
		// cannot tell "no cache hit" from "not reported"; treat it as not
		// reported rather than show a 0% hit rate.
		usage := decode(message["usage"])
		read, write := optionalNumber(usage, "cacheRead"), optionalNumber(usage, "cacheWrite")
		if (read == nil || *read == 0) && (write == nil || *write == 0) {
			read, write = nil, nil
		}
		m.usage.Add(inclusiveUsage(number(usage, "input"), number(usage, "output"), read, write))
		if m.piWholeMessage {
			if text.Len() == 0 {
				return nil, nil
			}
			return []engine.Event{{Kind: engine.Message, ID: id, Text: text.String()}}, nil
		}
		return m.text(id, text.String(), true)
	case "tool_execution_start":
		return m.toolStart(str(frame, "toolCallId"), str(frame, "toolName"), frame["args"], piDisplay(str(frame, "toolName"), frame["args"]))
	case "tool_execution_end":
		return m.toolEnd(str(frame, "toolCallId"), str(frame, "toolName"), boolean(frame, "isError"), nil)
	case "auto_retry_start":
		return []engine.Event{{Kind: engine.Error, Recoverable: true, Text: "Pi is retrying; execution has not ended"}}, nil
	case "auto_retry_end":
		if !boolean(frame, "success") {
			m.failure = "Pi retry exhausted"
			m.pending = ""
		}
	case "agent_settled":
		// agent_end closes only a low-level run. Overflow recovery and
		// queued work can continue even when willRetry is false.
		m.done = true
		if boolean(frame, "aborted") {
			m.piAborted = true
			m.failure = "Pi reported an aborted native result"
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
