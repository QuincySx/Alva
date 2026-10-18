// INPUT: strings.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	strings "strings"
)

// vertexThinking is the thinkingConfig a request asks Vertex AI for, which
// turns away what a model can't do. Gemini 3 is sent its level, fitted to
// the model's own by vertexEffort, with no thoughts given back when the
// client turned reasoning off: the level is then the model's lowest, as
// Gemini 3 has no way to stop thinking altogether. 2.5 takes no level
// ("thinking_level is not supported by this model"): it is sent a budget
// in its own range. Turned off, Flash and Flash-Lite are sent 0, without
// thoughts asked for ("include_thoughts is only enabled when thinking is
// enabled"), and Pro, which can't stop ("does not support setting
// thinking_budget to 0"), its least. A Gemini before 2.5 doesn't think.
func vertexThinking(r *Request, model string) map[string]any {
	m := strings.ToLower(model)
	if strings.HasPrefix(m, "gemini-1") || strings.HasPrefix(m, "gemini-2.0") {
		return nil
	}
	if r.Effort == "" && !r.Thinking && !r.ThinkOff {
		return nil
	}
	if off, least, most, ok := vertexBudgets(m); ok {
		if r.ThinkOff {
			return map[string]any{"thinkingBudget": off}
		}
		tc := map[string]any{"includeThoughts": true}
		if r.Effort != "" {
			tc["thinkingBudget"] = min(max(budgetOf(r.Effort), least), most)
		}
		return tc
	}
	tc := map[string]any{"includeThoughts": !r.ThinkOff}
	if r.Effort != "" {
		tc["thinkingLevel"] = r.Effort
	}
	return tc
}

// vertexBudgets are the thinking budgets a Gemini 2.5 takes, as Google's
// model reference gives them: the one that turns its thinking off, or its
// least where it can't be, and its range. Not 2.5: false.
func vertexBudgets(model string) (off, least, most int, ok bool) {
	switch {
	case strings.HasPrefix(model, "gemini-2.5-pro"):
		return 128, 128, 32768, true
	case strings.HasPrefix(model, "gemini-2.5-flash-lite"):
		return 0, 512, 24576, true
	case strings.HasPrefix(model, "gemini-2.5-flash"):
		return 0, 1, 24576, true
	}
	return 0, 0, 0, false
}
