// INPUT: encoding/base64, encoding/json, fmt, regexp, strconv, strings, sync/atomic, time, unicode/utf8.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	base64 "encoding/base64"
	json "encoding/json"
	fmt "fmt"

	regexp "regexp"
	strconv "strconv"
	strings "strings"
	atomic "sync/atomic"
	time "time"
	utf8 "unicode/utf8"
)

type aBlock struct {
	Type string `json:"type"`
	// text
	Text string `json:"text,omitempty"`
	// image
	Source *struct {
		Type      string `json:"type"`
		MediaType string `json:"media_type,omitempty"`
		Data      string `json:"data,omitempty"`
		URL       string `json:"url,omitempty"`
	} `json:"source,omitempty"`
	// tool_use
	ID    string          `json:"id,omitempty"`
	Name  string          `json:"name,omitempty"`
	Input json.RawMessage `json:"input,omitempty"`
	// tool_result
	ToolUseID string          `json:"tool_use_id,omitempty"`
	Content   json.RawMessage `json:"content,omitempty"`
	IsError   bool            `json:"is_error,omitempty"`
	// thinking
	Thinking  string `json:"thinking,omitempty"`
	Signature string `json:"signature,omitempty"`
	// a prompt-cache breakpoint: the prompt up to here is cached
	CacheControl map[string]string `json:"cache_control,omitempty"`
	// sealed: a redacted_thinking block as Anthropic wrote it, which goes
	// back as it came (Part.Sealed, #1445)
	sealed json.RawMessage
}

// MarshalJSON writes a thinking block's text even when it is empty.
// Messages requires the field: a signed block with no text (Claude Code's
// thinking when its display is omitted) sent without it is refused,
// "messages.N.content.0.thinking.thinking: Field required" (#1447). Every
// other block keeps its omitempty fields.
func (b aBlock) MarshalJSON() ([]byte, error) {
	type plain aBlock
	if len(b.sealed) > 0 {
		return b.sealed, nil
	}
	if b.Type != "thinking" {
		return json.Marshal(plain(b))
	}
	return json.Marshal(struct {
		plain
		Thinking string `json:"thinking"`
	}{plain(b), b.Thinking})
}

// ephemeral marks a prompt-cache breakpoint.
var ephemeral = map[string]string{"type": "ephemeral"}

type aRequest struct {
	Model        string          `json:"model"`
	System       json.RawMessage `json:"system,omitempty"`
	CacheControl json.RawMessage `json:"cache_control,omitempty"`
	Messages     []struct {
		Role    string          `json:"role"`
		Content json.RawMessage `json:"content"`
	} `json:"messages"`
	Tools []struct {
		Type         string          `json:"type,omitempty"`
		Name         string          `json:"name"`
		Description  string          `json:"description,omitempty"`
		InputSchema  json.RawMessage `json:"input_schema,omitempty"`
		CacheControl json.RawMessage `json:"cache_control,omitempty"`
	} `json:"tools,omitempty"`
	ToolChoice *struct {
		Type                   string `json:"type"`
		Name                   string `json:"name,omitempty"`
		DisableParallelToolUse bool   `json:"disable_parallel_tool_use,omitempty"`
	} `json:"tool_choice,omitempty"`
	MaxTokens     int      `json:"max_tokens"`
	Temperature   *float64 `json:"temperature,omitempty"`
	TopP          *float64 `json:"top_p,omitempty"`
	StopSequences []string `json:"stop_sequences,omitempty"`
	Stream        bool     `json:"stream,omitempty"`
	Thinking      *struct {
		Type         string `json:"type"`
		BudgetTokens int    `json:"budget_tokens,omitempty"`
	} `json:"thinking,omitempty"`
	OutputConfig *struct {
		Effort string `json:"effort,omitempty"`
		Format *struct {
			Type   string          `json:"type"`
			Schema json.RawMessage `json:"schema"`
		} `json:"format,omitempty"`
	} `json:"output_config,omitempty"`
	Metadata   json.RawMessage `json:"metadata,omitempty"`
	Speed      string          `json:"speed,omitempty"` // "fast": Claude's fast mode
	Safeguards json.RawMessage `json:"safeguards,omitempty"`
}

func parseAnthropic(body []byte) (*Request, error) {
	return parseAnthropicRequest(body)
}

func parseAnthropicRequest(body []byte) (*Request, error) {
	var a aRequest
	if err := decodeJSON(body, &a); err != nil {
		return nil, fmt.Errorf("invalid request: %v", err)
	}
	r := &Request{Model: a.Model, System: stringOrText(a.System), MaxTokens: a.MaxTokens,
		Temp: a.Temperature, TopP: a.TopP, Stop: a.StopSequences, Stream: a.Stream, Fast: a.Speed == "fast"}

	r.Safeguards = a.Safeguards
	if string(r.Safeguards) == "null" {
		r.Safeguards = nil
	}
	if len(a.Metadata) > 0 && string(a.Metadata) != "null" {
		r.Metadata = a.Metadata
	}
	if oc := a.OutputConfig; oc != nil && oc.Format != nil && oc.Format.Type == "json_schema" && len(oc.Format.Schema) > 0 {
		r.Format = &Format{Type: "json_schema", Name: formatName, Schema: oc.Format.Schema}
	}
	for _, m := range a.Messages {
		msg := Message{Role: m.Role}
		var s string
		if decodeJSON(m.Content, &s) == nil {
			if s != "" {
				msg.Parts = append(msg.Parts, Part{Kind: Text, Text: s})
			}
		} else {
			var blocks []aBlock
			if err := decodeJSON(m.Content, &blocks); err != nil {
				return nil, fmt.Errorf("invalid message content: %v", err)
			}
			var raws []json.RawMessage // each block as the client sent it
			for i, b := range blocks {

				switch b.Type {
				case "text":
					msg.Parts = append(msg.Parts, Part{Kind: Text, Text: b.Text})
				case "image":
					if b.Source != nil {
						msg.Parts = append(msg.Parts, Part{Kind: Image, MediaType: b.Source.MediaType, Data: b.Source.Data, URL: b.Source.URL})
					}
				case "tool_use":
					msg.Parts = append(msg.Parts, Part{Kind: ToolCall, ID: anthropicInID(b.ID), Name: b.Name, Args: b.Input})
				case "tool_result":
					out, images := toolOutput(b.Content)
					msg.Parts = append(msg.Parts, Part{Kind: ToolResult, CallID: anthropicInID(b.ToolUseID), Text: out, Images: images, IsError: b.IsError})
				case "thinking":
					msg.Parts = append(msg.Parts, Part{Kind: Thinking, Text: b.Thinking, Signature: b.Signature})
				case "redacted_thinking":

					if raws == nil {
						_ = decodeJSON(m.Content, &raws)
					}
					if i < len(raws) {
						msg.Parts = append(msg.Parts, Part{Kind: Thinking, Sealed: raws[i], SealedBy: sealAnthropic})
					}
				}

			}
		}
		r.Messages = append(r.Messages, msg)
	}
	for _, t := range a.Tools {
		if t.Type != "" && t.Type != "custom" && len(t.InputSchema) == 0 {

			if strings.HasPrefix(t.Type, "web_search") {
				r.WebSearch = true
			}
			if strings.HasPrefix(t.Type, "advisor_") {

				for i := range r.Messages {
					for j, pt := range r.Messages[i].Parts {
						if pt.Kind == Text {
							r.Messages[i].Parts[j].Text, _ = advisorGoneIn(pt.Text)
						}
					}
				}
			}
			continue
		}
		if t.Name == "DeferredToolPlaceholder" {

			continue
		}
		r.Tools = append(r.Tools, Tool{Name: t.Name, Description: t.Description, Schema: t.InputSchema})
	}
	if tc := a.ToolChoice; tc != nil {
		switch tc.Type {
		case "auto", "none":
			r.ToolChoice = tc.Type
		case "any":
			r.ToolChoice = "required"
		case "tool":
			r.ToolChoice = "name:" + tc.Name
		}
		if tc.DisableParallelToolUse {
			f := false
			r.Parallel = &f
		}
	}

	if th := a.Thinking; th != nil && (th.Type == "disabled" || th.Type == "between_tools") {
		r.ThinkOff = true
	} else if th != nil && (th.Type == "enabled" || th.Type == "adaptive") {
		r.Thinking = true
		r.Effort = effortOfBudget(th.BudgetTokens)
		if oc := a.OutputConfig; oc != nil {
			if e := effortOf(oc.Effort); e != "" {
				r.Effort = e
			}
		}
	}
	return r, nil
}

// anthropicModel is one of Anthropic's own models, which think only when asked,
// so a request for one is left as the agent sent it.
var anthropicModel = regexp.MustCompile(`(?i)(?:^|[/.:-])claude-`)

// claudeVersion finds the family's version in a Claude model id however a
// relay spells it: claude-opus-4-6, claude-opus-5, anthropic.claude-sonnet-4.6-v1,
// a relay's opus-5.5, claude-fable-5-1, or the old order, claude-3-7-sonnet.
var claudeVersion = regexp.MustCompile(`(?:^|[^a-z0-9])(?:claude-)?(?:opus|sonnet|haiku|fable|mythos)-(\d{1,2})(?:[-.](\d{1,2}))?(?:[^0-9]|$)|claude-(\d+)(?:[-.](\d))?-(?:opus|sonnet|haiku)`)

// adaptiveOnly is a Claude model from 4.6 on, which thinks adaptively:
// claude-opus-5-5 refuses thinking.type=enabled with a budget ("requires
// adaptive thinking"), as do Fable and Mythos 5 and 5.1, whose adaptive
// thinking is always on, so how hard it thinks goes in output_config.effort.
func adaptiveOnly(model string) bool {
	major, minor, ok := claudeVersionOf(model)
	return ok && (major > 4 || major == 4 && minor >= 6)
}

// noXhigh is a Claude that thinks only adaptively but has no xhigh effort,
// Opus and Sonnet 4.6, so it is asked max in its place. Every later one
// takes xhigh (platform.claude.com/docs/en/build-with-claude/effort), and
// max there spends without limit.
func noXhigh(model string) bool {
	major, minor, ok := claudeVersionOf(model)
	return ok && major == 4 && minor == 6
}

// claudeVersionOf is a Claude model's version as claudeVersion reads it;
// ok is false for a model it doesn't read as a Claude.
func claudeVersionOf(model string) (major, minor int, ok bool) {
	m := claudeVersion.FindStringSubmatch(strings.ToLower(model))
	if m == nil {
		return 0, 0, false
	}
	v := m[1:3]
	if m[3] != "" {
		v = m[3:5]
	}
	major, _ = strconv.Atoi(v[0])
	minor, _ = strconv.Atoi(v[1])
	return major, minor, true
}

// effortInOutputConfig is a model that takes how hard it thinks from
// output_config.effort, beside thinking turned on, as ZCode asks it: Z.ai's
// GLM-5.2 and GLM-5.3 on their Anthropic endpoints. A budget alone leaves
// them at their own default.
var effortInOutputConfig = regexp.MustCompile(`(?i)glm-5\.[23](?:$|[-.:/\[])`)

// imageBlock is an image as an Anthropic block: inline, or by its URL.
func imageBlock(p Part) aBlock {
	b := aBlock{Type: "image"}
	b.Source = &struct {
		Type      string `json:"type"`
		MediaType string `json:"media_type,omitempty"`
		Data      string `json:"data,omitempty"`
		URL       string `json:"url,omitempty"`
	}{}
	if p.URL != "" && p.Data == "" {
		b.Source.Type, b.Source.URL = "url", p.URL
	} else {
		b.Source.Type, b.Source.MediaType, b.Source.Data = "base64", p.MediaType, p.Data
	}
	return b
}

// aMessage is one message of a request built for a Messages upstream.
type aMessage struct {
	Role    string   `json:"role"`
	Content []aBlock `json:"content"`
}

// interruptedTool is what an unanswered call gets, as pairToolMessages and
// pairToolItems make for the Chat and Responses paths.
const interruptedTool = "[The result of this tool call is unavailable: the turn was interrupted.]"

// buildAnthropic renders a request for an Anthropic-style upstream.
func buildAnthropic(r *Request, model string) []byte {
	return anthropicBody(r, model, false)
}

// anthropicBody is buildAnthropic; with replay, for an upstream that wants
// a turn's thinking back (anthropicReplaysThinking), thinking with no
// signature goes as its text instead of being left out.
func anthropicBody(r *Request, model string, replay bool) []byte {
	if r.Format != nil && r.Format.schema() == nil {

		r = r.inSystem()
	}
	var msgs []aMessage
	push := func(role string, blocks []aBlock) {
		if len(blocks) == 0 {
			return
		}
		if n := len(msgs); n > 0 && msgs[n-1].Role == role {
			msgs[n-1].Content = append(msgs[n-1].Content, blocks...)
			return
		}
		msgs = append(msgs, aMessage{Role: role, Content: blocks})
	}
	for _, m := range r.Messages {
		var results, rest []aBlock
		for _, p := range m.Parts {
			switch p.Kind {
			case Text:
				if strings.TrimSpace(p.Text) != "" {
					rest = append(rest, aBlock{Type: "text", Text: p.Text})
				}
			case File:
				rest = append(rest, aBlock{Type: "text", Text: attachmentText(p)})
			case Image:
				rest = append(rest, imageBlock(p))
			case ToolCall:

				rest = append(rest, aBlock{Type: "tool_use", ID: anthropicOutID(p.ID), Name: p.Name, Input: argsOf(p)})
			case ToolResult:
				c, _ := json.Marshal(p.Text)
				if len(p.Images) > 0 {
					// a tool_result holds images beside its text
					var blocks []aBlock
					if strings.TrimSpace(p.Text) != "" {
						blocks = append(blocks, aBlock{Type: "text", Text: p.Text})
					}
					for _, im := range p.Images {
						blocks = append(blocks, imageBlock(im))
					}
					c, _ = json.Marshal(blocks)
				}
				results = append(results, aBlock{Type: "tool_result", ToolUseID: anthropicOutID(p.CallID), Content: c, IsError: p.IsError})
			case Thinking:
				if p.sealedBy(sealAnthropic) {
					rest = append(rest, aBlock{Type: "redacted_thinking", sealed: p.Sealed})
				} else if p.Signature != "" {
					rest = append(rest, aBlock{Type: "thinking", Thinking: p.Text, Signature: p.Signature})
				} else if replay && p.Text != "" {
					rest = append(rest, aBlock{Type: "thinking", Thinking: p.Text})
				}
			}
		}
		role := m.Role
		if role != "assistant" {
			role = "user"
		}
		push(role, append(results, rest...))
	}

	msgs = pairToolBlocks(msgs)

	if n := len(msgs); n > 0 {
		c := msgs[n-1].Content
		for i := len(c) - 1; i >= 0; i-- {
			if c[i].Type != "thinking" && c[i].Type != "redacted_thinking" {
				c[i].CacheControl = ephemeral
				break
			}
		}
	}

	out := map[string]any{"model": model, "messages": msgs, "stream": r.Stream}
	if r.System != "" {

		out["system"] = []aBlock{{Type: "text", Text: r.System, CacheControl: ephemeral}}
	}
	maxTokens := r.MaxTokens
	if maxTokens <= 0 {
		maxTokens = defaultMaxTokens
	}

	think := (r.Thinking || r.Effort != "") && !r.ThinkOff
	if r.ThinkOff {
		out["thinking"] = map[string]any{"type": "disabled"}
	}
	if think && adaptiveOnly(model) {

		out["thinking"] = map[string]any{"type": "adaptive", "display": "summarized"}
		if e := r.Effort; e != "" {
			if e == "xhigh" && noXhigh(model) {
				e = "max"
			}
			out["output_config"] = map[string]any{"effort": e}
		}
	} else if budget := budgetOf(r.Effort); think && (r.MaxTokens <= 0 || r.MaxTokens > 2048) {
		if r.MaxTokens > 0 {

			budget = min(budget, maxTokens-1024)
		} else if maxTokens < budget+4096 {
			maxTokens = budget + 4096
		}
		out["thinking"] = map[string]any{"type": "enabled", "budget_tokens": budget}
		if r.Effort != "" && effortInOutputConfig.MatchString(model) {
			out["output_config"] = map[string]any{"effort": r.Effort}
		}
	} else if r.Temp != nil {
		out["temperature"] = *r.Temp
	} else if r.TopP != nil {
		out["top_p"] = *r.TopP
	}
	if s := r.Format.schema(); len(s) > 0 {
		oc, _ := out["output_config"].(map[string]any)
		if oc == nil {
			oc = map[string]any{}
		}
		oc["format"] = map[string]any{"type": "json_schema", "schema": s}
		out["output_config"] = oc
	}
	out["max_tokens"] = maxTokens
	if len(r.Stop) > 0 {
		out["stop_sequences"] = r.Stop
	}
	if len(r.Metadata) > 0 {
		out["metadata"] = r.Metadata
	}
	if len(r.Safeguards) > 0 && anthropicModel.MatchString(model) {

		out["safeguards"] = r.Safeguards
	}
	if len(r.Tools) > 0 || r.WebSearch {
		var tools []map[string]any
		for _, t := range r.Tools {
			schema := t.Schema
			if len(schema) == 0 {
				schema = json.RawMessage(`{"type":"object","properties":{}}`)
			} else {
				schema = objectSchema(schema)
			}
			tools = append(tools, map[string]any{"name": t.Name, "description": t.Description, "input_schema": schema})
		}
		if r.System == "" && len(tools) > 0 {
			tools[len(tools)-1]["cache_control"] = ephemeral
		}
		if r.WebSearch {
			tools = append(tools, map[string]any{"type": "web_search_20250305", "name": "web_search", "max_uses": 5})
		}
		out["tools"] = tools
		var tc map[string]any
		switch {
		case r.ToolChoice == "auto" || r.ToolChoice == "":
			if r.Parallel != nil && !*r.Parallel {
				tc = map[string]any{"type": "auto"}
			}
		case r.ToolChoice == "required":
			tc = map[string]any{"type": "any"}
		case r.ToolChoice == "none":
			tc = map[string]any{"type": "none"}
		case strings.HasPrefix(r.ToolChoice, "name:"):
			tc = map[string]any{"type": "tool", "name": strings.TrimPrefix(r.ToolChoice, "name:")}
		}
		if tc != nil {
			if r.Parallel != nil && !*r.Parallel && tc["type"] != "none" {
				tc["disable_parallel_tool_use"] = true
			}
			out["tool_choice"] = tc
		}
	}
	b, _ := json.Marshal(out)
	return b
}

// pairToolBlocks mends the tool exchange of a request built for a Messages
// upstream, as pairToolMessages does for a Chat one (#554) and
// pairToolItems for a relayed Responses request (#1341): Anthropic refuses
// a tool_use the next message's tool_results don't answer ("`tool_use` ids
// were found without `tool_result` blocks immediately after") and a
// tool_result naming a call it never saw ("unexpected `tool_use_id` found
// in `tool_result` blocks") — an interrupted turn or a client that dropped
// its calls leaves both behind, and a relay serving Claude over Messages
// passes the refusal on. A call left unanswered gets a synthetic result in
// the user message that follows, or one of its own where none does; an
// answer to no call becomes a text block, keeping what it said. A
// well-formed exchange is built exactly as it was.
func pairToolBlocks(msgs []aMessage) []aMessage {
	out := make([]aMessage, 0, len(msgs))
	var pending []string // the calls of the assistant message in hand
	open := map[string]bool{}
	answered := map[string]bool{}

	synthetic := func(id string) aBlock {
		c, _ := json.Marshal(interruptedTool)
		return aBlock{Type: "tool_result", ToolUseID: id, Content: c, IsError: true}
	}

	closeExchange := func() {
		var blocks []aBlock
		for _, id := range pending {
			if !answered[id] {
				blocks = append(blocks, synthetic(id))
			}
		}
		if len(blocks) > 0 {
			out = append(out, aMessage{Role: "user", Content: blocks})
		}
		pending, open, answered = nil, map[string]bool{}, map[string]bool{}
	}
	for _, m := range msgs {
		if m.Role == "assistant" {
			closeExchange()
			for _, b := range m.Content {
				if b.Type == "tool_use" {
					pending = append(pending, b.ID)
					open[b.ID] = true
				}
			}
			out = append(out, m)
			continue
		}
		var kept, rest []aBlock
		for _, b := range m.Content {
			if b.Type != "tool_result" {
				rest = append(rest, b)
				continue
			}
			if !open[b.ToolUseID] {

				rest = append(rest, resultSaid(b)...)
				continue
			}
			if answered[b.ToolUseID] {
				continue
			}
			answered[b.ToolUseID] = true
			kept = append(kept, b)
		}
		for _, id := range pending {
			if !answered[id] {
				kept = append(kept, synthetic(id))
			}
		}
		out = append(out, aMessage{Role: "user", Content: append(kept, rest...)})
		pending, open, answered = nil, map[string]bool{}, map[string]bool{}
	}
	closeExchange()
	return out
}

// resultSaid is what a tool_result that answers no call keeps: its text as
// a text block, its images beside it as they came.
func resultSaid(b aBlock) []aBlock {
	var s string
	if len(b.Content) > 0 && json.Unmarshal(b.Content, &s) == nil {
		if strings.TrimSpace(s) == "" {
			s = "Tool result received."
		}
		return []aBlock{{Type: "text", Text: s}}
	}
	var blocks []aBlock
	if len(b.Content) > 0 && json.Unmarshal(b.Content, &blocks) == nil && len(blocks) > 0 {
		return blocks
	}
	return []aBlock{{Type: "text", Text: "Tool result received."}}
}

type aUsage struct {
	InputTokens              int `json:"input_tokens"`
	OutputTokens             int `json:"output_tokens"`
	CacheReadInputTokens     int `json:"cache_read_input_tokens"`
	CacheCreationInputTokens int `json:"cache_creation_input_tokens"`
	// CacheCreation splits the cache writes by how long they are kept,
	// which Anthropic bills apart: 1.25× input for 5 minutes, 2× for an
	// hour
	CacheCreation *aCacheCreation `json:"cache_creation,omitempty"`
}

type aCacheCreation struct {
	Ephemeral5m int `json:"ephemeral_5m_input_tokens"`
	Ephemeral1h int `json:"ephemeral_1h_input_tokens"`
}

func (u aUsage) usage() Usage {
	out := Usage{Input: u.InputTokens, Output: u.OutputTokens, CacheRead: u.CacheReadInputTokens, CacheWrite: u.CacheCreationInputTokens}
	if c := u.CacheCreation; c != nil {
		out.cacheCreationReported = true
		out.cacheCreationHasCounts = c.Ephemeral5m+c.Ephemeral1h > 0
		out.CacheWrite = max(out.CacheWrite, c.Ephemeral5m+c.Ephemeral1h)
		out.CacheWrite1h = c.Ephemeral1h
	}
	return out
}

// anthropic is the usage as Anthropic says it, the cache writes split by
// how long they are kept when upstream reported TTLs or some were for an hour.
func (u Usage) anthropic() aUsage {
	out := aUsage{InputTokens: u.Input, OutputTokens: u.Output, CacheReadInputTokens: u.CacheRead, CacheCreationInputTokens: u.CacheWrite}
	if h := min(u.CacheWrite1h, u.CacheWrite); u.cacheCreationReported || h > 0 {
		out.CacheCreation = &aCacheCreation{Ephemeral5m: u.CacheWrite - h, Ephemeral1h: h}
	}
	return out
}

func stopToAnthropic(s string) string {
	switch s {
	case "length":
		return "max_tokens"
	case "tool":
		return "tool_use"
	case "filter":
		return "refusal"
	}
	return "end_turn"
}

// anthropicEncoder writes events as an Anthropic event stream.
type anthropicEncoder struct {
	id    string
	w     *sseWriter
	model string
	index int
	open  Kind // kind of the open content block, "" when none
	args  bool // the open tool block got arguments
	// fresh: a thinking block began upstream (KThinkStart), to be opened
	// apart from the one open with what it says first
	fresh   bool
	started bool
	col     collector
}

// anthropicID is a reply's id as Anthropic's API gives one, msg_…: an
// OpenAI-shaped upstream's chatcmpl-… (a plugin's provider, a relay) reads
// as a built-in's does.
func anthropicID(id string) string {
	if id == "" {
		return "msg_" + newID()
	}
	if strings.HasPrefix(id, "msg_") {
		return id
	}
	return "msg_" + strings.TrimPrefix(id, "chatcmpl-")
}

// Anthropic's Messages API, and Claude Code with it, takes a tool_use id
// only of [A-Za-z0-9_-]: Claude Code drops a call whose id has another
// character, and the turn fails "could not be parsed" (#1304). Other
// vendors' ids can have one (Devin's "Bash:0#a65b…", a plugin's or a
// relay's), so a call's id goes to an Anthropic client as anthropicOutID
// makes it, and an id that comes back in the client's request is turned
// back by anthropicInID, so the upstream that made the call gets its own
// id on the next turn. An id that is already
// safe goes both ways byte for byte, and Devin's "dv_…" ids, safe already,
// are not encoded twice.
const anthropicIDMark = "mp_"

func anthropicOutID(id string) string {
	if id == "" {
		return id
	}
	return safeCallID(anthropicIDMark, id)
}

func anthropicInID(id string) string { return rawCallID(anthropicIDMark, id) }

// safeCallID is id as one of [A-Za-z0-9_-] only: an id with another
// character, or one beginning with mark, is mark and its base64url, which
// rawCallID turns back; any other id is itself. It keeps no state, so an
// id comes back the same from any request, after any restart.
func safeCallID(mark, id string) string {
	if safeIDChars.MatchString(id) && !strings.HasPrefix(id, mark) {
		return id
	}
	return mark + base64.RawURLEncoding.EncodeToString([]byte(id))
}

// rawCallID is the id safeCallID was given for id, or id itself when
// safeCallID didn't make it.
func rawCallID(mark, id string) string {
	if !strings.HasPrefix(id, mark) || !safeIDChars.MatchString(id) {
		return id
	}
	raw, err := base64.RawURLEncoding.DecodeString(id[len(mark):])
	if err != nil || len(raw) == 0 || !utf8.Valid(raw) || safeCallID(mark, string(raw)) != id {
		return id
	}
	return string(raw)
}

var safeIDChars = regexp.MustCompile(`^[A-Za-z0-9_-]+$`)

func (e *anthropicEncoder) start(ev Event) {
	if e.started {
		return
	}
	e.started = true
	id := anthropicID(ev.MsgID)
	e.id = id
	model := ev.Model
	if model == "" {
		model = e.model
	}
	e.w.event("message_start", map[string]any{"type": "message_start", "message": map[string]any{
		"id": id, "type": "message", "role": "assistant", "model": model, "content": []any{},
		"stop_reason": nil, "stop_sequence": nil, "usage": ev.Usage.anthropic()}})
}

func (e *anthropicEncoder) close() {
	if e.open == "" {
		return
	}
	if e.open == ToolCall && !e.args {
		e.w.event("content_block_delta", map[string]any{"type": "content_block_delta", "index": e.index,
			"delta": map[string]any{"type": "input_json_delta", "partial_json": "{}"}})
	}
	e.w.event("content_block_stop", map[string]any{"type": "content_block_stop", "index": e.index})
	e.index++
	e.open = ""
}

func (e *anthropicEncoder) openBlock(k Kind, block map[string]any) {
	if e.open == k && k != ToolCall {
		return
	}
	e.close()
	e.args, e.fresh = false, false
	block["type"] = map[Kind]string{Text: "text", Thinking: "thinking", ToolCall: "tool_use"}[k]
	e.w.event("content_block_start", map[string]any{"type": "content_block_start", "index": e.index, "content_block": block})
	e.open = k
}

// thinking makes the open block a thinking block: the one open, unless a
// new one has begun since (KThinkStart).
func (e *anthropicEncoder) thinking() {
	if e.fresh {
		e.close()
		e.fresh = false
	}
	e.openBlock(Thinking, map[string]any{"thinking": ""})
}

func (e *anthropicEncoder) delta(d map[string]any) {
	e.w.event("content_block_delta", map[string]any{"type": "content_block_delta", "index": e.index, "delta": d})
}

func (e *anthropicEncoder) event(ev Event) {
	if ev.Kind != KStart && !e.started {
		e.start(Event{})
	}
	switch ev.Kind {
	case KStart:
		e.start(ev)
	case KText:
		if ev.Text == "" {
			return
		}
		e.openBlock(Text, map[string]any{"text": ""})
		e.delta(map[string]any{"type": "text_delta", "text": ev.Text})
	case KThinkStart:

		e.fresh = true
		e.col.add(ev)
		return
	case KThink:
		if ev.Text == "" {
			return
		}
		e.thinking()
		e.delta(map[string]any{"type": "thinking_delta", "thinking": ev.Text})
	case KSig:

		if e.open == Thinking || e.fresh {
			e.thinking()
			e.delta(map[string]any{"type": "signature_delta", "signature": ev.Text})
		}
	case KSealed:
		if ev.Name == sealAnthropic {

			e.close()
			e.fresh = false
			e.w.event("content_block_start", map[string]any{"type": "content_block_start", "index": e.index, "content_block": json.RawMessage(ev.Text)})
			e.w.event("content_block_stop", map[string]any{"type": "content_block_stop", "index": e.index})
			e.index++
		}
	case KToolStart:
		id := ev.ID
		if id == "" {
			id = "toolu_" + newID()
		}
		e.openBlock(ToolCall, map[string]any{"id": anthropicOutID(id), "name": ev.Name, "input": map[string]any{}})
	case KToolArgs:
		if e.open == ToolCall && ev.Text != "" {
			e.args = true
			e.delta(map[string]any{"type": "input_json_delta", "partial_json": ev.Text})
		}
	case KSearch:

		e.close()
		id := "srvtoolu_" + newID()
		query, _ := json.Marshal(map[string]string{"query": ev.Text})
		e.w.event("content_block_start", map[string]any{"type": "content_block_start", "index": e.index,
			"content_block": map[string]any{"type": "server_tool_use", "id": id, "name": "web_search", "input": map[string]any{}}})
		e.delta(map[string]any{"type": "input_json_delta", "partial_json": string(query)})
		e.w.event("content_block_stop", map[string]any{"type": "content_block_stop", "index": e.index})
		e.index++
		e.w.event("content_block_start", map[string]any{"type": "content_block_start", "index": e.index,
			"content_block": searchResultBlock(id, ev.Hits)})
		e.w.event("content_block_stop", map[string]any{"type": "content_block_stop", "index": e.index})
		e.index++
	case KError:
		e.close()
		typ := "api_error"
		if ev.Status == 413 && ev.Code != "context_length_exceeded" {

			typ = "request_too_large"
		}
		failed := map[string]any{"type": typ, "message": ev.Text}
		if ev.Code != "" {
			failed["code"] = ev.Code
		}
		saidError(failed, ev.Said)
		e.w.event("error", map[string]any{"type": "error", "error": failed})
	}
	e.col.add(ev)
}

// keepalive is Anthropic's own: a ping event, which its SDKs skip and
// Claude Code's stream watchdog counts.
func (e *anthropicEncoder) keepalive() {
	e.w.event("ping", map[string]any{"type": "ping"})
}

func (e *anthropicEncoder) finish() {
	if !e.started {
		e.start(Event{})
	}
	e.close()
	res := e.col.finish()
	delta := map[string]any{"stop_reason": stopToAnthropic(res.Stop), "stop_sequence": nil}
	if len(res.SafeguardResults) > 0 {
		delta["safeguard_results"] = res.SafeguardResults
	}
	if len(res.StopDetails) > 0 {
		delta["stop_details"] = res.StopDetails
	}
	e.w.event("message_delta", map[string]any{"type": "message_delta",
		"delta": delta,
		"usage": res.Usage.anthropic()})
	e.w.event("message_stop", map[string]any{"type": "message_stop"})
}

// renderAnthropic is the non-streaming reply.
func renderAnthropic(res Result, model string) []byte {
	content := []any{}
	for _, p := range res.Parts {
		switch p.Kind {
		case Text:
			if p.Text == "" && p.Signature != "" {
				continue
			}
			content = append(content, map[string]any{"type": "text", "text": p.Text})
		case Thinking:
			switch {
			case p.sealedBy(sealAnthropic):
				content = append(content, p.Sealed)
			case p.Text == "" && p.Signature == "":

			default:
				content = append(content, map[string]any{"type": "thinking", "thinking": p.Text, "signature": p.Signature})
			}
		case ToolCall:
			id := p.ID
			if id == "" {
				id = "toolu_" + newID()
			}
			content = append(content, map[string]any{"type": "tool_use", "id": anthropicOutID(id), "name": p.Name, "input": argsOf(p)})
		case Search:
			id := "srvtoolu_" + newID()
			content = append(content,
				map[string]any{"type": "server_tool_use", "id": id, "name": "web_search", "input": map[string]any{"query": p.Text}},
				searchResultBlock(id, p.Hits))
		}
	}
	id := anthropicID(res.ID)
	if res.Model != "" {
		model = res.Model
	}
	reply := map[string]any{"id": id, "type": "message", "role": "assistant", "model": model,
		"content": content, "stop_reason": stopToAnthropic(res.Stop), "stop_sequence": nil, "usage": res.Usage.anthropic()}
	if len(res.SafeguardResults) > 0 {
		reply["safeguard_results"] = res.SafeguardResults
	}
	if len(res.StopDetails) > 0 {
		reply["stop_details"] = res.StopDetails
	}
	b, _ := json.Marshal(reply)
	return b
}

// searchResultBlock is a web_search_tool_result block of the pages a
// search found.
func searchResultBlock(id string, hits []Hit) map[string]any {
	results := []map[string]any{}
	for _, h := range hits {
		// never the searcher's encrypted_content: it is sealed for the
		// vendor that searched, and the client would send it to another
		var age any
		if h.PageAge != "" {
			age = h.PageAge
		}
		results = append(results, map[string]any{"type": "web_search_result", "title": h.Title, "url": h.URL,
			"encrypted_content": "", "page_age": age})
	}
	return map[string]any{"type": "web_search_tool_result", "tool_use_id": id, "content": results}
}

// idSeq keeps ids made within one tick of the clock apart: Windows'
// clock moves in steps of up to 15.6ms and macOS's in microseconds, so
// the parallel calls of one Gemini chunk all got the same id.
var idSeq atomic.Uint32

// idClock is where newID reads the time; a test holds it still to stand in
// for a coarse clock.
var idClock = time.Now

func newID() string {
	return fmt.Sprintf("%x%08x", idClock().UnixNano(), idSeq.Add(1))
}
