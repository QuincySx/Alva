// INPUT: unicode/utf8
// OUTPUT: InlineResultLimit and InlinePrefix for delivering small tool output straight to the model
// POS: [L3 capability library] One shared bound for "small enough to show the model now"; larger bodies stay reference-only.
package agentcontext

import "unicode/utf8"

// InlineResultLimit is the largest body (bytes) a tool hands to the model
// directly. Larger bodies are delivered as a prefix plus the reference.
const InlineResultLimit = 8 << 10

// InlinePrefix returns text when it fits InlineResultLimit; otherwise the
// longest prefix that ends on a UTF-8 boundary and truncated=true.
func InlinePrefix(text string) (prefix string, truncated bool) {
	if len(text) <= InlineResultLimit {
		return text, false
	}
	cut := InlineResultLimit
	for cut > 0 && !utf8.RuneStart(text[cut]) {
		cut--
	}
	return text[:cut], true
}
