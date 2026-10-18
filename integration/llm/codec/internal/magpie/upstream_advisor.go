// INPUT: strings.
// OUTPUT: Private protocol helpers; no exported declarations.
// POS: Internal pinned upstream conversion slice; Alva owns admission and transport.
// Derived from yetone/magpie e085d6e9262786815c0f6c5ff65c5a585916cc9e, MIT, copyright 2026 yetone.
package magpie

import (
	strings "strings"
)

// advisorNote opens the instructions Claude Code puts in the conversation
// when its advisor is on ("# Advisor Tool\n\nYou have access to an
// `advisor` tool … Call advisor BEFORE substantive work"), at the end of a
// {role: "system"} message's text (2.1.300), and advisorGone is what Claude
// Code itself says there once the advisor is turned off.
const (
	advisorNote = "# Advisor Tool\n"
	advisorGone = "The advisor tool is no longer available; disregard the earlier advisor instructions."
)

// advisorGoneIn is text with the advisor's instructions, up to the end of
// the reminder they are in or of the text, said to be void as Claude Code
// says it, and whether it held them.
func advisorGoneIn(text string) (string, bool) {
	i := strings.Index(text, advisorNote)
	if i < 0 {
		return text, false
	}
	end := len(text)
	if j := strings.Index(text[i:], "</system-reminder>"); j >= 0 {
		end = i + j
	}
	note := text[i:end]
	trail := note[len(strings.TrimRight(note, " \t\r\n")):]
	return text[:i] + advisorGone + trail + text[end:], true
}
