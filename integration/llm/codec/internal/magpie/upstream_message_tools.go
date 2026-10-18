// INPUT: bytes, encoding/json, github.com/tidwall/gjson.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	bytes "bytes"
	json "encoding/json"
	gjson "github.com/tidwall/gjson"
)

// liftMessageTools is a Chat request with the tools its messages carry
// moved into its own tools, each once by name: a tool the request already
// has, or an earlier message gave, keeps that first definition, as a
// tool_addition's does (withoutDeferredTools). A system or developer
// message that carried only tools is left out, rather than sent on as an
// empty system message; one with content keeps its place without its
// tools. A request with no message tools, or one that doesn't parse, is
// as it came.
func liftMessageTools(body []byte) []byte {
	if !bytes.Contains(body, []byte(`"tools"`)) {
		return body
	}
	if len(gjson.GetBytes(body, "messages.#.tools").Array()) == 0 {
		return body
	}
	var v struct {
		Messages []json.RawMessage `json:"messages"`
		Tools    []json.RawMessage `json:"tools"`
	}
	if json.Unmarshal(body, &v) != nil {
		return body
	}
	have := map[string]bool{}
	for _, t := range v.Tools {
		have[chatToolKey(gjson.ParseBytes(t))] = true
	}
	tools := v.Tools
	msgs := make([]json.RawMessage, 0, len(v.Messages))
	for _, raw := range v.Messages {
		m := gjson.ParseBytes(raw)
		more := m.Get("tools")
		if !more.Exists() {
			msgs = append(msgs, raw)
			continue
		}
		for _, t := range more.Array() {
			if !t.IsObject() {
				continue
			}
			if k := chatToolKey(t); !have[k] {
				have[k] = true
				tools = append(tools, json.RawMessage(t.Raw))
			}
		}
		role := m.Get("role").String()
		if (role == "system" || role == "developer") && emptyContent(m.Get("content")) {
			continue
		}
		var mm map[string]json.RawMessage
		if json.Unmarshal(raw, &mm) != nil {
			msgs = append(msgs, raw)
			continue
		}
		delete(mm, "tools")
		out, err := marshalPlain(mm)
		if err != nil {
			return body
		}
		msgs = append(msgs, out)
	}
	fields := map[string]any{"messages": msgs}
	if len(tools) > 0 {
		fields["tools"] = tools
	}
	return withFields(body, fields)
}

// chatToolKey is a Chat tool's name, a hosted tool's without one by its
// type.
func chatToolKey(t gjson.Result) string {
	if n := t.Get("function.name").String(); n != "" {
		return n
	}
	if n := t.Get("name").String(); n != "" {
		return n
	}
	return "#" + t.Get("type").String()
}
