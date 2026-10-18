// INPUT: encoding/json, fmt, regexp, strconv, strings.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	json "encoding/json"
	fmt "fmt"
	regexp "regexp"
	strconv "strconv"
	strings "strings"
)

// skipSignature stands in for a thought signature on a function call the
// model didn't make here: Google checks the ones Gemini 3 hands out, and
// this one tells it not to.
const skipSignature = "skip_thought_signature_validator"

var unsafeToolID = regexp.MustCompile(`[^a-zA-Z0-9_-]`)

// buildCodeAssistSent is buildCodeAssist with the id the envelope is
// addressed to already worked out, for the caller that names the envelope
// with that same id and would otherwise work it out a second time. There is
// no model but that id in here: on Antigravity the two differ, and the one
// the request goes out under is the one everything below is shaped from.
// agent "vertex" is Vertex AI's request (buildVertex).
func buildCodeAssistSent(r *Request, sent, agent string) []byte {
	ag := agent == "antigravity"
	vx := agent == "vertex"
	at := ""

	claude := strings.Contains(strings.ToLower(sent), "claude")
	toolID := func(id string) string {
		if !ag || id == "" {
			return id
		}
		return unsafeToolID.ReplaceAllString(id, "_")
	}
	names := map[string]string{}
	var contents []map[string]any
	for _, m := range dropSecondAnswers(r.Messages) {
		role := "user"
		if m.Role == "assistant" {
			role = "model"
		}
		var parts, after []map[string]any // after: tools' images held back
		toolImage := map[int]bool{}

		signed := false
		for _, p := range m.Parts {
			if p.Kind == ToolCall && p.Signature != "" {
				signed = true
			}
		}
		for _, p := range m.Parts {
			switch p.Kind {
			case Text:
				if p.Text != "" || (p.Signature != "" && !claude) {
					part := map[string]any{"text": p.Text}
					if p.Signature != "" && !claude {

						part["thoughtSignature"] = p.Signature
					}
					parts = append(parts, part)
				}
			case Image, File:
				if p.Data != "" {
					parts = append(parts, map[string]any{"inlineData": map[string]any{"mimeType": p.MediaType, "data": p.Data}})
				} else if p.URL != "" {
					parts = append(parts, map[string]any{"fileData": map[string]any{"mimeType": p.MediaType, "fileUri": p.URL}})
				}
			case ToolCall:
				names[p.ID] = p.Name
				call := map[string]any{"name": p.Name, "args": json.RawMessage(argsOf(p))}
				if id := toolID(p.ID); id != "" {
					call["id"] = id
				}
				part := map[string]any{"functionCall": call}
				switch {
				case claude:
					part["thoughtSignature"] = skipSignature
				case p.Signature != "":

					part["thoughtSignature"] = p.Signature
				case !signed:
					part["thoughtSignature"] = skipSignature
				}
				parts = append(parts, part)
			case ToolResult:
				name := names[p.CallID]
				if name == "" {
					name = "tool"
				}
				key := "output"
				switch {
				case ag:
					key = "result"
				case p.IsError:
					key = "error"
				}
				res := map[string]any{"name": name, "response": map[string]any{key: p.Text}}
				if id := toolID(p.CallID); id != "" {
					res["id"] = id
				}
				parts = append(parts, map[string]any{"functionResponse": res})
				// the images a tool returned follow its response, as Gemini
				// CLI sends a file it read. Antigravity hands a Claude model
				// the turn back as Anthropic blocks, where an image between
				// two responses splits them from their calls ("tool_use ids
				// were found without tool_result blocks immediately after"):
				// there the images wait until every response is in, each
				// after a line saying whose it is.
				var ims []map[string]any
				for _, im := range p.Images {
					if im.Data != "" {
						ims = append(ims, map[string]any{"inlineData": map[string]any{"mimeType": im.MediaType, "data": im.Data}})
					} else if im.URL != "" {
						ims = append(ims, map[string]any{"fileData": map[string]any{"mimeType": im.MediaType, "fileUri": im.URL}})
					}
				}
				if ag && claude && len(ims) > 0 {
					label := fmt.Sprintf("Image returned by tool %s (call %s):", name, toolID(p.CallID))
					if len(ims) > 1 {
						label = fmt.Sprintf("%d images returned by tool %s (call %s):", len(ims), name, toolID(p.CallID))
					}
					after = append(after, map[string]any{"text": label})
					after = append(after, ims...)
				} else {
					for _, im := range ims {
						toolImage[len(parts)] = true
						parts = append(parts, im)
					}
				}
			}

		}
		if len(after) > 0 {
			// the responses lead the turn, as an Anthropic tool_result
			// turn has to; what the user said and the tools' images follow
			var lead, rest []map[string]any
			for _, part := range parts {
				if part["functionResponse"] != nil {
					lead = append(lead, part)
				} else {
					rest = append(rest, part)
				}
			}
			parts = append(append(lead, after...), rest...)
		}
		// what the user said goes in a turn of its own after the tools'
		// responses: Gemini answers a turn mixing a functionResponse with
		// text "Requests ending with a model turn are not supported" (#1533,
		// yycsyu: Codex's compaction prompt after a tool's output), and takes
		// the two as separate turns. A Claude model on Antigravity keeps them
		// in one, the tool_result turn Anthropic reads them as.
		var said []map[string]any
		if role == "user" && !(ag && claude) && hasResponse(parts) {
			var tools []map[string]any
			for i, part := range parts {
				if part["functionResponse"] != nil || toolImage[i] {
					tools = append(tools, part)
				} else {
					said = append(said, part)
				}
			}
			parts = tools
		}
		for _, ps := range [][]map[string]any{parts, said} {
			if len(ps) == 0 {
				continue
			}
			if n := len(contents); n > 0 && contents[n-1]["role"] == role && (role != "user" || hasResponse(contents[n-1]["parts"].([]map[string]any)) == hasResponse(ps)) {
				contents[n-1]["parts"] = append(contents[n-1]["parts"].([]map[string]any), ps...)
				continue
			}
			contents = append(contents, map[string]any{"role": role, "parts": ps})
		}
	}
	contents = pairFunctionTurns(contents, ag, ag && claude)
	if len(contents) > 0 && contents[0]["role"] == "model" {

		contents = append([]map[string]any{{"role": "user", "parts": []map[string]any{{"text": "[Continuing from previous AI thoughts...]"}}}}, contents...)
	}
	if n := len(contents); n > 0 && contents[n-1]["role"] == "model" {

		contents = append(contents, map[string]any{"role": "user", "parts": []map[string]any{{"text": modelLastAsk(r)}}})
	}
	req := map[string]any{"contents": contents}
	if r.System != "" {
		req["systemInstruction"] = map[string]any{"role": "user", "parts": []map[string]any{{"text": r.System}}}
	}

	if len(r.Tools) > 0 && r.ToolChoice != "none" {
		var decls []map[string]any
		for _, t := range r.Tools {
			d := map[string]any{"name": t.Name}
			if t.Description != "" {
				d["description"] = t.Description
			}
			schema := t.Schema
			if len(schema) == 0 {
				schema = json.RawMessage(`{"type":"object","properties":{}}`)
			}
			if ag {
				d["parameters"] = plainSchema(schema)
			} else {
				d["parametersJsonSchema"] = schema
			}
			decls = append(decls, d)
		}
		req["tools"] = []map[string]any{{"functionDeclarations": decls}}
		mode := "AUTO"
		fc := map[string]any{}
		switch {
		case r.ToolChoice == "required":
			mode = "ANY"
		case strings.HasPrefix(r.ToolChoice, "name:"):
			mode = "ANY"
			fc["allowedFunctionNames"] = []string{strings.TrimPrefix(r.ToolChoice, "name:")}
		case ag && claude:

			mode = "VALIDATED"
		}
		fc["mode"] = mode
		req["toolConfig"] = map[string]any{"functionCallingConfig": fc}
	} else if r.WebSearch {

		req["tools"] = []map[string]any{{"googleSearch": map[string]any{}}}
	}

	gen := map[string]any{}
	if r.MaxTokens > 0 && (!ag || claude) {
		gen["maxOutputTokens"] = r.MaxTokens
	}
	if r.Temp != nil {
		gen["temperature"] = *r.Temp
	}
	if r.TopP != nil {
		gen["topP"] = *r.TopP
	}
	if len(r.Stop) > 0 {
		gen["stopSequences"] = r.Stop
	}
	tc := thinkingConfig(r, sent, claude, at)
	if vx {
		tc = vertexThinking(r, sent)
	}
	if tc != nil {
		gen["thinkingConfig"] = tc

		if b, ok := tc["thinkingBudget"].(int); ok && claude && gen["maxOutputTokens"] == nil {
			gen["maxOutputTokens"] = b + 32000
		}
	}
	if r.Format != nil {

		gen["responseMimeType"] = "application/json"
		if sc := r.Format.schema(); len(sc) > 0 {
			gen["responseJsonSchema"] = sc
		}
	}
	if DrawsID(sent) {

		gen["responseModalities"] = []string{"TEXT", "IMAGE"}
	}
	if len(gen) > 0 {
		req["generationConfig"] = gen
	}
	b, _ := json.Marshal(map[string]any{"model": sent, "request": req})
	return b
}

// dropSecondAnswers leaves out a tool result whose call an earlier result
// already answered, with the images it carried: the first answer stands, as
// pairToolMessages and pairToolBlocks keep it for a Chat or Messages
// upstream. Google turns a call turn away when its functionResponse count
// differs from its functionCall count, so a client that answers one call
// twice (#2262) had every later turn refused. A call made again under the
// same id takes an answer of its own, and a result with no id is left as
// it is. msgs is not changed: a fallback builds from the same request.
func dropSecondAnswers(msgs []Message) []Message {
	var out []Message
	answered := map[string]bool{}
	for i, m := range msgs {
		var parts []Part
		for j, p := range m.Parts {
			switch {
			case p.Kind == ToolCall && p.ID != "":
				delete(answered, p.ID)
			case p.Kind == ToolResult && p.CallID != "" && p.Standalone == nil:
				if answered[p.CallID] {
					if parts == nil {
						parts = append(make([]Part, 0, len(m.Parts)), m.Parts[:j]...)
					}
					if out == nil {
						out = append(make([]Message, 0, len(msgs)), msgs[:i]...)
					}
					continue
				}
				answered[p.CallID] = true
			}
			if parts != nil {
				parts = append(parts, p)
			}
		}
		if out == nil {
			continue
		}
		if parts != nil {
			if len(parts) == 0 {
				continue
			}
			m.Parts = parts
		}
		out = append(out, m)
	}
	if out == nil {
		return msgs
	}
	return out
}

// interruptedToolNote is the result a call left unanswered is answered
// with, the words the chat build gives such a call (pairToolMessages).
const interruptedToolNote = "[The result of this tool call is unavailable: the turn was interrupted.]"

// pairFunctionTurns answers a model turn's function calls in the turn
// right after it, as Google's API reads its histories ("Please ensure
// that the number of function response parts is equal to the number of
// function call parts of the function call turn"): a call the client
// answers nowhere — an interrupted turn leaves its call pending, and
// Google turns every later turn of the conversation away with 400 over
// the one — is answered with the synthetic result the chat build gives
// such a call (pairToolMessages, #1275). The answer goes in the turn that
// follows the calls when that turn already holds answers, and otherwise
// in a turn of its own before it: a user turn mixing a functionResponse
// with text is turned away too (#1533). A Claude model on Antigravity
// (agClaude) keeps the answer and what the user said in one turn, the
// tool_result turn Anthropic reads them as. A call with no id counts as
// answered by a response of its name with no id.
func pairFunctionTurns(contents []map[string]any, ag, agClaude bool) []map[string]any {
	if len(contents) == 0 {
		return contents
	}
	partsOf := func(c map[string]any) []map[string]any {
		ps, _ := c["parts"].([]map[string]any)
		return ps
	}
	answered := map[string]bool{}
	answeredName := map[string]bool{}
	for _, c := range contents {
		for _, p := range partsOf(c) {
			if fr, ok := p["functionResponse"].(map[string]any); ok {
				if id, _ := fr["id"].(string); id != "" {
					answered[id] = true
				} else {
					name, _ := fr["name"].(string)
					answeredName[name] = true
				}
			}
		}
	}
	key := "output"
	if ag {
		key = "result"
	}
	out := make([]map[string]any, 0, len(contents)+1)
	for i, c := range contents {
		out = append(out, c)
		if c["role"] != "model" {
			continue
		}
		var synth []map[string]any
		for _, p := range partsOf(c) {
			fc, ok := p["functionCall"].(map[string]any)
			if !ok {
				continue
			}
			id, _ := fc["id"].(string)
			name, _ := fc["name"].(string)
			if id != "" && answered[id] || id == "" && answeredName[name] {
				continue
			}
			res := map[string]any{"name": name, "response": map[string]any{key: interruptedToolNote}}
			if id != "" {
				res["id"] = id
			}
			synth = append(synth, map[string]any{"functionResponse": res})
		}
		if len(synth) == 0 {
			continue
		}
		if i+1 < len(contents) && contents[i+1]["role"] == "user" && (agClaude || hasResponse(partsOf(contents[i+1]))) {
			contents[i+1]["parts"] = append(synth, partsOf(contents[i+1])...)
			continue
		}
		out = append(out, map[string]any{"role": "user", "parts": synth})
	}
	return out
}

// thinkingConfig says how hard the model should think: a level for
// Gemini 3 and later (geminiGeneration), a budget for the rest. A model
// that doesn't think gets none, and one whose id already says its level,
// at (Antigravity's gemini-3.7-flash-low), is told that level and no
// other — the effort asked for picked that id. Told no level, Antigravity's variants all but
// stop thinking once the request has tools, and give none of it back
// (#636); Antigravity's own tiered ids, sent a level, don't.
//
// Gemini 3's levels: medium goes as medium to Flash, and up to high to
// Pro, which took low and high only (3 Pro). A later Gemini is sent the
// medium it was asked, already fitted to its own levels (fitFor):
// gemini-4-argon has low, medium and high (#2083). Minimal goes only to a
// Flash variant at minimal: 3.7 and 3.8 Flash have none (AI Studio
// answers 3.8's with a 400), and the id here doesn't say which Flash serves it (Gemini
// CLI's 3.5 Flash goes out as 3.8 Flash where that is rolled out).
// Reasoning off otherwise (the auto mode classifier's least level) goes at
// low, which every Gemini 3 text model takes; an image model may have no
// low (3.1 Flash Image: minimal and high), and goes at high.
func thinkingConfig(r *Request, model string, claude bool, at string) map[string]any {
	m := strings.ToLower(model)
	if strings.HasPrefix(m, "gpt-oss") || claude && !strings.Contains(m, "thinking") {
		return nil
	}
	if r.Effort == "" && !r.Thinking {

		if !claude {
			return nil
		}
	}
	tc := map[string]any{"includeThoughts": true}
	if gen := geminiGeneration(m); gen >= 3 || strings.HasPrefix(m, "gemini-pro-agent") {
		effort := r.Effort
		if at != "" {
			effort = at
		}
		if effort != "" {
			level := "high"
			switch {
			case effort == "low", effort == "minimal" && at != "" && !strings.Contains(m, "flash"):
				level = "low"
			case effort == "minimal" && at != "":
				level = "minimal"
			case offEffort(effort) && !DrawsID(m):
				level = "low"
			case effort == "medium" && (strings.Contains(m, "flash") || gen > 3):
				level = "medium"
			}
			tc["thinkingLevel"] = level
		}
		return tc
	}
	budget := budgetOf(effortOf(r.Effort))
	if claude && r.MaxTokens > 0 && budget >= r.MaxTokens {
		budget = r.MaxTokens - 1
		if budget < 1024 {
			return nil
		}
	}
	tc["thinkingBudget"] = budget
	return tc
}

// geminiVersion is the major version in a Gemini model's id, as the id
// names it: gemini-4-argon, models/gemini-3.8-flash, gemini-2.5-pro.
var geminiVersion = regexp.MustCompile(`(?:^|/)gemini-(\d+)`)

// geminiGeneration is the generation of Gemini model names, 0 for an id
// that names none (a non-Gemini model, or an alias such as
// gemini-flash-latest, which may still be a 2.5). From Gemini 3 a model is
// told a thinking level; Gemini 4 turns a budget away with "Thinking
// budget is not supported for this model. Use thinking level instead."
// (#2083), so a new generation isn't sent one either.
func geminiGeneration(model string) int {
	m := geminiVersion.FindStringSubmatch(model)
	if m == nil {
		return 0
	}
	n, _ := strconv.Atoi(m[1])
	return n
}

// plainSchema is a JSON schema cut down to what Antigravity's function
// declarations take: an OpenAPI-style subset with no references, unions
// or keywords outside it.
func plainSchema(raw json.RawMessage) json.RawMessage {
	var root map[string]any
	if json.Unmarshal(raw, &root) != nil {
		return raw
	}
	defs := map[string]any{}
	for _, k := range []string{"$defs", "definitions"} {
		if d, ok := root[k].(map[string]any); ok {
			for n, v := range d {
				defs[n] = v
			}
		}
	}
	var walk func(v any, depth int) any
	walk = func(v any, depth int) any {
		switch x := v.(type) {
		case []any:
			out := make([]any, len(x))
			for i, e := range x {
				out[i] = walk(e, depth)
			}
			return out
		case map[string]any:
			if depth > 32 {
				return map[string]any{"type": "object"}
			}
			if ref, ok := x["$ref"].(string); ok {
				name := ref[strings.LastIndex(ref, "/")+1:]
				if d, ok := defs[name]; ok {
					return walk(d, depth+1)
				}
				return map[string]any{"type": "object"}
			}

			if all, ok := x["allOf"].([]any); ok {
				merged := map[string]any{}
				for k, v := range x {
					if k != "allOf" {
						merged[k] = v
					}
				}
				for _, e := range all {
					if m, ok := walk(e, depth+1).(map[string]any); ok {
						for k, v := range m {
							if k == "properties" {
								props, _ := merged["properties"].(map[string]any)
								if props == nil {
									props = map[string]any{}
								}
								for pk, pv := range v.(map[string]any) {
									props[pk] = pv
								}
								merged["properties"] = props
							} else if _, has := merged[k]; !has {
								merged[k] = v
							}
						}
					}
				}
				return walk(merged, depth+1)
			}

			for _, k := range []string{"anyOf", "oneOf"} {
				if alts, ok := x[k].([]any); ok {
					var pick map[string]any
					nullable := false
					for _, a := range alts {
						m, _ := a.(map[string]any)
						if m["type"] == "null" {
							nullable = true
						} else if pick == nil && m != nil {
							pick = m
						}
					}
					out := map[string]any{}
					if pick != nil {
						if m, ok := walk(pick, depth+1).(map[string]any); ok {
							out = m
						}
					}
					if d, ok := x["description"].(string); ok && out["description"] == nil {
						out["description"] = d
					}
					if nullable {
						out["nullable"] = true
					}
					if out["type"] == nil {
						out["type"] = "string"
					}
					return out
				}
			}
			out := map[string]any{}
			for k, v := range x {
				switch k {
				case "description", "nullable", "required", "pattern", "minimum", "maximum", "minLength", "maxLength",
					"minItems", "maxItems", "minProperties", "maxProperties", "propertyOrdering":
					out[k] = v
				case "exclusiveMinimum", "exclusiveMaximum":

					if n, ok := v.(float64); ok {
						if b := "m" + strings.TrimPrefix(k, "exclusiveM"); x[b] == nil {
							out[b] = n
						}
					}
				case "type":
					if ts, ok := v.([]any); ok {
						for _, t := range ts {
							if t == "null" {
								out["nullable"] = true
							} else if out["type"] == nil {
								out["type"] = t
							}
						}
					} else {
						out["type"] = v
					}
				case "properties":
					props := map[string]any{}
					if m, ok := v.(map[string]any); ok {
						for pk, pv := range m {
							props[pk] = walk(pv, depth+1)
						}
					}
					out[k] = props
				case "items":
					out[k] = walk(v, depth+1)
				case "enum":

					if es, ok := v.([]any); ok {
						strs := make([]any, 0, len(es))
						for _, e := range es {
							if s, ok := e.(string); ok {
								strs = append(strs, s)
							}
						}
						if len(strs) == len(es) {
							out[k] = strs
						}
					}
				default:

				}
			}
			if c, ok := x["const"]; ok {
				if s, ok := c.(string); ok {
					out["enum"] = []any{s}
				}
			}
			if out["type"] == nil {
				switch {
				case out["properties"] != nil:
					out["type"] = "object"
				case out["items"] != nil:
					out["type"] = "array"
				}
			}
			if req, ok := out["required"].([]any); ok {

				props, _ := out["properties"].(map[string]any)
				keep := []any{}
				for _, n := range req {
					if s, ok := n.(string); ok && props[s] != nil {
						keep = append(keep, s)
					}
				}
				if len(keep) == 0 {
					delete(out, "required")
				} else {
					out["required"] = keep
				}
			}
			return out
		}
		return v
	}
	b, err := json.Marshal(walk(root, 0))
	if err != nil {
		return raw
	}
	return b
}

// hasResponse says whether a Gemini turn's parts hold a tool's response.
func hasResponse(parts []map[string]any) bool {
	for _, p := range parts {
		if p["functionResponse"] != nil {
			return true
		}
	}
	return false
}
