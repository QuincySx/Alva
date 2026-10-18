// INPUT: Untrusted ANSI/OSC/C1 text plus ordinary Unicode and escaped business text
// OUTPUT: Visible terminal-control protection without rewriting normal content or source values
// POS: [platform / OS primitive] Pure display regressions exercise both independent CLI consumers' shared boundary.
package terminaltext

import (
	"strings"
	"testing"
)

func TestEscapePreventsControlSequencesWithoutLosingOrdinaryText(t *testing.T) {
	input := "中文🙂\n\tcode \\u001b \x1b[2J\x1b]52;c;clipboard\a\r\u009b31m\u009d52;hidden\u009c\x7f"
	want := "中文🙂\n\tcode \\u001b \\u001b[2J\\u001b]52;c;clipboard\\u0007\\u000d\\u009b31m\\u009d52;hidden\\u009c\\u007f"
	if actual := Escape(input); actual != want {
		t.Fatal(actual)
	}
	if !strings.Contains(input, "\x1b") || Escape(want) != want {
		t.Fatal("source mutated or escaped display is not stable")
	}
}
