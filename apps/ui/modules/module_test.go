// INPUT: testing and bounded JSON message module fixtures
// OUTPUT: pure-data version/size/schema and safe native renderer data regressions
// POS: [L6 application] Message modules cannot smuggle executable renderer configuration
package modules

import (
	"strings"
	"testing"
)

func TestModuleEnvelopeAndBudgets(t *testing.T) {
	good := `{"kind":"table","version":1,"data":{"columns":["A"],"rows":[["one"]]}}`
	m, err := Decode(good)
	if err != nil || m.Kind != "table" {
		t.Fatal(m, err)
	}
	for _, bad := range []string{strings.Repeat("x", 16385), `{"kind":"table","version":0,"data":{}}`, good + `{}`, `{"kind":"table","version":1,"data":{},"script":"alert(1)"}`} {
		if _, err := Decode(bad); err == nil {
			t.Fatal("invalid module accepted")
		}
	}
}
func TestDataShapeFailsInsteadOfInventingChart(t *testing.T) {
	if (Table{Columns: []string{"a"}, Rows: [][]string{{"one", "two"}}}).Validate() == nil {
		t.Fatal("malformed table accepted")
	}
	if (Chart{Labels: []string{"a"}, Values: []float64{-1}}).Validate() == nil {
		t.Fatal("invalid chart accepted")
	}
	if (Chart{Labels: []string{"a"}, Values: []float64{7}}).Validate() != nil {
		t.Fatal("valid chart rejected")
	}
}
