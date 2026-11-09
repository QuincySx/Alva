// INPUT: Untrusted text from tools and runs: logs, diffs, error messages, titles.
// OUTPUT: clip: control and bidirectional-override characters removed, size bounded by lines and bytes, with the number of dropped lines reported.
// POS: [L7 Kit blocks] Every block that shows text it did not write goes through here, so a huge or hostile output can neither freeze a frame nor reorder what the user reads.
//go:build desktop

package blocks

import (
	"strings"
	"unicode/utf8"
)

// Bounds on tool output and one-line text. A frame lays out all of what it shows, so the cap is what keeps a
// 200,000-character log from stalling the window; the full text stays with the product.
const (
	outputMaxLines = 200
	outputMaxBytes = 16 << 10
	lineMaxBytes   = 256
)

// printable reports whether r may be shown: tab and newline stay; other C0 and C1 controls, bidirectional
// embedding, override and isolate controls, marks and the invalid rune do not.
func printable(r rune) bool {
	switch {
	case r == '\t' || r == '\n':
		return true
	case r < 0x20 || r == 0x7f || (r >= 0x80 && r <= 0x9f):
		return false
	case r == 0x061c || r == 0x200e || r == 0x200f || (r >= 0x202a && r <= 0x202e) || (r >= 0x2066 && r <= 0x2069):
		return false
	case r == utf8.RuneError || r == 0xfeff:
		return false
	}
	return true
}

// clip returns s cleaned and cut to at most maxLines lines and maxBytes bytes (on a rune boundary), and how many
// lines were dropped. A carriage return is dropped, so CRLF logs read as plain lines.
func clip(s string, maxLines, maxBytes int) (text string, droppedLines int) {
	var b strings.Builder
	lines := 1
	cut := -1
	for i, r := range s {
		if !printable(r) {
			continue
		}
		if r == '\n' {
			if lines == maxLines {
				cut = i
				break
			}
			lines++
		}
		if b.Len()+utf8.RuneLen(r) > maxBytes {
			cut = i
			break
		}
		b.WriteRune(r)
	}
	if cut >= 0 {
		// A trailing newline ends the last line, it does not start another.
		droppedLines = strings.Count(strings.TrimSuffix(s[cut:], "\n"), "\n")
		if s[cut] != '\n' {
			droppedLines++
		}
	}
	return b.String(), droppedLines
}

// oneLine cleans s, folds it to a single line and cuts it to maxBytes; it never ends mid-rune.
func oneLine(s string, maxBytes int) string {
	text, _ := clip(strings.ReplaceAll(s, "\n", " "), 1, maxBytes)
	return strings.ReplaceAll(text, "\t", " ")
}
