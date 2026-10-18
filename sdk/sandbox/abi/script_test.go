// INPUT: Valid and invalid script UTF-8, source/output sizes and result unions
// OUTPUT: Script ABI bounds and recoverable-error shape regressions
// POS: [L0 foundation] Pure protocol checks without a concrete script engine or host authority
package abi

import (
	"strings"
	"testing"
)

func TestScriptBoundsAndResultShape(t *testing.T) {
	for _, request := range []ScriptRequest{{Version: Version, Script: ""}, {Version: Version + 1, Script: "1"}, {Version: Version, Script: strings.Repeat("x", MaxScriptBytes+1)}, {Version: Version, Script: string([]byte{255})}} {
		if request.Validate() == nil {
			t.Fatal("invalid script request accepted")
		}
	}
	if err := (ScriptRequest{Version: Version, Script: "print('你好'); 42"}).Validate(); err != nil {
		t.Fatal(err)
	}
	for _, result := range []ScriptResult{{Version: Version, Result: "ok", Error: "bad"}, {Version: Version, Output: make([]string, 257)}, {Version: Version, Output: []string{strings.Repeat("x", 64<<10+1)}}, {Version: Version, Error: strings.Repeat("x", 32<<10+1)}} {
		if result.Validate() == nil {
			t.Fatal("invalid script result accepted")
		}
	}
	if err := (ScriptResult{Version: Version, Output: []string{"partial output"}, Error: "recoverable script failure; file effects may be partial"}).Validate(); err != nil {
		t.Fatal(err)
	}
}
