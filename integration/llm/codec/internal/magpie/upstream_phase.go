// INPUT: Request-local Magpie values.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

const (
	phaseCommentary = "commentary"
	phaseFinal      = "final_answer"
)

// heldMost is how many events may wait behind a message whose phase isn't
// known yet; past it they go on, and the message without a phase.
const heldMost = 512

// unphased reports whether an output item is an assistant message with no
// phase of its own.
func unphased(it map[string]any) bool {
	if it == nil || it["type"] != "message" {
		return false
	}
	if r, ok := it["role"]; ok && r != "assistant" {
		return false
	}
	p, _ := it["phase"].(string)
	return p == ""
}

// phaseOutput gives each assistant message in a reply's output its phase,
// and reports whether it gave any. done is whether the reply ended
// completed: only then is the last message the final answer.
func phaseOutput(output []any, done bool) bool {
	last := -1
	for i, it := range output {
		if m, _ := it.(map[string]any); m != nil && m["type"] != "reasoning" {
			last = i
		}
	}
	changed := false
	for i, it := range output {
		m, _ := it.(map[string]any)
		if !unphased(m) {
			continue
		}
		switch {
		case i < last:
			m["phase"] = phaseCommentary
		case i == last && done:
			m["phase"] = phaseFinal
		default:
			continue
		}
		changed = true
	}
	return changed
}
