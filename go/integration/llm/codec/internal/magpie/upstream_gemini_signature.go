// INPUT: encoding/json.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	json "encoding/json"
)

// googleSignature is the thought signature in a tool call's extra_content.
func googleSignature(extra json.RawMessage) string {
	if len(extra) == 0 {
		return ""
	}
	var v struct {
		Google struct {
			ThoughtSignature string `json:"thought_signature"`
		} `json:"google"`
	}
	json.Unmarshal(extra, &v)
	return v.Google.ThoughtSignature
}

// googleExtra is extra_content carrying a thought signature.
func googleExtra(sig string) map[string]any {
	return map[string]any{"google": map[string]any{"thought_signature": sig}}
}
