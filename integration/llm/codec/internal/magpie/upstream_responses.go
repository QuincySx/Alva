// INPUT: bytes, encoding/json, fmt, github.com/tidwall/gjson, slices, strings, time.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	bytes "bytes"
	json "encoding/json"
	fmt "fmt"
	gjson "github.com/tidwall/gjson"
	slices "slices"
	strings "strings"
	time "time"
)

type rItem struct {
	Type    string          `json:"type,omitempty"`
	Role    string          `json:"role,omitempty"`
	Content json.RawMessage `json:"content,omitempty"`
	// function_call / function_call_output
	ID        string          `json:"id,omitempty"`
	CallID    string          `json:"call_id,omitempty"`
	Name      string          `json:"name,omitempty"`
	Namespace string          `json:"namespace,omitempty"`
	Arguments rArgs           `json:"arguments,omitempty"`
	Input     string          `json:"input,omitempty"` // custom_tool_call's
	Output    json.RawMessage `json:"output,omitempty"`
	Status    string          `json:"status,omitempty"`
	// tool_search_output: the tools Codex's search found, which the model
	// may call from then on
	Tools []rTool `json:"tools,omitempty"`
	// reasoning
	Summary          []rText `json:"summary,omitempty"`
	EncryptedContent string  `json:"encrypted_content,omitempty"`
	// web_search_call
	Action *struct {
		Query   string `json:"query"`
		Sources []struct {
			URL string `json:"url"`
		} `json:"sources"`
	} `json:"action,omitempty"`
}

// rArgs is a call's arguments: a JSON string on a function_call, an object
// on Codex's tool_search_call.
type rArgs string

func (a *rArgs) UnmarshalJSON(b []byte) error {
	var s string
	if decodeJSON(b, &s) == nil {
		*a = rArgs(s)
	} else if string(b) != "null" {
		*a = rArgs(b)
	}
	return nil
}

type rText struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

type rTool struct {
	Type        string          `json:"type"`
	Name        string          `json:"name"`
	Description string          `json:"description,omitempty"`
	Parameters  json.RawMessage `json:"parameters,omitempty"`
	Strict      *bool           `json:"strict,omitempty"`
	Tools       []rTool         `json:"tools,omitempty"` // a namespace's
	Execution   string          `json:"execution,omitempty"`
	Format      *rFormat        `json:"format,omitempty"` // a custom tool's
}

// rFormat is what a custom (freeform) tool's input is: "text", or a
// "grammar" in a syntax ("lark", "regex") with its definition.
type rFormat struct {
	Type       string `json:"type"`
	Syntax     string `json:"syntax,omitempty"`
	Definition string `json:"definition,omitempty"`
}

// customSchema is the arguments a custom tool is offered as taking.
var customSchema = json.RawMessage(`{"type":"object","properties":{"input":{"type":"string","description":"The tool's free-form input, the whole of it, exactly as it is to be run."}},"required":["input"],"additionalProperties":false}`)

// customDescription is a custom tool's description as a function's: its
// own, what its input is, and the grammar the input follows.
func customDescription(t rTool) string {
	d := strings.TrimSpace(t.Description)
	if d != "" {
		d += "\n\n"
	}
	d += "Put the tool's whole free-form input, as it is to be run, in the `input` string; it is passed on as written, so it is not itself JSON."
	if f := t.Format; f != nil && f.Type == "grammar" && strings.TrimSpace(f.Definition) != "" {
		syntax := f.Syntax
		if syntax == "" {
			syntax = "the"
		}
		d += "\n\nThe input follows this " + syntax + " grammar:\n" + strings.TrimSpace(f.Definition)
	}
	return d
}

// customInput is the free-form input of a call the model made to a custom
// tool offered as a function: its input argument, else what it gave.
func customInput(args string) string {
	var a struct {
		Input *string `json:"input"`
	}
	if decodeJSON([]byte(args), &a) == nil && a.Input != nil {
		return *a.Input
	}
	var str string
	if decodeJSON([]byte(args), &str) == nil {
		return str
	}
	return args
}

// toolSearch is Codex's tool search. With it Codex names its MCP tools (the
// ChatGPT apps' among them) and its sub-agent tools only in the search's
// description, and hands the model the ones it searched for in a
// tool_search_output, instead of every schema on every request (#258).
// Codex runs the search itself ("execution": "client"): a model magpie
// translates for is offered it as the function it is, and its call goes
// back to Codex as the tool_search_call Codex runs.
const toolSearch = "tool_search"

// searchFound is what a model reads of a tool search's result: the tools it
// may call now, by the names it is offered them under.
func searchFound(tools []rTool) string {
	var names []string
	for _, t := range tools {
		switch t.Type {
		case "function":
			names = append(names, t.Name)
		case "namespace":
			for _, nt := range t.Tools {
				if nt.Type == "function" {
					names = append(names, flatName(t.Name, nt.Name))
				}
			}
		}
	}
	if len(names) == 0 {
		return "No tools matched the search."
	}
	return "These tools are now available to call: " + strings.Join(names, ", ")
}

// flatName is the name a namespaced tool is offered to a model under,
// namespace__name, the same a Grok subscription is offered it under.
// unsealed is a tool's parameters without the "encrypted" marks Codex puts
// on some (spawn_agent's message): a translated request's calls go back to
// Codex as unsealed (callTo's encrypted_function_args), so an upstream that
// honours the mark must not seal them (#613).
// Alva preserves schema annotations and literal values verbatim. Unsupported
// encrypted-argument annotations are rejected by the outer admission gate.
func unsealed(schema json.RawMessage) json.RawMessage { return schema }

func flatName(namespace, name string) string {
	return FlatName(namespace, name)
}

type rRequest struct {
	Model             string          `json:"model"`
	Instructions      string          `json:"instructions,omitempty"`
	Input             json.RawMessage `json:"input"`
	Tools             []rTool         `json:"tools,omitempty"`
	ToolChoice        json.RawMessage `json:"tool_choice,omitempty"`
	MaxOutputTokens   int             `json:"max_output_tokens,omitempty"`
	Temperature       *float64        `json:"temperature,omitempty"`
	TopP              *float64        `json:"top_p,omitempty"`
	Stream            bool            `json:"stream,omitempty"`
	ParallelToolCalls *bool           `json:"parallel_tool_calls,omitempty"`
	ServiceTier       string          `json:"service_tier,omitempty"`
	PromptCacheKey    string          `json:"prompt_cache_key,omitempty"`
	Include           []string        `json:"include,omitempty"`
	ClientMetadata    json.RawMessage `json:"client_metadata,omitempty"`
	AccessPrograms    json.RawMessage `json:"access_programs,omitempty"`
	Text              json.RawMessage `json:"text,omitempty"`
	Reasoning         *struct {
		Effort  string `json:"effort,omitempty"`
		Summary string `json:"summary,omitempty"`
	} `json:"reasoning,omitempty"`
}

// textFormat splits a Responses client's text into the rest of it and
// the format it asks the answer in (text.format), which is asked for again
// in each upstream's own words. A format of plain text stays where it was.
func textFormat(text json.RawMessage) (json.RawMessage, *Format) {
	var fields map[string]json.RawMessage
	if decodeJSON(text, &fields) != nil {
		return text, nil
	}
	f := openAIFormat(fields["format"])
	if f == nil {
		return text, nil
	}
	delete(fields, "format")
	if len(fields) == 0 {
		return nil, f
	}
	rest, _ := json.Marshal(fields)
	return rest, f
}

// sentRaw is a raw field the client sent, unless it sent null.
func sentRaw(v json.RawMessage) json.RawMessage {
	if t := strings.TrimSpace(string(v)); t == "" || t == "null" {
		return nil
	}
	return v
}

func parseResponses(body []byte) (*Request, error) {

	body = liftAdditionalTools(body)
	var q rRequest
	if err := decodeJSON(body, &q); err != nil {
		return nil, fmt.Errorf("invalid request: %v", err)
	}
	r := &Request{Model: q.Model, System: q.Instructions, MaxTokens: q.MaxOutputTokens, Temp: q.Temperature,
		TopP: q.TopP, Stream: q.Stream, Parallel: q.ParallelToolCalls, Fast: q.ServiceTier == "priority" || q.ServiceTier == "ultrafast", Ultrafast: q.ServiceTier == "ultrafast", Tier: q.ServiceTier, CacheKey: q.PromptCacheKey, Include: q.Include,
		ClientMetadata: sentRaw(q.ClientMetadata), AccessPrograms: q.AccessPrograms}
	r.Text, r.Format = textFormat(sentRaw(q.Text))
	if q.Reasoning != nil {
		r.Effort = effortOf(q.Reasoning.Effort)
		r.Thinking = true
		r.ThinkOff = strings.EqualFold(strings.TrimSpace(q.Reasoning.Effort), "none")
	}
	var found []rTool // what Codex's tool searches found
	var s string
	if decodeJSON(q.Input, &s) == nil {
		r.Messages = append(r.Messages, Message{Role: "user", Parts: []Part{{Kind: Text, Text: s}}})
	} else {
		var items []rItem
		if err := decodeJSON(q.Input, &items); err != nil {
			return nil, fmt.Errorf("invalid input: %v", err)
		}

		replied := false
		var rawItems []json.RawMessage // decoded only for native standalone outputs and sealed reasoning
		for i, it := range items {
			if it.Role == "assistant" || strings.HasPrefix(it.Type, "function_call") || strings.HasPrefix(it.Type, "custom_tool_call") || strings.HasPrefix(it.Type, "tool_search") || it.Type == "reasoning" {
				replied = true
			}
			switch {
			case it.Type == "message" || (it.Type == "" && it.Role != ""):
				role := "user"
				if it.Role == "assistant" {
					role = "assistant"
				}
				parts := responsesParts(it.Content)
				if (it.Role == "system" || it.Role == "developer") && replied {

					if t := text(parts); t != "" {
						r.Messages = append(r.Messages, Message{Role: "user", Parts: []Part{{Kind: Text, Text: "<system-reminder>\n" + t + "\n</system-reminder>"}}})
					}
					continue
				}
				if it.Role == "system" || it.Role == "developer" {
					if t := text(parts); t != "" {
						if r.System != "" {
							r.System += "\n\n"
						}
						r.System += t
					}
					continue
				}
				r.Messages = append(r.Messages, Message{Role: role, Parts: parts})
			case it.Type == "agent_message":

				if parts := responsesParts(it.Content); len(parts) > 0 {
					r.Messages = append(r.Messages, Message{Role: "user", Parts: parts})
				}
			case it.Type == "function_call":
				name := it.Name
				if it.Namespace != "" {
					name = flatName(it.Namespace, it.Name)
				}
				r.Messages = append(r.Messages, Message{Role: "assistant", Parts: []Part{{Kind: ToolCall, ID: it.CallID, Name: name, Args: parseArgs(string(it.Arguments))}}})
			case it.Type == "custom_tool_call":
				name := it.Name
				if it.Namespace != "" {
					name = flatName(it.Namespace, it.Name)
				}
				args, _ := json.Marshal(map[string]string{"input": it.Input})
				r.Messages = append(r.Messages, Message{Role: "assistant", Parts: []Part{{Kind: ToolCall, ID: it.CallID, Name: name, Args: args}}})
			case it.Type == "tool_search_call":
				r.Messages = append(r.Messages, Message{Role: "assistant", Parts: []Part{{Kind: ToolCall, ID: it.CallID, Name: toolSearch, Args: parseArgs(string(it.Arguments))}}})
			case it.Type == "tool_search_output":
				found = append(found, it.Tools...)
				r.Messages = append(r.Messages, Message{Role: "user", Parts: []Part{{Kind: ToolResult, CallID: it.CallID, Text: searchFound(it.Tools)}}})
			case it.Type == "function_call_output" || it.Type == "custom_tool_call_output":
				out, images := toolOutput(it.Output)
				part := Part{Kind: ToolResult, CallID: it.CallID, Text: out, Images: images}
				if it.CallID == "" {

					if rawItems == nil {
						_ = decodeJSON(q.Input, &rawItems)
					}
					_ = decodeJSON(rawItems[i], &part.Standalone)
				}
				r.Messages = append(r.Messages, Message{Role: "user", Parts: []Part{part}})
			case it.Type == "reasoning":
				// the reasoning itself when the item carries it, else its
				// summary (all magpie gives a client of a translated reply)
				var b strings.Builder
				var content []rText
				_ = decodeJSON(it.Content, &content)
				for _, c := range content {
					if c.Type == "reasoning_text" {
						b.WriteString(c.Text)
					}
				}
				if b.Len() == 0 {
					for _, s := range it.Summary {
						b.WriteString(s.Text)
					}
				}
				part := Part{Kind: Thinking, Text: b.String()}
				if it.EncryptedContent != "" {

					if rawItems == nil {
						_ = decodeJSON(q.Input, &rawItems)
					}
					if i < len(rawItems) {
						part.Sealed, part.SealedBy = rawItems[i], sealResponses
					}
				}
				if b.Len() > 0 || part.Sealed != nil {
					r.Messages = append(r.Messages, Message{Role: "assistant", Parts: []Part{part}})
				}
			}
		}
	}
	r.Messages = mergeTurns(r.Messages)
	sent := len(q.Tools)
	offer := func(i int, t Tool, ns nsTool) {

		if i >= sent && slices.ContainsFunc(r.Tools, func(o Tool) bool { return o.Name == t.Name }) {
			return
		}
		if ns != (nsTool{}) {
			if r.Namespaced == nil {
				r.Namespaced = map[string]nsTool{}
			}
			r.Namespaced[t.Name] = ns
		}
		r.Tools = append(r.Tools, t)
	}

	for i, t := range append(q.Tools, found...) {
		if strings.HasPrefix(t.Type, "web_search") {
			r.WebSearch = true
		}
		switch t.Type {
		case "function":
			offer(i, Tool{Name: t.Name, Description: t.Description, Schema: unsealed(t.Parameters), Strict: t.Strict != nil && *t.Strict}, nsTool{})
		case "custom":
			offer(i, Tool{Name: t.Name, Description: customDescription(t), Schema: customSchema}, nsTool{Name: t.Name, Custom: true})
		case "namespace":

			for _, nt := range t.Tools {
				flat := flatName(t.Name, nt.Name)
				switch nt.Type {
				case "function":
					offer(i, Tool{Name: flat, Description: nt.Description, Schema: unsealed(nt.Parameters), Strict: nt.Strict != nil && *nt.Strict}, nsTool{Namespace: t.Name, Name: nt.Name})
				case "custom":
					offer(i, Tool{Name: flat, Description: customDescription(nt), Schema: customSchema}, nsTool{Namespace: t.Name, Name: nt.Name, Custom: true})
				}
			}
		case toolSearch:
			if t.Execution == "client" {
				offer(i, Tool{Name: toolSearch, Description: t.Description, Schema: t.Parameters}, nsTool{Search: true})
			}
		}
	}
	var tc string
	if decodeJSON(q.ToolChoice, &tc) == nil {
		r.ToolChoice = tc
	} else {
		var o struct {
			Type  string `json:"type"`
			Name  string `json:"name"`
			Mode  string `json:"mode"`
			Tools []struct {
				Type      string `json:"type"`
				Name      string `json:"name"`
				Namespace string `json:"namespace"`
			} `json:"tools"`
		}
		if decodeJSON(q.ToolChoice, &o) == nil {
			switch o.Type {
			case "allowed_tools":
				if o.Mode != "auto" && o.Mode != "required" {
					return nil, fmt.Errorf("invalid allowed_tools mode %q", o.Mode)
				}
				r.ToolChoice = o.Mode
				allowed := make(map[string]bool, len(o.Tools))
				webSearch := false
				for _, tool := range o.Tools {
					switch {
					case (tool.Type == "function" || tool.Type == "custom") && tool.Name != "":
						if tool.Namespace != "" {
							allowed[flatName(tool.Namespace, tool.Name)] = true
						} else {
							allowed[tool.Name] = true
						}
					case strings.HasPrefix(tool.Type, "web_search"):
						webSearch = true
					}
				}
				r.WebSearch = r.WebSearch && webSearch
				var tools []Tool
				for _, tool := range r.Tools {
					if allowed[tool.Name] {
						tools = append(tools, tool)
					}
				}
				r.Tools = tools
				for name := range r.Namespaced {
					if !allowed[name] {
						delete(r.Namespaced, name)
					}
				}
			case "function":
				if o.Name != "" {
					r.ToolChoice = "name:" + o.Name
				}
			}
		}
	}
	return r, nil
}

// toolCallItem is a Responses input item that makes a tool call, and
// toolResultItem one that answers a call.
func toolCallItem(t string) bool {
	switch t {
	case "function_call", "custom_tool_call", "tool_search_call":
		return true
	}
	return false
}

func toolResultItem(t string) bool {
	switch t {
	case "function_call_output", "custom_tool_call_output", "tool_search_output":
		return true
	}
	return false
}

// pairToolItems gives a Responses request's tool call that no result in
// the request answers a synthetic one, as pairToolMessages does for a Chat
// request: an upstream that checks the exchange on the chat form it turns
// the request into refuses a lone call — Volcengine's coding plan answers
// 400 "An assistant message with 'tool_calls' must be followed by tool
// messages responding to each 'tool_call_id'" (#1341), and OpenAI's own
// Responses API "No tool output found for function call". The result goes
// at the end of the run of calls and results the call is in, where a chat
// form has its tool messages. A result with no call is left to
// orphanedToolOutputs, and one naming a call another request carried
// (previous_response_id) stays as it is.
func pairToolItems(body []byte) []byte {
	if !bytes.Contains(body, []byte(`_call"`)) {
		return body
	}
	var q map[string]json.RawMessage
	if decodeJSON(body, &q) != nil {
		return body
	}
	var input []json.RawMessage
	if decodeJSON(q["input"], &input) != nil {
		return body
	}
	type item struct {
		Type   string `json:"type"`
		CallID string `json:"call_id"`
	}
	items := make([]item, len(input))
	answered := map[string]bool{}
	for i, raw := range input {
		if decodeJSON(raw, &items[i]) != nil {
			items[i] = item{}
		}
		if t := items[i].Type; (t == "function_call_output" || t == "custom_tool_call_output") && items[i].CallID != "" {
			answered[items[i].CallID] = true
		}
	}
	exchange := func(t string) bool {
		return toolCallItem(t) || toolResultItem(t)
	}
	out := make([]json.RawMessage, 0, len(input)+1)
	var synthetic []json.RawMessage // results owed by the run in progress
	for i, raw := range input {
		out = append(out, raw)
		it := items[i]
		if (it.Type == "function_call" || it.Type == "custom_tool_call") && it.CallID != "" && !answered[it.CallID] {
			kind := "function_call_output"
			if it.Type == "custom_tool_call" {
				kind = "custom_tool_call_output"
			}
			result, _ := marshalPlain(map[string]any{"type": kind, "call_id": it.CallID,
				"output": "[The result of this tool call is unavailable: the turn was interrupted.]"})
			synthetic = append(synthetic, result)
			answered[it.CallID] = true
		}
		if len(synthetic) > 0 && exchange(it.Type) && (i+1 == len(input) || !exchange(items[i+1].Type)) {
			out = append(out, synthetic...)
			synthetic = nil
		}
	}
	if len(out) == len(input) {
		return body
	}
	q["input"], _ = marshalPlain(out)
	encoded, err := marshalPlain(q)
	if err != nil {
		return body
	}
	return encoded
}

// mergeTurns joins consecutive messages of the same role, since the
// Responses API splits an assistant turn into one item per part.
func mergeTurns(msgs []Message) []Message {
	var out []Message
	for _, m := range msgs {
		if n := len(out); n > 0 && out[n-1].Role == m.Role {
			out[n-1].Parts = append(out[n-1].Parts, m.Parts...)
			continue
		}
		out = append(out, m)
	}
	return out
}

func responsesParts(raw json.RawMessage) []Part {
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
		ImageURL string `json:"image_url"`
	}
	decodeJSON(raw, &items)
	var out []Part
	for _, it := range items {
		switch it.Type {
		case "input_text", "output_text", "text":
			out = append(out, Part{Kind: Text, Text: it.Text})
		case "input_image":
			if it.ImageURL != "" {
				out = append(out, imagePart(it.ImageURL))
			}
		}
	}
	return out
}

// replaysReasoning is whether model, served by host's Responses API, wants
// a turn's reasoning text back between tool calls (DeepSeek: "The
// reasoning_text in the thinking mode must be passed back", #388). OpenAI's,
// xAI's, Copilot's and Azure's read only their own sealed reasoning.
func replaysReasoning(model, host string) bool {
	return strings.Contains(strings.ToLower(model), "deepseek") &&
		!slices.Contains([]string{"chatgpt.com", "api.openai.com", "api.x.ai", "api.githubcopilot.com"}, host) &&
		!strings.HasSuffix(host, ".openai.azure.com")
}

// stepKind is what a Responses input item is to a step of the model's: its
// reasoning, something it said or a call it made, what answered a call, or
// a message of the user's (or the system's) that starts a turn.
type stepKind int

const (
	stepOther stepKind = iota
	stepReasoning
	stepSaid
	stepCall
	stepOutput
	stepTurn
)

func stepKindOf(typ, role string) stepKind {
	switch {
	case typ == "reasoning":
		return stepReasoning
	case strings.HasSuffix(typ, "_output"):
		return stepOutput
	case strings.HasSuffix(typ, "_call"):
		return stepCall
	case (typ == "message" || typ == "") && role == "assistant":
		return stepSaid
	case typ == "message" || typ == "" && role != "":
		return stepTurn
	}
	return stepOther
}

// noReasoningItem is the reasoning item a step leads with when it has none
// of its own (reasonFirst).
func noReasoningItem() map[string]any {
	return map[string]any{"type": "reasoning", "summary": []any{},
		"content": []map[string]any{{"type": "reasoning_text", "text": noReasoning}}}
}

// reasonFirst has each step of the model's (a run of its reasoning, what it
// said and the calls it made, between a user message or a tool output and
// the next) that makes a call, or that follows a tool output, lead with a
// reasoning item: the step's first one moved ahead of the rest, else none.
// DeepSeek's /v1/responses reads a step as one assistant message whose
// thinking is the reasoning item it starts with, and turns the whole
// request away ("The `reasoning_text` in the thinking mode must be passed
// back to the API") when a step of the turn being continued starts with
// anything else: a call (#1592), or what the model said, as Codex sends a
// step that said something before its call with its reasoning between them,
// or with none, which magpie put between them (#1822). Checked against
// api.deepseek.com on 2026-10-12: [text, reasoning, call] and [output,
// text] are refused, [reasoning, text, call] and [output, reasoning, text]
// are taken. ok is false, and items comes back as it is, when every such
// step leads with one already.
func reasonFirst[T any](items []T, kinds []stepKind, none T) (led []T, ok bool) {
	step := func(k stepKind) bool { return k == stepReasoning || k == stepSaid || k == stepCall }
	afterOutput := false
	for i := 0; i < len(items); {
		if !step(kinds[i]) {
			switch kinds[i] {
			case stepOutput:
				afterOutput = true
			case stepTurn:
				afterOutput = false
			}
			if ok {
				led = append(led, items[i])
			}
			i++
			continue
		}
		j, calls, first := i, false, -1
		for ; j < len(items) && step(kinds[j]); j++ {
			calls = calls || kinds[j] == stepCall
			if kinds[j] == stepReasoning && first < 0 {
				first = j
			}
		}
		if (calls || afterOutput) && first != i {
			if !ok {
				led, ok = append(make([]T, 0, len(items)+1), items[:i]...), true
			}
			if first < 0 {
				led = append(led, none)
			} else {
				led = append(led, items[first])
			}
			for x := i; x < j; x++ {
				if x != first {
					led = append(led, items[x])
				}
			}
		} else if ok {
			led = append(led, items[i:j]...)
		}
		i = j
	}
	if !ok {
		return items, false
	}
	return led, true
}

// sealedReasoning is a reasoning item a Responses API sealed, fit to go
// back to one as it came: encrypted_content in it, and no reasoning text
// (which OpenAI and the ChatGPT backend refuse in input, #1411; one with
// text goes as before).
func sealedReasoning(p Part) bool {
	if !p.sealedBy(sealResponses) {
		return false
	}
	var it struct {
		Enc     string            `json:"encrypted_content"`
		Content []json.RawMessage `json:"content"`
	}
	return decodeJSON(p.Sealed, &it) == nil && it.Enc != "" && len(it.Content) == 0
}

// buildResponses renders a request for a Responses upstream.
func buildResponses(r *Request, model, host string, rejectTemp bool) []byte {

	replay := replaysReasoning(model, host)
	var input []any

	for _, m := range dropSecondAnswers(r.Messages) {
		var content []map[string]any
		flushMsg := func() {
			if len(content) == 0 {
				return
			}
			input = append(input, map[string]any{"type": "message", "role": m.Role, "content": content})
			content = nil
		}
		for _, p := range m.Parts {
			switch p.Kind {
			case Text:
				if p.Text == "" && p.Signature != "" {
					continue
				}
				t := "input_text"
				if m.Role == "assistant" {
					t = "output_text"
				}
				content = append(content, map[string]any{"type": t, "text": p.Text})
			case File:
				t := "input_text"
				if m.Role == "assistant" {
					t = "output_text"
				}
				content = append(content, map[string]any{"type": t, "text": attachmentText(p)})
			case Image:
				if m.Role != "assistant" {
					content = append(content, map[string]any{"type": "input_image", "detail": "auto", "image_url": dataURL(p)})
				}
			case Thinking:
				if !replay && sealedReasoning(p) {

					flushMsg()
					input = append(input, p.Sealed)
				} else if replay && p.Text != "" {
					flushMsg()
					input = append(input, map[string]any{"type": "reasoning", "summary": []any{},
						"content": []map[string]any{{"type": "reasoning_text", "text": p.Text}}})
				}
			case ToolCall:
				flushMsg()
				id := p.ID
				if id == "" {
					id = "call_" + newID()
				}
				input = append(input, map[string]any{"type": "function_call", "call_id": id, "name": p.Name, "arguments": argsString(p)})
			case ToolResult:
				flushMsg()
				if p.Standalone != nil {
					input = append(input, p.Standalone)
					continue
				}
				var output any = p.Text
				if len(p.Images) > 0 {
					// an output can be a list of text and images
					var items []map[string]any
					if strings.TrimSpace(p.Text) != "" {
						items = append(items, map[string]any{"type": "input_text", "text": p.Text})
					}
					for _, im := range p.Images {
						items = append(items, map[string]any{"type": "input_image", "detail": "auto", "image_url": dataURL(im)})
					}
					output = items
				}
				input = append(input, map[string]any{"type": "function_call_output", "call_id": p.CallID, "output": output})
			}
		}
		flushMsg()
	}
	if replay {

		kinds := make([]stepKind, len(input))
		for i, it := range input {
			var k struct {
				Type string `json:"type"`
				Role string `json:"role"`
			}
			switch v := it.(type) {
			case map[string]any:
				k.Type, _ = v["type"].(string)
				k.Role, _ = v["role"].(string)
			case json.RawMessage:
				decodeJSON(v, &k)
			}
			kinds[i] = stepKindOf(k.Type, k.Role)
		}
		input, _ = reasonFirst(input, kinds, any(noReasoningItem()))
	}
	if input == nil {
		input = []any{}
	}
	out := map[string]any{"model": model, "input": input, "stream": r.Stream, "store": false}
	if len(r.ClientMetadata) > 0 {
		out["client_metadata"] = r.ClientMetadata
	}
	if len(r.AccessPrograms) > 0 {
		out["access_programs"] = r.AccessPrograms
	}
	if len(r.Text) > 0 || r.Format != nil {
		text := map[string]any{}
		decodeJSON(r.Text, &text)
		if r.Format != nil {
			text["format"] = r.Format.responses()
		}
		out["text"] = text
	}
	if r.CacheKey != "" {
		out["prompt_cache_key"] = r.CacheKey
	}
	if r.System != "" {
		out["instructions"] = r.System
	}
	if r.MaxTokens > 0 {
		out["max_output_tokens"] = r.MaxTokens
	}
	if !rejectTemp {
		if r.Temp != nil {
			out["temperature"] = *r.Temp
		}
		if r.TopP != nil {
			out["top_p"] = *r.TopP
		}
	}

	if r.Fast && (host == "chatgpt.com" || host == "api.openai.com") {
		out["service_tier"] = "priority"
	}

	if r.Ultrafast && host == "chatgpt.com" {
		out["service_tier"] = "ultrafast"
	}
	if r.OwnTier && r.Tier != "" {
		out["service_tier"] = r.Tier
	}
	if r.Effort != "" {
		out["reasoning"] = map[string]any{"effort": r.Effort, "summary": "auto"}
	} else if r.Thinking {
		out["reasoning"] = map[string]any{"summary": "auto"}
	}
	// the client's include goes on as the request would have without
	// magpie. Sealed reasoning is asked for only with reasoning, as Codex
	// asks for it: OpenAI refuses it of a model that doesn't reason. What
	// comes back sealed goes to a Responses client as it came, and the
	// input's goes back to a Responses upstream, as it would without magpie
	// (#1445); an account or vendor that can't read another's turns it away,
	// and is asked again without it (foreignReasoning, withoutRefused).
	var include []string
	for _, v := range r.Include {
		if v == "" || slices.Contains(include, v) || (v == "reasoning.encrypted_content" && out["reasoning"] == nil) {
			continue
		}
		include = append(include, v)
	}
	if len(r.Tools) > 0 || r.WebSearch {
		var tools []map[string]any
		for _, t := range r.Tools {

			tool := map[string]any{"type": "function", "name": t.Name, "description": t.Description, "strict": t.Strict}
			if len(t.Schema) > 0 {
				tool["parameters"] = t.Schema
			}
			tools = append(tools, tool)
		}
		if r.WebSearch {
			tools = append(tools, map[string]any{"type": "web_search"})

			if host != "api.x.ai" && !slices.Contains(include, "web_search_call.action.sources") {
				include = append(include, "web_search_call.action.sources")
			}
		}
		out["tools"] = tools
		switch {
		case r.ToolChoice == "auto" || r.ToolChoice == "none" || r.ToolChoice == "required":
			out["tool_choice"] = r.ToolChoice
		case strings.HasPrefix(r.ToolChoice, "name:"):
			out["tool_choice"] = map[string]any{"type": "function", "name": strings.TrimPrefix(r.ToolChoice, "name:")}
		}
		if r.Parallel != nil {
			out["parallel_tool_calls"] = *r.Parallel
		}
	}
	if len(include) > 0 {
		out["include"] = include
	}
	b, _ := json.Marshal(out)

	return pairToolItems(b)
}

func (u Usage) responses() map[string]any {
	in := u.prompt()
	return map[string]any{"input_tokens": in, "output_tokens": u.Output, "total_tokens": in + u.Output,
		"input_tokens_details":  map[string]any{"cached_tokens": u.CacheRead, "cache_write_tokens": u.CacheWrite},
		"output_tokens_details": map[string]any{"reasoning_tokens": u.Reasoning}}
}

// saidError puts the type, code and param of the vendor's own error (an
// Event's Said) on the error a stream encoder writes, so a request the
// vendor refused reads as refused (invalid_request_error) to the agent and
// to the hold that decides whether to ask again, not as magpie's api_error
// (magpie-releases#18). Without Said it is left as it was.
func saidError(failed map[string]any, said json.RawMessage) {
	e := gjson.GetBytes(said, "error")
	for _, k := range []string{"type", "code", "param"} {
		if v := e.Get(k); v.Type == gjson.String && v.String() != "" {
			failed[k] = v.String()
		}
	}
}

// responsesEncoder writes events as a Responses stream. Codex reads the
// full item from output_item.done and usage from response.completed.
type responsesEncoder struct {
	w       *sseWriter
	model   string
	id      string
	created int64
	seq     int
	started bool
	item    int             // index of the open output item
	open    Kind            // kind of the open item
	itemID  string          // id of the open item
	text    strings.Builder // text of the open message / reasoning
	// a reasoning item began upstream (KThinkStart): the next thinking
	// opens an item of its own, by thinkID when the upstream named it
	thinkNew bool
	thinkID  string
	output   []any
	col      collector
	named    map[string]nsTool // the request's namespaced tools
	// call is the open function_call item's id and name, kept here: the
	// collector's last part isn't it once a hidden thinking block (signed
	// with no text, or redacted) has come after the call (#1786)
	call Part
	// a message's output_item.done, and the events after it, wait here
	// until its phase is known (phase.go, #2437)
	heldMsg map[string]any
	held    []heldEvent
}

type heldEvent struct {
	typ    string
	fields map[string]any
}

// callTo names the tool a function_call item is to as the client knows it:
// a namespaced tool by its name and namespace, not the flat name the model
// used.
func callTo(item map[string]any, name string, named map[string]nsTool) map[string]any {
	if q := named[name]; q.Search {

		args, _ := item["arguments"].(string)
		delete(item, "name")
		item["type"], item["execution"], item["arguments"] = "tool_search_call", "client", parseArgs(args)
	} else if q.Custom {

		args, _ := item["arguments"].(string)
		delete(item, "arguments")
		item["type"], item["name"], item["input"] = "custom_tool_call", q.Name, ""
		if args != "" {
			item["input"] = customInput(args)
		}
		if q.Namespace != "" {
			item["namespace"] = q.Namespace
		}
	} else if q, ok := named[name]; ok {
		item["name"], item["namespace"] = q.Name, q.Namespace

		item["encrypted_function_args"] = []any{}
	} else {
		item["name"] = name
	}
	return item
}

// itemPrefix is the prefix of a call item's id, by its type: OpenAI turns
// away a tool_search_call or custom_tool_call whose id isn't its own kind
// ("Invalid 'input[98].id': 'fc_…'. Expected an ID that begins with
// 'tsc'", openaiItemPrefix), and Codex hands the item back to it when the
// conversation goes there.
func itemPrefix(item map[string]any) string {
	if t, _ := item["type"].(string); openaiItemPrefix[t] != "" {
		return openaiItemPrefix[t]
	}
	return "fc_"
}

func (e *responsesEncoder) send(typ string, fields map[string]any) {
	if e.heldMsg != nil {
		e.held = append(e.held, heldEvent{typ, fields})
		if len(e.held) > heldMost {
			e.release("")
		}
		return
	}
	e.write(typ, fields)
}

func (e *responsesEncoder) write(typ string, fields map[string]any) {
	fields["type"] = typ
	fields["sequence_number"] = e.seq
	e.seq++
	e.w.event(typ, fields)
}

// release sends the waiting message's events, the message given phase
// (or none, for "").
func (e *responsesEncoder) release(phase string) {
	if e.heldMsg == nil {
		return
	}
	if phase != "" {
		e.heldMsg["phase"] = phase
	}
	held := e.held
	e.heldMsg, e.held = nil, nil
	for _, h := range held {
		e.write(h.typ, h.fields)
	}
}

func (e *responsesEncoder) response(status string, extra map[string]any) map[string]any {

	output := e.output
	if output == nil {
		output = []any{}
	}
	out := map[string]any{"id": e.id, "object": "response", "created_at": e.created, "status": status,
		"model": e.model, "output": output, "parallel_tool_calls": true, "tool_choice": "auto", "tools": []any{}}
	for k, v := range extra {
		out[k] = v
	}
	return out
}

func (e *responsesEncoder) start(ev Event) {
	if e.started {
		return
	}
	e.started, e.item = true, -1
	e.id, e.created = ev.MsgID, time.Now().Unix()
	if e.id == "" {
		e.id = newID()
	}
	if !strings.HasPrefix(e.id, "resp_") {
		e.id = "resp_" + e.id
	}
	if ev.Model != "" {
		e.model = ev.Model
	}
	e.send("response.created", map[string]any{"response": e.response("in_progress", nil)})
	e.send("response.in_progress", map[string]any{"response": e.response("in_progress", nil)})
}

func (e *responsesEncoder) closeItem() {
	e.closeAs(nil)
}

// closeAs closes the open item, as sealed when the upstream wrote it
// whole (a reasoning item it sealed).
func (e *responsesEncoder) closeAs(sealed json.RawMessage) {
	if e.open == "" {
		return
	}
	var item any
	switch e.open {
	case Text:
		t := e.text.String()
		e.send("response.output_text.done", map[string]any{"item_id": e.itemID, "output_index": e.item, "content_index": 0, "text": t, "logprobs": []any{}})
		part := map[string]any{"type": "output_text", "text": t, "annotations": []any{}, "logprobs": []any{}}
		e.send("response.content_part.done", map[string]any{"item_id": e.itemID, "output_index": e.item, "content_index": 0, "part": part})
		m := map[string]any{"id": e.itemID, "type": "message", "role": "assistant", "status": "completed", "content": []map[string]any{part}}

		e.release(phaseCommentary)
		item, e.heldMsg = m, m
	case Thinking:
		t := e.text.String()
		e.send("response.reasoning_summary_text.done", map[string]any{"item_id": e.itemID, "output_index": e.item, "summary_index": 0, "text": t})
		part := map[string]any{"type": "summary_text", "text": t}
		e.send("response.reasoning_summary_part.done", map[string]any{"item_id": e.itemID, "output_index": e.item, "summary_index": 0, "part": part})
		item = map[string]any{"id": e.itemID, "type": "reasoning", "status": "completed", "summary": []map[string]any{part}}
		if sealed != nil {
			item = sealed
		}
	case ToolCall:
		args := strings.TrimSpace(e.text.String())
		if args == "" {
			args = "{}"
		}
		p := e.call
		if q := e.named[p.Name]; !q.Search && !q.Custom {
			e.send("response.function_call_arguments.done", callTo(map[string]any{"item_id": e.itemID, "output_index": e.item, "call_id": p.ID, "arguments": args}, p.Name, e.named))
		}
		item = callTo(map[string]any{"id": e.itemID, "type": "function_call", "status": "completed", "call_id": p.ID, "arguments": args}, p.Name, e.named)
	}
	e.send("response.output_item.done", map[string]any{"output_index": e.item, "item": item})
	e.output = append(e.output, item)
	e.open, e.text = "", strings.Builder{}
}

func (e *responsesEncoder) openItem(k Kind, prefix string, item map[string]any) {
	e.closeItem()
	if k != Thinking {
		e.release(phaseCommentary)
	}
	e.item++
	e.open, e.itemID = k, prefix+newID()
	if k == Thinking && e.thinkID != "" {
		e.itemID = e.thinkID
	}
	e.thinkNew, e.thinkID = false, ""
	item["id"] = e.itemID
	item["status"] = "in_progress"
	e.send("response.output_item.added", map[string]any{"output_index": e.item, "item": item})
}

func (e *responsesEncoder) event(ev Event) {
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
		if e.open != Text {
			e.openItem(Text, "msg_", map[string]any{"type": "message", "role": "assistant", "content": []any{}})
			e.send("response.content_part.added", map[string]any{"item_id": e.itemID, "output_index": e.item, "content_index": 0,
				"part": map[string]any{"type": "output_text", "text": "", "annotations": []any{}, "logprobs": []any{}}})
		}
		e.text.WriteString(ev.Text)
		e.send("response.output_text.delta", map[string]any{"item_id": e.itemID, "output_index": e.item, "content_index": 0, "delta": ev.Text, "logprobs": []any{}})
	case KThink:
		if ev.Text == "" {
			return
		}
		if e.open != Thinking || e.thinkNew {
			e.openItem(Thinking, "rs_", map[string]any{"type": "reasoning", "summary": []any{}})
			e.send("response.reasoning_summary_part.added", map[string]any{"item_id": e.itemID, "output_index": e.item, "summary_index": 0,
				"part": map[string]any{"type": "summary_text", "text": ""}})
		}
		e.text.WriteString(ev.Text)
		e.send("response.reasoning_summary_text.delta", map[string]any{"item_id": e.itemID, "output_index": e.item, "summary_index": 0, "delta": ev.Text})
	case KThinkStart:
		e.thinkNew, e.thinkID = true, ""
		if strings.HasPrefix(ev.ID, "rs_") {
			e.thinkID = ev.ID
		}
	case KSealed:
		if ev.Name != sealResponses {
			break
		}
		raw := json.RawMessage(ev.Text)
		if e.open == Thinking && !e.thinkNew && (ev.ID == "" || ev.ID == e.itemID) {

			e.closeAs(raw)
			break
		}

		e.closeItem()
		e.item++
		e.thinkNew, e.thinkID = false, ""
		e.send("response.output_item.added", map[string]any{"output_index": e.item, "item": raw})
		e.send("response.output_item.done", map[string]any{"output_index": e.item, "item": raw})
		e.output = append(e.output, raw)
	case KSearch:

		e.closeItem()
		e.release(phaseCommentary)
		e.item++
		item := responsesSearchItem(ev.Text, ev.Hits)
		e.send("response.output_item.added", map[string]any{"output_index": e.item, "item": item})
		e.send("response.web_search_call.completed", map[string]any{"output_index": e.item, "item_id": item["id"]})
		e.send("response.output_item.done", map[string]any{"output_index": e.item, "item": item})
		e.output = append(e.output, item)
	case KToolStart:
		if ev.ID == "" {
			ev.ID = "call_" + newID()
		}

		item := callTo(map[string]any{"type": "function_call", "call_id": ev.ID, "arguments": ""}, ev.Name, e.named)
		e.openItem(ToolCall, itemPrefix(item), item)
		e.call = Part{Kind: ToolCall, ID: ev.ID, Name: ev.Name}
		e.col.add(ev)
		return
	case KToolArgs:
		if e.open == ToolCall && ev.Text != "" {
			e.text.WriteString(ev.Text)

			if !e.named[e.call.Name].Custom {
				e.send("response.function_call_arguments.delta", map[string]any{"item_id": e.itemID, "output_index": e.item, "delta": ev.Text})
			}
		}
	case KError:
		e.closeItem()
		e.release("")
		code := "server_error"
		if ev.Code != "" {
			code = ev.Code
		}
		failed := map[string]any{"code": code, "message": ev.Text}
		saidError(failed, ev.Said)
		e.send("response.failed", map[string]any{"response": e.response("failed", map[string]any{"error": failed})})
	}
	e.col.add(ev)
}

// keepalive is response.in_progress again: Codex's idle timeout counts
// events only, an SSE comment never reaching it, and skips this one.
func (e *responsesEncoder) keepalive() {
	if !e.started {
		e.start(Event{})
		return
	}

	e.write("response.in_progress", map[string]any{"response": e.response("in_progress", nil)})
}

func (e *responsesEncoder) finish() {
	if !e.started {
		e.start(Event{})
	}
	e.closeItem()
	res := e.col.finish()
	status, typ := "completed", "response.completed"
	extra := map[string]any{"usage": res.Usage.responses(), "incomplete_details": nil}
	if res.Stop == "length" || res.Stop == "filter" {
		status, typ = "incomplete", "response.incomplete"
		reason := "max_output_tokens"
		if res.Stop == "filter" {
			reason = "content_filter"
		}
		extra["incomplete_details"] = map[string]any{"reason": reason}
	}
	if status == "completed" {
		e.release(phaseFinal)
	}
	e.release("")
	e.send(typ, map[string]any{"response": e.response(status, extra)})
}

// responsesSearchItem keeps a completed search in both Responses reply forms.
// Keep query for older clients and queries for clients using the current API.
func responsesSearchItem(query string, hits []Hit) map[string]any {
	sources := make([]map[string]any, 0, len(hits))
	for _, h := range hits {
		if h.URL != "" {
			sources = append(sources, map[string]any{"type": "url", "url": h.URL})
		}
	}
	return map[string]any{
		"id": "ws_" + newID(), "type": "web_search_call", "status": "completed",
		"action": map[string]any{"type": "search", "query": query, "queries": []string{query}, "sources": sources},
	}
}

// renderResponses is the non-streaming reply.
func renderResponses(res Result, model string, named map[string]nsTool) []byte {
	output := []any{}
	for _, p := range res.Parts {
		switch p.Kind {
		case Text:
			if p.Text == "" && p.Signature != "" {
				continue
			}
			output = append(output, map[string]any{"id": "msg_" + newID(), "type": "message", "role": "assistant", "status": "completed",
				"content": []map[string]any{{"type": "output_text", "text": p.Text, "annotations": []any{}}}})
		case Thinking:
			if p.sealedBy(sealResponses) {

				output = append(output, p.Sealed)
				continue
			}
			if p.Text == "" {
				continue
			}
			output = append(output, map[string]any{"id": "rs_" + newID(), "type": "reasoning", "status": "completed",
				"summary": []map[string]any{{"type": "summary_text", "text": p.Text}}})
		case ToolCall:
			id := p.ID
			if id == "" {
				id = "call_" + newID()
			}
			item := callTo(map[string]any{"type": "function_call", "status": "completed",
				"call_id": id, "arguments": argsString(p)}, p.Name, named)
			item["id"] = itemPrefix(item) + newID()
			output = append(output, item)
		case Search:
			output = append(output, responsesSearchItem(p.Text, p.Hits))
		}
	}
	id := res.ID
	if id == "" {
		id = newID()
	}
	if !strings.HasPrefix(id, "resp_") {
		id = "resp_" + id
	}
	if res.Model != "" {
		model = res.Model
	}
	status := "completed"
	var incomplete any
	switch res.Stop {
	case "length":
		status, incomplete = "incomplete", map[string]any{"reason": "max_output_tokens"}
	case "filter":

		status, incomplete = "incomplete", map[string]any{"reason": "content_filter"}
	}
	phaseOutput(output, status == "completed")
	b, _ := json.Marshal(map[string]any{"id": id, "object": "response", "created_at": time.Now().Unix(), "status": status,
		"model": model, "output": output, "usage": res.Usage.responses(), "incomplete_details": incomplete,
		"parallel_tool_calls": true, "tool_choice": "auto", "tools": []any{}, "error": nil})
	return b
}
