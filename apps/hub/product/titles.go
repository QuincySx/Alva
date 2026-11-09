// INPUT: A user's first prompt, an explicit title, or a manual rename
// OUTPUT: SessionTitle (the one generated-title rule) and RenameTitle validation
// POS: [L7 Hub product] Desktop, HTTP and CLI derive titles here only; replacing the rule with a model-generated title means editing this file alone
package product

import (
	"errors"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
)

const (
	// DefaultSessionTitle names a session created with neither title nor prompt.
	DefaultSessionTitle = "新任务"
	// generatedTitleRunes is the first-line cap of a prompt-derived title.
	generatedTitleRunes = 32
	// maxRenameRunes bounds a manual title; the store caps it at 256 bytes.
	maxRenameRunes = 64
)

// SessionTitle is the single title rule. An explicit non-blank title wins
// (first line, trimmed, same rune cap as a rename); otherwise the title is the
// first non-empty line of the prompt capped at 32 runes with an ellipsis; with
// neither it is DefaultSessionTitle.
func SessionTitle(explicit, prompt string) string {
	if line := firstLine(explicit); line != "" {
		return truncateRunes(line, maxRenameRunes)
	}
	if line := firstLine(prompt); line != "" {
		if utf8.RuneCountInString(line) > generatedTitleRunes {
			return string([]rune(line)[:generatedTitleRunes]) + "…"
		}
		return line
	}
	return DefaultSessionTitle
}

// RenameTitle validates a user-typed title: one non-empty line after trimming,
// at most 64 runes. Longer or blank text is rejected, never silently cut.
func RenameTitle(text string) (string, error) {
	line := firstLine(text)
	if hasControlInFirstLine(text) {
		return "", errors.New("标题不能包含控制字符")
	}
	if line == "" {
		return "", errors.New("标题不能为空")
	}
	if utf8.RuneCountInString(line) > maxRenameRunes {
		return "", errors.New("标题最多 64 个字")
	}
	return line, nil
}

// firstLine is the first non-blank line, split on every line break (LF, CR,
// U+2028/2029); control and format runes (NUL, bidi controls, ...) are dropped
// so a generated or explicit title is always one clean visible line.
func firstLine(text string) string {
	for _, line := range strings.FieldsFunc(text, func(r rune) bool { return r == '\n' || r == '\r' || r == '\u2028' || r == '\u2029' }) {
		if !utf8.ValidString(line) {
			continue
		}
		clean := strings.Map(func(r rune) rune {
			if unicode.IsControl(r) || unicode.Is(unicode.Cf, r) {
				return -1
			}
			return r
		}, line)
		if clean = strings.TrimSpace(clean); clean != "" {
			return clean
		}
	}
	return ""
}

func truncateRunes(text string, limit int) string {
	if utf8.RuneCountInString(text) <= limit {
		return text
	}
	return string([]rune(text)[:limit])
}

// hasControlInFirstLine is true when the line a rename would keep still holds
// a control or format rune; a rename rejects it where a generated title strips.
func hasControlInFirstLine(text string) bool {
	for _, line := range strings.FieldsFunc(text, func(r rune) bool { return r == '\n' || r == '\r' || r == '\u2028' || r == '\u2029' }) {
		if strings.TrimSpace(line) != "" {
			return hub.TitleHasControl(line)
		}
	}
	return false
}
