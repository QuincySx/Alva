// INPUT: Standalone notebook argument bytes and strict native cell-edit decoder.
// OUTPUT: Duplicate and incorrectly cased edit fields reject before any file transformation.
// POS: [L4 extension] Notebook mutation has one exact model-selected cell and source.
package notebook

import (
	"strings"
	"testing"
)

func TestNotebookRejectsAmbiguousEditArguments(t *testing.T) {
	for _, raw := range []string{`{"notebook_path":"a.ipynb","cell_id":"one","cell_id":"two","edit_mode":"delete"}`, `{"notebook_path":"a.ipynb","cell_id":"one","edit_mode":"replace","NEW_SOURCE":"changed"}`} {
		if _, err := decode([]byte(raw)); err == nil {
			t.Fatal("ambiguous edit accepted")
		}
	}
}
func TestNotebookRejectsDuplicateProtectionFields(t *testing.T) {
	raw := strings.Replace(original, `"metadata":{"execution"`, `"metadata":{"editable":false,"editable":true,"execution"`, 1)
	if _, err := transform([]byte(raw), editArguments{Path: "a.ipynb", ID: "code1", Mode: "replace", Source: sourcePointer("changed")}); err == nil {
		t.Fatal("duplicate metadata bypassed cell protection")
	}
}
func sourcePointer(value string) *string { return &value }
