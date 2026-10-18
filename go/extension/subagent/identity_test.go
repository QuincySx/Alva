// INPUT: testing, kernel/abi
// OUTPUT: Grant identity comparisons reject dynamically noncomparable values
// POS: [L4 extension] Cannot prove direct-parent tool identity through map/slice configuration.
package subagent

import (
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"testing"
)

type identityGrantTool struct {
	abi.Tool
	configuration any
}

func TestActualGrantIdentityUsesDynamicValues(t *testing.T) {
	for _, values := range [][2]any{{map[string]string{}, "comparable"}, {"comparable", []string{}}, {map[string]string{}, map[string]string{}}} {
		func() {
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("grant comparison panicked: %v", value)
				}
			}()
			if sameActualTool(identityGrantTool{configuration: values[0]}, identityGrantTool{configuration: values[1]}) {
				t.Error("noncomparable value proved grant identity")
			}
		}()
	}
	if !sameActualTool(identityGrantTool{configuration: "same"}, identityGrantTool{configuration: "same"}) {
		t.Fatal("comparable value identity lost")
	}
}
