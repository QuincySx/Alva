// INPUT: net/http/httptest, OpenAI-compatible chat and Anthropic Messages SSE wire formats
// OUTPUT: fakeModel, a scripted local model server that replays real protocol streams turn by turn and records every request
// POS: [scenario harness] The only replaced component of the real Hub assembly; it speaks the actual provider protocols.
package scenarios

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/provider"
)

// toolCall is one function call the fake model asks for.
type toolCall struct{ ID, Name, Args string }

// reply is what the fake model answers to one request.
type reply struct {
	// Status other than 0/200 answers with that HTTP error instead of a stream.
	Status int
	// Text is streamed as separate deltas, flushed one by one.
	Text []string
	// Calls are requested after the text; the finish reason becomes tool use.
	Calls []toolCall
	// Hold, when set, streams Text, signals Reached and then blocks. When Hold
	// closes the stream continues with After and finishes normally; when the
	// client goes away first the stream simply ends unfinished.
	Hold    chan struct{}
	Reached chan struct{}
	After   []string
	// Delay spaces the text deltas so streaming is observable.
	Delay        time.Duration
	Input, Usage int
}

// modelRequest is one recorded request.
type modelRequest struct {
	N      int
	Header http.Header
	Body   []byte
}

func (r modelRequest) has(text string) bool { return strings.Contains(string(r.Body), text) }

// toolResults returns each tool result the request carries back to the model,
// keyed by call ID, as the raw JSON of its content (plus an error marker).
func (r modelRequest) toolResults() map[string]string {
	out := map[string]string{}
	var body struct {
		Messages []struct {
			Role       string          `json:"role"`
			ToolCallID string          `json:"tool_call_id"`
			Content    json.RawMessage `json:"content"`
		} `json:"messages"`
	}
	if json.Unmarshal(r.Body, &body) != nil {
		return out
	}
	for _, m := range body.Messages {
		if m.Role == "tool" && m.ToolCallID != "" {
			out[m.ToolCallID] = string(m.Content)
			continue
		}
		var blocks []struct {
			Type      string          `json:"type"`
			ToolUseID string          `json:"tool_use_id"`
			Content   json.RawMessage `json:"content"`
			IsError   bool            `json:"is_error"`
		}
		if json.Unmarshal(m.Content, &blocks) != nil {
			continue
		}
		for _, b := range blocks {
			if b.Type == "tool_result" {
				text := string(b.Content)
				if b.IsError {
					text = "[is_error] " + text
				}
				out[b.ToolUseID] = text
			}
		}
	}
	return out
}

// last returns the role and raw content of the request's final message.
func (r modelRequest) last() (string, string) {
	var body struct {
		Messages []struct {
			Role    string          `json:"role"`
			Content json.RawMessage `json:"content"`
		} `json:"messages"`
	}
	if json.Unmarshal(r.Body, &body) != nil || len(body.Messages) == 0 {
		return "", ""
	}
	m := body.Messages[len(body.Messages)-1]
	return m.Role, string(m.Content)
}

// prompt returns the text of the final message (a user prompt keeps any
// handoff preamble in front of the words the user typed).
func (r modelRequest) prompt() string {
	_, content := r.last()
	var plain string
	if json.Unmarshal([]byte(content), &plain) == nil {
		return plain
	}
	var blocks []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	}
	_ = json.Unmarshal([]byte(content), &blocks)
	var out []string
	for _, b := range blocks {
		if b.Type == "text" {
			out = append(out, b.Text)
		}
	}
	return strings.Join(out, "\n")
}

// typed reports whether the final message ends with the given user words.
func (r modelRequest) typed(words string) bool {
	return strings.HasSuffix(strings.TrimSpace(r.prompt()), words)
}

// answersTool reports whether the request returns a tool result to the model.
func (r modelRequest) answersTool() bool {
	role, content := r.last()
	return role == "tool" || strings.Contains(content, `"tool_result"`)
}

// fakeModel answers requests in order from script; next, when set, decides
// dynamically instead (it sees every request, including the ones script
// already answered when script is empty).
type fakeModel struct {
	t        *testing.T
	kind     provider.Kind
	server   *httptest.Server
	mu       sync.Mutex
	script   []reply
	next     func(modelRequest) reply
	requests []modelRequest
	// catalog, when set, answers GET .../models in the protocol's list format.
	catalog []string
}

func newFakeModel(t *testing.T, kind provider.Kind, script ...reply) *fakeModel {
	t.Helper()
	m := &fakeModel{t: t, kind: kind, script: script}
	m.server = httptest.NewServer(http.HandlerFunc(m.serve))
	t.Cleanup(m.server.Close)
	return m
}

// endpoint is the complete request URL the protocol uses.
func (m *fakeModel) endpoint() string {
	if m.kind == provider.AnthropicKind {
		return m.server.URL + "/v1/messages"
	}
	return m.server.URL + "/v1/chat/completions"
}

func (m *fakeModel) calls() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.requests)
}

func (m *fakeModel) request(n int) modelRequest {
	m.mu.Lock()
	defer m.mu.Unlock()
	if n < 1 || n > len(m.requests) {
		m.t.Fatalf("model request %d not received (got %d)", n, len(m.requests))
	}
	return m.requests[n-1]
}

func (m *fakeModel) all() []modelRequest {
	m.mu.Lock()
	defer m.mu.Unlock()
	return append([]modelRequest(nil), m.requests...)
}

func (m *fakeModel) serve(w http.ResponseWriter, r *http.Request) {
	if r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/models") {
		m.mu.Lock()
		ids := append([]string(nil), m.catalog...)
		m.mu.Unlock()
		var data []map[string]string
		for _, id := range ids {
			data = append(data, map[string]string{"id": id, "display_name": "Model " + id})
		}
		w.Header().Set("Content-Type", "application/json")
		fmt.Fprint(w, jsonText(map[string]any{"data": data, "has_more": false}))
		return
	}
	data, _ := io.ReadAll(r.Body)
	m.mu.Lock()
	request := modelRequest{N: len(m.requests) + 1, Header: r.Header.Clone(), Body: data}
	m.requests = append(m.requests, request)
	var answer reply
	switch {
	case len(m.script) > 0:
		answer, m.script = m.script[0], m.script[1:]
	case m.next != nil:
		next := m.next
		m.mu.Unlock()
		answer = next(request)
		m.mu.Lock()
	default:
		m.mu.Unlock()
		m.t.Errorf("fake model received unscripted request %d", request.N)
		http.Error(w, "unscripted request", http.StatusInternalServerError)
		return
	}
	m.mu.Unlock()
	if answer.Status != 0 && answer.Status != http.StatusOK {
		http.Error(w, `{"error":{"message":"scripted failure"}}`, answer.Status)
		return
	}
	w.Header().Set("Content-Type", "text/event-stream")
	flusher, _ := w.(http.Flusher)
	send := func(event, payload string) {
		if m.kind == provider.AnthropicKind {
			fmt.Fprintf(w, "event: %s\ndata: %s\n\n", event, payload)
		} else {
			fmt.Fprintf(w, "data: %s\n\n", payload)
		}
		if flusher != nil {
			flusher.Flush()
		}
	}
	if m.kind == provider.AnthropicKind {
		m.anthropic(r, answer, send)
	} else {
		m.chat(r, answer, send)
	}
}

func jsonText(v any) string {
	data, _ := json.Marshal(v)
	return string(data)
}

// hold reports whether the stream must end unfinished because the client left.
func (m *fakeModel) hold(r *http.Request, answer reply) bool {
	if answer.Hold == nil {
		return false
	}
	if answer.Reached != nil {
		close(answer.Reached)
	}
	select {
	case <-answer.Hold:
		return false
	case <-r.Context().Done():
		return true
	}
}

func (m *fakeModel) chat(r *http.Request, answer reply, send func(string, string)) {
	for i, text := range answer.Text {
		delta := map[string]any{"content": text}
		if i == 0 {
			delta["role"] = "assistant"
		}
		send("", jsonText(map[string]any{"id": "fake", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": nil}}}))
		if answer.Delay > 0 {
			time.Sleep(answer.Delay)
		}
	}
	if m.hold(r, answer) {
		return
	}
	for _, text := range answer.After {
		send("", jsonText(map[string]any{"id": "fake", "choices": []any{map[string]any{"index": 0, "delta": map[string]any{"content": text}, "finish_reason": nil}}}))
	}
	finish := "stop"
	if len(answer.Calls) > 0 {
		finish = "tool_calls"
		var calls []any
		for i, c := range answer.Calls {
			calls = append(calls, map[string]any{"index": i, "id": c.ID, "type": "function", "function": map[string]string{"name": c.Name, "arguments": c.Args}})
		}
		send("", jsonText(map[string]any{"id": "fake", "choices": []any{map[string]any{"index": 0, "delta": map[string]any{"tool_calls": calls}, "finish_reason": nil}}}))
	}
	send("", jsonText(map[string]any{"id": "fake", "choices": []any{map[string]any{"index": 0, "delta": map[string]any{}, "finish_reason": finish}}}))
	if answer.Input != 0 || answer.Usage != 0 {
		send("", jsonText(map[string]any{"id": "fake", "choices": []any{}, "usage": map[string]int{"prompt_tokens": answer.Input, "completion_tokens": answer.Usage, "total_tokens": answer.Input + answer.Usage}}))
	}
	send("", "[DONE]")
}

func (m *fakeModel) anthropic(r *http.Request, answer reply, send func(string, string)) {
	send("message_start", jsonText(map[string]any{"type": "message_start", "message": map[string]any{"id": "fake", "role": "assistant", "usage": map[string]int{"input_tokens": answer.Input, "output_tokens": 0}}}))
	index := 0
	if len(answer.Text) > 0 {
		send("content_block_start", jsonText(map[string]any{"type": "content_block_start", "index": index, "content_block": map[string]any{"type": "text", "text": ""}}))
		for _, text := range answer.Text {
			send("content_block_delta", jsonText(map[string]any{"type": "content_block_delta", "index": index, "delta": map[string]any{"type": "text_delta", "text": text}}))
			if answer.Delay > 0 {
				time.Sleep(answer.Delay)
			}
		}
		if m.hold(r, answer) {
			return
		}
		for _, text := range answer.After {
			send("content_block_delta", jsonText(map[string]any{"type": "content_block_delta", "index": index, "delta": map[string]any{"type": "text_delta", "text": text}}))
		}
		send("content_block_stop", jsonText(map[string]any{"type": "content_block_stop", "index": index}))
		index++
	} else if m.hold(r, answer) {
		return
	}
	stop := "end_turn"
	for _, c := range answer.Calls {
		stop = "tool_use"
		send("content_block_start", jsonText(map[string]any{"type": "content_block_start", "index": index, "content_block": map[string]any{"type": "tool_use", "id": c.ID, "name": c.Name, "input": map[string]any{}}}))
		send("content_block_delta", jsonText(map[string]any{"type": "content_block_delta", "index": index, "delta": map[string]any{"type": "input_json_delta", "partial_json": c.Args}}))
		send("content_block_stop", jsonText(map[string]any{"type": "content_block_stop", "index": index}))
		index++
	}
	send("message_delta", jsonText(map[string]any{"type": "message_delta", "delta": map[string]any{"stop_reason": stop}, "usage": map[string]int{"output_tokens": answer.Usage}}))
	send("message_stop", `{"type":"message_stop"}`)
}

// text and call build the common replies.
func text(parts ...string) reply { return reply{Text: parts} }
func call(id, name, args string) reply {
	return reply{Calls: []toolCall{{ID: id, Name: name, Args: args}}}
}
