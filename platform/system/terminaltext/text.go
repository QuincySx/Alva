// INPUT: Untrusted UTF-8 display text from an explicitly selected CLI source
// OUTPUT: Escape visible C0/C1 control sequences while preserving ordinary Unicode/LF/TAB
// POS: [platform / OS primitive] Display transformation only; no terminal actions, history mutation or secret-policy claim.
package terminaltext

import (
	"fmt"
	"strings"
)

func Escape(value string) string {
	var out strings.Builder
	for _, r := range value {
		if r != '\n' && r != '\t' && (r < 32 || r >= 127 && r < 160) {
			fmt.Fprintf(&out, "\\u%04x", r)
		} else {
			out.WriteRune(r)
		}
	}
	return out.String()
}
