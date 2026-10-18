// INPUT: testing
// OUTPUT: Typed capability registration regressions
// POS: [L0 foundation] Ensures bus boundaries do not degrade into last-writer-wins global state.
package bus

import (
	"testing"
)

type service interface{ Value() string }
type implementation struct{}

func (*implementation) Value() string { return "service" }

func TestTypedCapabilitiesRejectDuplicatesAndSeal(t *testing.T) {
	writer, handle := New()
	if err := Provide[service](writer, &implementation{}); err != nil {
		t.Fatal(err)
	}
	if value, ok := Resolve[service](handle); !ok || value.Value() != "service" {
		t.Fatal("capability missing")
	}
	if _, ok := Resolve[*implementation](handle); ok {
		t.Fatal("concrete type leaked into interface registry")
	}
	if err := Provide[service](writer, &implementation{}); err == nil {
		t.Fatal("duplicate accepted")
	}
	writer.Seal()
	if err := Provide[string](writer, "late"); err == nil {
		t.Fatal("sealed write accepted")
	}
}
func TestNilCapabilityRejected(t *testing.T) {
	writer, _ := New()
	var value *implementation
	if err := Provide[service](writer, value); err == nil {
		t.Fatal("typed nil accepted")
	}
}
