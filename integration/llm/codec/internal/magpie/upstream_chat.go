// INPUT: cmp, encoding/json, fmt, maps, net/netip, path, slices, sort, strings, time.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	cmp "cmp"
	json "encoding/json"
	fmt "fmt"
	maps "maps"

	netip "net/netip"
	path "path"
	slices "slices"
	sort "sort"
	strings "strings"
	time "time"
)

type cToolCall struct {
	Index    *int   `json:"index,omitempty"`
	ID       string `json:"id,omitempty"`
	Type     string `json:"type,omitempty"`
	Function struct {
		Name      string `json:"name,omitempty"`
		Arguments string `json:"arguments,omitempty"`
	} `json:"function"`
	// Gemini's OpenAI-compatible API gives a call's thought signature
	// here, and wants it back (gemini_signature.go)
	ExtraContent json.RawMessage `json:"extra_content,omitempty"`
}

type cRequest struct {
	Model    string `json:"model"`
	Messages []struct {
		Role             string          `json:"role"`
		Content          json.RawMessage `json:"content"`
		ReasoningContent json.RawMessage `json:"reasoning_content,omitempty"`
		Reasoning        json.RawMessage `json:"reasoning,omitempty"`
		ReasoningDetails json.RawMessage `json:"reasoning_details,omitempty"`
		ToolCalls        []cToolCall     `json:"tool_calls,omitempty"`
		ToolCallID       string          `json:"tool_call_id,omitempty"`
	} `json:"messages"`
	Tools []struct {
		Type     string `json:"type"`
		Function struct {
			Name        string          `json:"name"`
			Description string          `json:"description,omitempty"`
			Parameters  json.RawMessage `json:"parameters,omitempty"`
			Strict      *bool           `json:"strict,omitempty"`
		} `json:"function"`
	} `json:"tools,omitempty"`
	ToolChoice          json.RawMessage `json:"tool_choice,omitempty"`
	WebSearchOptions    json.RawMessage `json:"web_search_options,omitempty"`
	MaxTokens           int             `json:"max_tokens,omitempty"`
	MaxCompletionTokens int             `json:"max_completion_tokens,omitempty"`
	Temperature         *float64        `json:"temperature,omitempty"`
	TopP                *float64        `json:"top_p,omitempty"`
	Stop                json.RawMessage `json:"stop,omitempty"`
	Stream              bool            `json:"stream,omitempty"`
	ReasoningEffort     string          `json:"reasoning_effort,omitempty"`
	ParallelToolCalls   *bool           `json:"parallel_tool_calls,omitempty"`
	ServiceTier         string          `json:"service_tier,omitempty"`
	PromptCacheKey      string          `json:"prompt_cache_key,omitempty"`
	ResponseFormat      json.RawMessage `json:"response_format,omitempty"`
	// not a Chat field: OpenCode merges a model's options into its body
	// whichever API it speaks, and a Codex model it asks on Chat is asked
	// on Responses (Request.AccessPrograms)
	AccessPrograms json.RawMessage `json:"access_programs,omitempty"`
}

func parseChat(body []byte) (*Request, error) {
	body = liftMessageTools(body)
	body = shapeChatTools(body)
	var c cRequest
	if err := decodeJSON(body, &c); err != nil {
		return nil, fmt.Errorf("invalid request: %v", err)
	}
	r := &Request{Model: c.Model, MaxTokens: c.MaxCompletionTokens, Temp: c.Temperature, TopP: c.TopP,
		Stream: c.Stream, Effort: effortOf(c.ReasoningEffort), Parallel: c.ParallelToolCalls, Fast: c.ServiceTier == "priority" || c.ServiceTier == "ultrafast", Ultrafast: c.ServiceTier == "ultrafast", Tier: c.ServiceTier,
		CacheKey: c.PromptCacheKey, Format: openAIFormat(c.ResponseFormat), AccessPrograms: c.AccessPrograms}
	if r.MaxTokens == 0 {
		r.MaxTokens = c.MaxTokens
	}
	if r.Effort != "" {
		r.Thinking = true
	}
	r.ThinkOff = strings.EqualFold(strings.TrimSpace(c.ReasoningEffort), "none")
	var stop string
	if decodeJSON(c.Stop, &stop) == nil && stop != "" {
		r.Stop = []string{stop}
	} else {
		decodeJSON(c.Stop, &r.Stop)
	}
	var sys []string
	for _, m := range c.Messages {
		switch m.Role {
		case "system", "developer":
			sys = append(sys, stringOrText(m.Content))
		case "user":
			r.Messages = append(r.Messages, Message{Role: "user", Parts: chatParts(m.Content)})
		case "assistant":
			msg := Message{Role: "assistant"}

			if think := reasoningText(map[string]json.RawMessage{
				"reasoning_content": m.ReasoningContent,
				"reasoning":         m.Reasoning,
				"reasoning_details": m.ReasoningDetails,
			}); think != "" {
				msg.Parts = append(msg.Parts, Part{Kind: Thinking, Text: think})
			}
			msg.Parts = append(msg.Parts, chatParts(m.Content)...)
			for _, tc := range m.ToolCalls {
				msg.Parts = append(msg.Parts, Part{Kind: ToolCall, ID: tc.ID, Name: tc.Function.Name, Args: parseArgs(tc.Function.Arguments),
					Signature: googleSignature(tc.ExtraContent)})
			}
			r.Messages = append(r.Messages, msg)
		case "tool":
			out, images := toolOutput(m.Content)
			r.Messages = append(r.Messages, Message{Role: "user", Parts: []Part{{Kind: ToolResult, CallID: m.ToolCallID, Text: out, Images: images}}})
		}
	}
	r.System = strings.Join(sys, "\n\n")
	r.WebSearch = len(c.WebSearchOptions) > 0 && string(c.WebSearchOptions) != "null"
	for _, t := range c.Tools {
		if strings.HasPrefix(t.Type, "web_search") {
			r.WebSearch = true
		}
		if t.Type != "" && t.Type != "function" {
			continue
		}
		r.Tools = append(r.Tools, Tool{Name: t.Function.Name, Description: t.Function.Description, Schema: t.Function.Parameters, Strict: t.Function.Strict != nil && *t.Function.Strict})
	}
	var tc string
	if decodeJSON(c.ToolChoice, &tc) == nil {
		r.ToolChoice = tc
	} else {
		var o struct {
			Type     string `json:"type"`
			Function struct {
				Name string `json:"name"`
			} `json:"function"`
		}
		if decodeJSON(c.ToolChoice, &o) == nil && o.Function.Name != "" {
			r.ToolChoice = "name:" + o.Function.Name
		}
	}
	return r, nil
}

// chatParts reads a message's content: a string, or an array of parts.
func chatParts(raw json.RawMessage) []Part {
	var s string
	if decodeJSON(raw, &s) == nil {
		if s == "" {
			return nil
		}
		return []Part{{Kind: Text, Text: s}}
	}
	var items []struct {
		Type     string `json:"type"`
		Text     string `json:"text"`
		ImageURL struct {
			URL string `json:"url"`
		} `json:"image_url"`
		File struct {
			FileData string `json:"file_data"`
			FileID   string `json:"file_id"`
			Filename string `json:"filename"`
		} `json:"file"`
	}
	decodeJSON(raw, &items)
	var out []Part
	for _, it := range items {
		switch it.Type {
		case "text":
			out = append(out, Part{Kind: Text, Text: it.Text})
		case "image_url":
			out = append(out, imagePart(it.ImageURL.URL))
		case "file":
			if p, ok := chatFile(it.File.FileData, it.File.FileID, it.File.Filename); ok {
				out = append(out, p)
			}
		}
	}
	return out
}

// chatFile is a Chat Completions file part (a PDF, say) as a file of the
// request's, which a Gemini upstream is given as inline data and the
// others are told of (attachmentText), rather than left out for the model
// to answer as if it had read it (#934). Its data is a data: URL, or the
// bare base64 some clients send; one named only by its OpenAI file id is
// kept as that id.
func chatFile(data, id, name string) (Part, bool) {
	mt, _, _ := strings.Cut(fileMediaType(strings.ToLower(path.Ext(name))), ";")
	switch {
	case strings.HasPrefix(data, "data:"):
		if p := imagePart(data); p.Data != "" {
			return Part{Kind: File, MediaType: cmp.Or(p.MediaType, mt, "application/octet-stream"), Data: p.Data}, true
		}
	case data != "":
		return Part{Kind: File, MediaType: cmp.Or(mt, "application/octet-stream"), Data: data}, true
	}
	if id != "" {
		return Part{Kind: File, MediaType: cmp.Or(mt, "application/octet-stream"), URL: id}, true
	}
	return Part{}, false
}

// imagePart reads a data: URL into an inline image, or keeps the URL.
func imagePart(u string) Part {
	if strings.HasPrefix(u, "data:") {
		if meta, data, ok := strings.Cut(strings.TrimPrefix(u, "data:"), ","); ok {
			mt := strings.TrimSuffix(meta, ";base64")
			return Part{Kind: Image, MediaType: mt, Data: data}
		}
	}
	return Part{Kind: Image, URL: u}
}

func dataURL(p Part) string {
	if p.URL != "" && p.Data == "" {
		return p.URL
	}
	return "data:" + p.MediaType + ";base64," + p.Data
}

// failedAsText are the plugins whose vendor has no failed mark on a tool
// result: the built-in said it in the text, "Error: " first (devin.go,
// qoder.go), and so the chat request to the plugin says it there too.
var failedAsText = map[string]bool{"devin": true, "qoder": true, "qoder-cn": true}

// chatReplaysReasoning is whether a Chat upstream wants a turn's reasoning
// back in reasoning_content: DeepSeek's models wherever they are served
// (#388), the model named as the provider names it (OpenCode Go's
// deepseek-v4.1-flash, whose host doesn't say so), and Command Code's
// plugin, which replays it for a Go key as the built-in did to
// /alpha/generate.
func chatReplaysReasoning(host, model string) bool {
	return strings.Contains(host, "deepseek") || strings.Contains(strings.ToLower(model), "deepseek") || host == "commandcode-plan"
}

func buildChat(r *Request, model, host string, rejectTemp bool) []byte {
	var msgs []map[string]any
	if r.System != "" {
		msgs = append(msgs, map[string]any{"role": "system", "content": r.System})
	}
	replay := chatReplaysReasoning(host, model)

	inlineThink := strings.HasSuffix(host, "minimax.io") || strings.HasSuffix(host, "minimaxi.com")

	gemini := geminiCompat(host, model)

	names := map[string]string{}
	var seen []map[string]any
	seeLater := func(p Part) {
		if len(p.Images) == 0 {
			return
		}
		of := "tool call " + p.CallID
		if name := names[p.CallID]; name != "" {
			of = name + " (" + of + ")"
		} else if p.Name != "" {
			of = p.Name + " (" + of + ")"
		}
		seen = append(seen, map[string]any{"type": "text", "text": "[From the result of " + of + ":]"})
		for _, im := range p.Images {
			seen = append(seen, map[string]any{"type": "image_url", "image_url": map[string]any{"url": dataURL(im)}})
		}
	}
	showSeen := func() {
		if len(seen) > 0 {
			msgs = append(msgs, map[string]any{"role": "user", "content": seen})
			seen = nil
		}
	}
	turns := joinSplitCalls(r.Messages)

	afterTool := false
	for i, m := range turns {
		if m.Role == "assistant" {
			showSeen()
			am := map[string]any{"role": "assistant"}
			var calls []map[string]any
			var think string
			signed := false
			for _, p := range m.Parts {
				if p.Kind == ToolCall && p.Signature != "" {
					signed = true
				}
			}
			for _, p := range m.Parts {
				switch p.Kind {
				case ToolCall:
					id := p.ID
					if id == "" {
						id = "call_" + newID()
					}
					names[id] = p.Name
					call := map[string]any{"id": id, "type": "function",
						"function": map[string]any{"name": p.Name, "arguments": argsString(p)}}
					if gemini {

						if sig := p.Signature; sig != "" {
							call["extra_content"] = googleExtra(sig)
						} else if !signed {
							call["extra_content"] = googleExtra(skipSignature)
						}
					}
					calls = append(calls, call)
				case Thinking:
					think += p.Text
				}
			}
			t := text(m.Parts)
			if think != "" && inlineThink && !(r.Resume && i == len(r.Messages)-1) {
				t = "<think>" + think + "</think>\n\n" + t
			}
			if t != "" || len(calls) == 0 {
				am["content"] = t
			}
			if len(calls) > 0 {
				am["tool_calls"] = calls
			}

			if r.Resume && i == len(turns)-1 {
				mode := chatPrefill(host, model)
				switch mode {
				case "prefix":
					am["prefix"] = true
				case "partial":
					am["partial"] = true
				}
				if think != "" && mode != "" {
					am["reasoning_content"] = think
				}
			} else if think != "" && replay {
				am["reasoning_content"] = think
			} else if replay && (len(calls) > 0 || afterTool) {

				am["reasoning_content"] = noReasoning
			}
			afterTool = false
			msgs = append(msgs, am)
			continue
		}
		var content []map[string]any
		plain := true
		flush := func() {
			if len(content) == 0 {
				return
			}
			if len(seen) > 0 {
				content, plain = append(seen, content...), false
				seen = nil
			}
			if plain {
				var b strings.Builder
				for _, c := range content {
					b.WriteString(c["text"].(string))
				}
				msgs = append(msgs, map[string]any{"role": "user", "content": b.String()})
			} else {
				msgs = append(msgs, map[string]any{"role": "user", "content": content})
			}
			content, plain = nil, true
		}
		// A turn's tool results all go ahead of its own text and images: a
		// tool message must immediately follow the assistant message whose
		// tool_calls it answers — a strict upstream (Kimi) refuses the
		// request otherwise, 400 "tool_call_id is not found".
		var tools []map[string]any
		for _, p := range m.Parts {
			switch p.Kind {
			case Text:
				content = append(content, map[string]any{"type": "text", "text": p.Text})
			case File:
				content = append(content, map[string]any{"type": "text", "text": attachmentText(p)})
			case Image:
				plain = false
				content = append(content, map[string]any{"type": "image_url", "image_url": map[string]any{"url": dataURL(p)}})
			case ToolResult:
				out := p.Text
				if n := len(p.Images); n > 0 {
					note := fmt.Sprintf("[The tool returned %d images; they follow in the next message.]", n)
					if n == 1 {
						note = "[The tool returned an image; it follows in the next message.]"
					}
					if strings.TrimSpace(out) != "" {
						out += "\n\n"
					}
					out += note
				}
				if p.IsError && failedAsText[host] {
					out = "Error: " + out
				}
				tool := map[string]any{"role": "tool", "tool_call_id": p.CallID, "content": out}
				if p.IsError && host == "cursor" {

					tool["is_error"] = true
				}
				tools = append(tools, tool)
				seeLater(p)
			}
		}
		msgs = append(msgs, tools...)
		afterTool = len(tools) > 0 && len(content) == 0 && len(seen) == 0
		flush()
	}
	showSeen()
	msgs, _ = withoutSilentAssistants(msgs)
	msgs = pairToolMessages(msgs)
	out := map[string]any{"model": model, "messages": msgs, "stream": r.Stream}
	if r.CacheKey != "" {
		out["prompt_cache_key"] = r.CacheKey
	}
	if r.Stream {
		out["stream_options"] = map[string]any{"include_usage": true}
	}

	if r.Fast && host == "cursor" {
		out["service_tier"] = "priority"
	}
	if r.OwnTier && r.Tier != "" {
		out["service_tier"] = r.Tier
	}
	if r.MaxTokens > 0 {
		if strings.HasSuffix(host, "openai.com") {
			out["max_completion_tokens"] = r.MaxTokens
		} else {
			out["max_tokens"] = r.MaxTokens
		}
	}
	if !rejectTemp {
		if r.Temp != nil {
			out["temperature"] = *r.Temp
		}
		if r.TopP != nil {
			out["top_p"] = *r.TopP
		}
	}
	if len(r.Stop) > 0 {
		out["stop"] = r.Stop
	}
	if r.GeminiCompat {

		if tc := aiStudioThinking(r, model); tc != nil {
			out["extra_body"] = map[string]any{"google": map[string]any{"thinking_config": tc}}
		} else if r.ThinkOff {

			out["reasoning_effort"] = cmp.Or(r.OffLevel, "minimal")
		} else if r.Effort != "" {
			out["reasoning_effort"] = r.Effort
		}
	} else if r.Effort != "" {
		out["reasoning_effort"] = r.Effort
	}
	if len(r.Tools) > 0 {
		var tools []map[string]any
		for _, t := range r.Tools {
			fn := map[string]any{"name": t.Name, "description": t.Description}
			if len(t.Schema) > 0 {
				fn["parameters"] = t.Schema
			}
			tools = append(tools, map[string]any{"type": "function", "function": fn})
		}
		out["tools"] = tools
		switch {
		case r.ToolChoice == "auto" || r.ToolChoice == "none" || r.ToolChoice == "required":
			out["tool_choice"] = r.ToolChoice
		case strings.HasPrefix(r.ToolChoice, "name:"):
			out["tool_choice"] = map[string]any{"type": "function", "function": map[string]any{"name": strings.TrimPrefix(r.ToolChoice, "name:")}}
		}
		if r.Parallel != nil {
			out["parallel_tool_calls"] = *r.Parallel
		}
	}
	if r.Format != nil {
		out["response_format"] = r.Format.chat()
	}
	if r.WebSearch && host == "openrouter.ai" {

		out["plugins"] = []map[string]any{{"id": "web"}}
	}
	b, _ := json.Marshal(out)
	return b
}

// pairToolMessages mends the tool exchange of a Chat request's messages
// for upstreams that validate it strictly — Kimi answers a mismatch with
// 400 "tool_call_id is not found" or "an assistant message with
// 'tool_calls' must be followed by tool messages…", and OpenAI-style
// upstreams refuse the same shapes. It runs on every request built for a
// Chat upstream (Zed's xAI path included); Chat→Chat traffic relays
// as-is and never reaches this path.
//
//   - the tool messages answering an assistant's tool_calls go in the
//     calls' order (an agent returns parallel results out of order);
//   - a second answer to the same call is dropped: the first answer
//     stands;
//   - a tool message answering no pending call — its call was answered
//     and flushed already, compacted away, or never there — becomes a
//     user message, so the result survives with no made-up call and no
//     id used twice, the way the Responses path's orphanedToolOutputs
//     turns an output without a call into a user message;
//   - a call left unanswered gets a synthetic error result, so the turn
//     can go on (an interrupted turn leaves its call pending); a turn's
//     calls split over consecutive assistant messages were joined into
//     one by joinSplitCalls before, so they are pending together (#1275);
//   - an assistant message with nothing in it — no text, no calls, no
//     reasoning — is dropped inside a pending exchange, where it would
//     sit between calls and their answers. One with neither text nor
//     calls was taken out before, wherever it stood
//     (withoutSilentAssistants, #2599).
func pairToolMessages(msgs []map[string]any) []map[string]any {
	out := make([]map[string]any, 0, len(msgs))
	var pending []string // calls of the last assistant message with tool_calls
	order := map[string]int{}
	answered := map[string]bool{}
	var tools []map[string]any // answers to pending, held for sorting

	flushTools := func() {
		if len(tools) == 0 {
			return
		}
		sort.SliceStable(tools, func(i, j int) bool {
			return order[toolMsgID(tools[i])] < order[toolMsgID(tools[j])]
		})
		out = append(out, tools...)
		tools = nil
	}
	flushPending := func() {
		flushTools()
		for _, id := range pending {
			if !answered[id] {
				out = append(out, map[string]any{"role": "tool", "tool_call_id": id,
					"content": "[The result of this tool call is unavailable: the turn was interrupted.]"})
			}
		}
		pending, order, answered = nil, map[string]int{}, map[string]bool{}
	}

	toolAsUser := func(m map[string]any) {
		text, _ := m["content"].(string)
		if strings.TrimSpace(text) == "" {
			text = "Tool result received."
		}
		if n := len(out); n > 0 && out[n-1]["role"] == "user" {
			if s, ok := out[n-1]["content"].(string); ok {
				out[n-1]["content"] = s + "\n\n" + text
				return
			}
		}
		out = append(out, map[string]any{"role": "user", "content": text})
	}

	for _, m := range msgs {
		switch m["role"] {
		case "assistant":
			calls, _ := m["tool_calls"].([]map[string]any)
			if len(calls) == 0 {
				if len(pending) > 0 && emptyAssistant(m) {

					continue
				}
				flushPending()
				out = append(out, m)
				continue
			}
			flushPending()
			out = append(out, m)
			for i, c := range calls {
				id, _ := c["id"].(string)
				pending = append(pending, id)
				order[id] = i
			}
		case "tool":
			id := toolMsgID(m)
			if _, ok := order[id]; !ok {

				flushPending()
				toolAsUser(m)
				continue
			}
			if answered[id] {
				continue
			}
			answered[id] = true
			tools = append(tools, m)
		default:
			flushPending()
			out = append(out, m)
		}
	}
	flushPending()
	return out
}

// toolMsgID is a tool message's tool_call_id.
func toolMsgID(m map[string]any) string {
	id, _ := m["tool_call_id"].(string)
	return id
}

// emptyAssistant reports whether an assistant message carries nothing:
// no text, no tool calls, no reasoning.
func emptyAssistant(m map[string]any) bool {
	if calls, ok := m["tool_calls"].([]map[string]any); ok && len(calls) > 0 {
		return false
	}
	if s, _ := m["reasoning_content"].(string); strings.TrimSpace(s) != "" {
		return false
	}
	s, _ := m["content"].(string)
	return strings.TrimSpace(s) == ""
}

// silentAssistant reports whether a Chat message is an assistant turn
// that says nothing: no content or only blank text, and no tool_calls or
// function_call. It is what an agent keeps of a reply the user cut off
// before it said anything — only its thinking, an empty text, nothing —
// and DeepSeek, Kimi and the like turn the whole request away over it,
// "Invalid assistant message: content or tool_calls must be set", on
// every later turn of the conversation (#2599). reasoning_content is not
// content to them. A prefill to go on from (prefix, partial), a refusal
// and an audio reply say something and stay.
func silentAssistant(m map[string]any) bool {
	if m["role"] != "assistant" {
		return false
	}
	if m["prefix"] == true || m["partial"] == true || m["function_call"] != nil || m["audio"] != nil {
		return false
	}
	if s, _ := m["refusal"].(string); s != "" {
		return false
	}
	switch c := m["tool_calls"].(type) {
	case []map[string]any:
		if len(c) > 0 {
			return false
		}
	case []any:
		if len(c) > 0 {
			return false
		}
	case nil:
	default:
		return false
	}
	switch c := m["content"].(type) {
	case nil:
		return true
	case string:
		return strings.TrimSpace(c) == ""
	case []any:
		for _, p := range c {
			pm, _ := p.(map[string]any)
			if t, _ := pm["text"].(string); pm == nil || pm["type"] != "text" || strings.TrimSpace(t) != "" {
				return false
			}
		}
		return true
	case []map[string]any:
		for _, pm := range c {
			if t, _ := pm["text"].(string); pm["type"] != "text" || strings.TrimSpace(t) != "" {
				return false
			}
		}
		return true
	}
	return false
}

// withoutSilentAssistants leaves out of a Chat request's messages every
// assistant turn that says nothing (silentAssistant), and joins the user
// messages it stood between into one, as toolAsUser joins: some models'
// chat templates turn away two user messages in a row. Nothing of the cut
// turn reaches the model, as nothing of it reached the user. Every
// strict-template rule stays kept: a tool message still follows the turn
// with its call, and a tool message followed by the user's is what an
// agent sends whenever the user writes after a tool result. It reports
// whether it changed anything; the messages are edited in place.
func withoutSilentAssistants(msgs []map[string]any) ([]map[string]any, bool) {
	if !slices.ContainsFunc(msgs, silentAssistant) {
		return msgs, false
	}
	out := make([]map[string]any, 0, len(msgs))
	dropped := false
	for _, m := range msgs {
		if silentAssistant(m) {
			dropped = true
			continue
		}
		if n := len(out); dropped && n > 0 && m["role"] == "user" && out[n-1]["role"] == "user" {
			if joined, ok := joinedUserContent(out[n-1]["content"], m["content"]); ok {
				prev := maps.Clone(out[n-1])
				prev["content"] = joined
				out[n-1] = prev
				continue
			}
		}
		dropped = false
		out = append(out, m)
	}
	return out, true
}

// joinedUserContent is two user messages' content as one: two texts with a
// blank line between them, else the parts of the first and then the
// second's, a text becoming a text part.
func joinedUserContent(a, b any) (any, bool) {
	as, aText := a.(string)
	bs, bText := b.(string)
	if aText && bText {
		return as + "\n\n" + bs, true
	}
	parts := func(c any) ([]any, bool) {
		switch c := c.(type) {
		case string:
			if strings.TrimSpace(c) == "" {
				return nil, true
			}
			return []any{map[string]any{"type": "text", "text": c}}, true
		case []any:
			return c, true
		case []map[string]any:
			out := make([]any, len(c))
			for i, p := range c {
				out[i] = p
			}
			return out, true
		}
		return nil, false
	}
	ap, aok := parts(a)
	bp, bok := parts(b)
	if !aok || !bok {
		return nil, false
	}
	return append(slices.Clip(ap), bp...), true
}

// aiStudioHost is Google AI Studio's Gemini API, whose OpenAI-compatible
// endpoint gives the model's thoughts only when asked for them.
const aiStudioHost = "generativelanguage.googleapis.com"

// geminiCompat reports whether a Chat upstream at host serving model is
// Gemini's OpenAI-compatible API: AI Studio's own, or for a Gemini model a
// proxy on this machine or the LAN, which is most often one in front of it
// (X @saoyan25's). A relay elsewhere is not assumed to pass thinking_config
// on, or to take it.
func geminiCompat(host, model string) bool {
	if host == aiStudioHost {
		return true
	}
	if !strings.Contains(strings.ToLower(model), "gemini") {
		return false
	}
	host = hostWithoutPort(host)
	host = strings.Trim(host, "[]")
	if host == "localhost" || strings.HasSuffix(host, ".local") {
		return true
	}
	ip, err := netip.ParseAddr(host)
	return err == nil && (ip.IsLoopback() || ip.IsPrivate())
}

// aiStudioThinking is the thinking_config asking Gemini for its thoughts at
// the effort the client asked, for a client that asked to see them: a level
// for Gemini 3 and later, a budget for 2.x (the steps Google maps
// reasoning_effort to).
func aiStudioThinking(r *Request, model string) map[string]any {
	if !r.Thinking || r.ThinkOff || r.Effort == "none" {
		return nil
	}
	tc := map[string]any{"include_thoughts": true}
	level := r.Effort
	switch level {
	case "xhigh", "max":
		level = "high"
	case "minimal", "low", "medium", "high":
	default:
		return tc
	}
	if strings.Contains(strings.ToLower(model), "gemini-2") {
		tc["thinking_budget"] = map[string]int{"minimal": 1024, "low": 1024, "medium": 8192, "high": 24576}[level]
	} else {
		tc["thinking_level"] = level
	}
	return tc
}

func (u Usage) chat() map[string]any {
	in := u.prompt()
	return map[string]any{"prompt_tokens": in, "completion_tokens": u.Output, "total_tokens": in + u.Output,
		"prompt_tokens_details":     map[string]any{"cached_tokens": u.CacheRead, "cache_write_tokens": u.CacheWrite},
		"completion_tokens_details": map[string]any{"reasoning_tokens": u.Reasoning}}
}

func stopToChat(s string) string {
	switch s {
	case "length":
		return "length"
	case "tool":
		return "tool_calls"
	case "filter":
		return "content_filter"
	}
	return "stop"
}

// chatEncoder writes events as a Chat Completions stream.
type chatEncoder struct {
	w       *sseWriter
	model   string
	id      string
	created int64
	started bool
	tool    int
	col     collector
}

func (e *chatEncoder) chunk(delta map[string]any, finish any, usage any) {
	choices := []map[string]any{}
	if delta != nil || finish != nil {
		if delta == nil {
			delta = map[string]any{}
		}
		choices = append(choices, map[string]any{"index": 0, "delta": delta, "finish_reason": finish})
	}
	e.w.event("", map[string]any{"id": e.id, "object": "chat.completion.chunk", "created": e.created,
		"model": e.model, "choices": choices, "usage": usage})
}

func (e *chatEncoder) start(ev Event) {
	if e.started {
		return
	}
	e.started, e.tool = true, -1
	e.id, e.created = ev.MsgID, time.Now().Unix()
	if e.id == "" {
		e.id = "chatcmpl-" + newID()
	}
	if !strings.HasPrefix(e.id, "chatcmpl-") {
		e.id = "chatcmpl-" + e.id
	}
	if ev.Model != "" {
		e.model = ev.Model
	}
	e.chunk(map[string]any{"role": "assistant", "content": ""}, nil, nil)
}

func (e *chatEncoder) event(ev Event) {
	if ev.Kind != KStart && !e.started {
		e.start(Event{})
	}
	switch ev.Kind {
	case KStart:
		e.start(ev)
	case KText:
		if ev.Text != "" {
			e.chunk(map[string]any{"content": ev.Text}, nil, nil)
		}
	case KThink:
		if ev.Text != "" {
			e.chunk(map[string]any{"reasoning_content": ev.Text}, nil, nil)
		}
	case KToolStart:
		e.tool++
		id := ev.ID
		if id == "" {
			id = "call_" + newID()
		}
		e.chunk(map[string]any{"tool_calls": []map[string]any{{"index": e.tool, "id": id, "type": "function",
			"function": map[string]any{"name": ev.Name, "arguments": ""}}}}, nil, nil)
	case KToolArgs:
		if e.tool >= 0 && ev.Text != "" {
			e.chunk(map[string]any{"tool_calls": []map[string]any{{"index": e.tool,
				"function": map[string]any{"arguments": ev.Text}}}}, nil, nil)
		}
	case KError:
		typ := "api_error"
		if ev.Status == 413 && ev.Code != "context_length_exceeded" {

			typ = "request_too_large"
		}
		failed := map[string]any{"message": ev.Text, "type": typ}
		if ev.Code != "" {
			failed["code"] = ev.Code
		}
		saidError(failed, ev.Said)
		e.w.event("", map[string]any{"error": failed})
	}
	e.col.add(ev)
}

// keepalive is an SSE comment, as OpenAI-compatible servers keep a Chat
// Completions stream alive; its readers skip one.
func (e *chatEncoder) keepalive() { e.w.comment("keepalive") }

func (e *chatEncoder) finish() {
	if !e.started {
		e.start(Event{})
	}
	res := e.col.finish()
	e.chunk(nil, stopToChat(res.Stop), nil)
	e.chunk(nil, nil, res.Usage.chat())
	e.w.event("", "[DONE]")
}

// renderChat is the non-streaming reply.
func renderChat(res Result, model string) []byte {
	msg := map[string]any{"role": "assistant", "content": nil}
	var calls []map[string]any
	var think string
	for _, p := range res.Parts {
		switch p.Kind {
		case Text:
			if p.Text == "" && p.Signature != "" {
				continue
			}
			if msg["content"] == nil {
				msg["content"] = p.Text
			} else {
				msg["content"] = msg["content"].(string) + p.Text
			}
		case Thinking:
			think += p.Text
		case ToolCall:
			id := p.ID
			if id == "" {
				id = "call_" + newID()
			}
			calls = append(calls, map[string]any{"id": id, "type": "function",
				"function": map[string]any{"name": p.Name, "arguments": argsString(p)}})
		}
	}
	if len(calls) > 0 {
		msg["tool_calls"] = calls
	}
	if think != "" {
		msg["reasoning_content"] = think
	}
	id := res.ID
	if id == "" {
		id = newID()
	}
	if !strings.HasPrefix(id, "chatcmpl-") {
		id = "chatcmpl-" + id
	}
	if res.Model != "" {
		model = res.Model
	}
	b, _ := json.Marshal(map[string]any{"id": id, "object": "chat.completion", "created": time.Now().Unix(), "model": model,
		"choices": []map[string]any{{"index": 0, "message": msg, "finish_reason": stopToChat(res.Stop)}},
		"usage":   res.Usage.chat()})
	return b
}
