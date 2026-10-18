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

// shapeChatTools is a Chat request with each such tool in the Chat shape.
// A tool that already has its function object, a hosted tool (any other
// type), one with no name, and a request that doesn't parse are as they
// came; a tool's other fields stay beside its function.
func shapeChatTools(body []byte) []byte {
	if !bytes.Contains(body, []byte(`"tools"`)) {
		return body
	}
	tools := gjson.GetBytes(body, "tools")
	if !tools.IsArray() {
		return body
	}
	need := false
	for _, t := range tools.Array() {
		if unshapedChatTool(t) {
			need = true
			break
		}
	}
	if !need {
		return body
	}
	var raw []json.RawMessage
	if json.Unmarshal([]byte(tools.Raw), &raw) != nil {
		return body
	}
	for i, t := range raw {
		if !unshapedChatTool(gjson.ParseBytes(t)) {
			continue
		}
		var m map[string]json.RawMessage
		if json.Unmarshal(t, &m) != nil {
			continue
		}
		fn := map[string]json.RawMessage{}
		for _, k := range []string{"name", "description", "strict"} {
			if v, ok := m[k]; ok {
				fn[k] = v
				delete(m, k)
			}
		}
		for _, k := range []string{"parameters", "inputSchema", "input_schema"} {
			if v, ok := m[k]; ok {
				if _, set := fn["parameters"]; !set {
					fn["parameters"] = v
				}
				delete(m, k)
			}
		}
		f, err := marshalPlain(fn)
		if err != nil {
			continue
		}
		m["type"] = json.RawMessage(`"function"`)
		m["function"] = f
		out, err := marshalPlain(m)
		if err != nil {
			continue
		}
		raw[i] = out
	}
	return withFields(body, map[string]any{"tools": raw})
}

// unshapedChatTool is whether t is a function tool with its name at the
// top rather than in a function object.
func unshapedChatTool(t gjson.Result) bool {
	if !t.IsObject() || t.Get("function").Exists() {
		return false
	}
	if typ := t.Get("type").String(); typ != "" && typ != "function" {
		return false
	}
	n := t.Get("name")
	return n.Type == gjson.String && n.String() != ""
}
