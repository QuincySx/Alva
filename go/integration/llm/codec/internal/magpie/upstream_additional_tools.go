// INPUT: bytes, encoding/json.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	bytes "bytes"
	json "encoding/json"
)

// liteNamespace is the namespace Responses Lite groups Codex's own
// functions in (codex_protocol::DEFAULT_FUNCTION_NAMESPACE).
const liteNamespace = "functions"

// liftAdditionalTools is a Responses request with its additional_tools items
// moved into its tools, or the request as it was when it has none.
func liftAdditionalTools(body []byte) []byte {
	if !bytes.Contains(body, []byte(`"additional_tools"`)) {
		return body
	}
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.UseNumber()
	var m map[string]any
	if dec.Decode(&m) != nil || m == nil {
		return body
	}
	items, ok := m["input"].([]any)
	if !ok {
		return body
	}
	tools, _ := m["tools"].([]any)
	have := toolsByName(tools)
	kept := items[:0:0]
	lifted := false
	for _, it := range items {
		im, _ := it.(map[string]any)
		if im["type"] != "additional_tools" {
			kept = append(kept, it)
			continue
		}
		lifted = true
		more, _ := im["tools"].([]any)
		for _, t := range more {
			tm, _ := t.(map[string]any)
			if tm == nil {
				continue
			}
			if tm["type"] == "namespace" && tm["name"] == liteNamespace {
				nested, _ := tm["tools"].([]any)
				for _, n := range nested {
					if nm, _ := n.(map[string]any); nm != nil {
						tools = addTool(tools, have, nm)
					}
				}
				continue
			}
			tools = addTool(tools, have, tm)
		}
	}
	if !lifted {
		return body
	}

	for _, it := range kept {
		if im, _ := it.(map[string]any); im != nil && im["namespace"] == liteNamespace {
			delete(im, "namespace")
		}
	}
	m["input"] = kept
	if len(tools) > 0 {
		m["tools"] = tools
	}
	out, err := marshalPlain(m)
	if err != nil {
		return body
	}
	return out
}

// toolsByName is a request's tools by their names, a hosted tool without
// one (web_search) by its type.
func toolsByName(tools []any) map[string]map[string]any {
	have := map[string]map[string]any{}
	for _, t := range tools {
		if tm, _ := t.(map[string]any); tm != nil {
			have[toolKey(tm)] = tm
		}
	}
	return have
}

func toolKey(tm map[string]any) string {
	if n, _ := tm["name"].(string); n != "" {
		return n
	}
	ty, _ := tm["type"].(string)
	return "#" + ty
}

// addTool is tools with tm among them: a tool added once, and a namespace
// given again adding to the one there the tools it didn't have.
func addTool(tools []any, have map[string]map[string]any, tm map[string]any) []any {
	k := toolKey(tm)
	old := have[k]
	if old == nil {
		have[k] = tm
		return append(tools, tm)
	}
	if old["type"] != "namespace" || tm["type"] != "namespace" {
		return tools
	}
	nested, _ := old["tools"].([]any)
	more, _ := tm["tools"].([]any)
	for _, nt := range more {
		nm, _ := nt.(map[string]any)
		if nm != nil && !containsNamed(nested, nm["name"]) {
			nested = append(nested, nm)
		}
	}
	old["tools"] = nested
	return tools
}

func containsNamed(tools []any, name any) bool {
	for _, o := range tools {
		if om, _ := o.(map[string]any); om["name"] == name {
			return true
		}
	}
	return false
}
