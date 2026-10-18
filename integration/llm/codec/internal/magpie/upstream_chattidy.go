// INPUT: encoding/json, strings.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	json "encoding/json"
	strings "strings"
)

// messageReasoning are the fields a Chat client sends an earlier
// assistant turn's thinking back in: reasoning_content (DeepSeek's, which
// OpenCode and most clients send), reasoning (OpenRouter's) and
// reasoning_details (OpenRouter's structured one). DeepSeek, Kimi and the
// like want it back; Mistral turns each away as extra_forbidden (#494).
var messageReasoning = []string{"reasoning_content", "reasoning", "reasoning_details"}

// reasoningText is the thinking a message carries in messageReasoning:
// reasoning_content, else reasoning, else reasoning_details' text.
func reasoningText(m map[string]json.RawMessage) string {
	for _, f := range messageReasoning[:2] {
		var s string
		if decodeJSON(m[f], &s) == nil && s != "" {
			return s
		}
	}
	var details []struct {
		Text    string `json:"text"`
		Summary string `json:"summary"`
	}
	decodeJSON(m["reasoning_details"], &details)
	var b strings.Builder
	for _, d := range details {
		if d.Text != "" {
			b.WriteString(d.Text)
		} else {
			b.WriteString(d.Summary)
		}
	}
	return b.String()
}

// noReasoning is the reasoning_content a tool call goes to an upstream
// that wants it back (chatReplaysReasoning) with, when the client has none
// for it: a turn another model made, or one whose thinking the client
// didn't keep. DeepSeek turns the whole request away when a tool call of
// the turn being continued has none ("The `reasoning_content` in the
// thinking mode must be passed back to the API", #1462). Its Chat API
// takes an empty one, but a relay that leaves empty fields out would send
// none, and its Responses API turns an empty reasoning_text away, so it is
// a space.
const noReasoning = " "
