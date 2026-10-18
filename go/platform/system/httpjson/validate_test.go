// INPUT: Concrete DTO and arbitrary protocol JSON byte fixtures
// OUTPUT: Duplicate/key-case/Unicode/depth/null denial and unknown-protocol-field preservation regressions
// POS: [platform / OS primitive] Shared input ambiguity acceptance without HTTP, credentials, model calls or product state
package httpjson

import (
	"strings"
	"testing"
)

func TestSharedValidation(t *testing.T) {
	type dto struct {
		Token string `json:"token"`
	}
	for _, data := range []string{`null`, `[]`, `{"Token":"x"}`, `{"token":"a","token":"b"}`, `{"token":"\ud800"}`, `{"unknown":"x"}`} {
		if err := ValidateObject([]byte(data), &dto{}); err == nil {
			t.Fatal("ambiguous DTO accepted", data)
		}
	}
	if err := ValidateObject([]byte(`{"token":"valid"}`), &dto{}); err != nil {
		t.Fatal(err)
	}
	if err := Validate([]byte(`{"unknown_future_field":{"value":1}}`)); err != nil {
		t.Fatal("generic protocol discarded future field", err)
	}
	for _, data := range []string{`null`, `[]`, `{"x":{"a":1,"a":2}}`, `{"x":` + strings.Repeat("[", 70) + `0` + strings.Repeat("]", 70) + `}`} {
		if err := Validate([]byte(data)); err == nil {
			t.Fatal("invalid/ambiguous document", data)
		}
	}
}
