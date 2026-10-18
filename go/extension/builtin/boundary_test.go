// INPUT: Real temporary workspace and standalone native file Tools without harness schema middleware.
// OUTPUT: Null/duplicate/invalid-Unicode writes reject before changing existing file bytes.
// POS: [L4 extension] File Tool owned validation preserves exact user content independently of the harness recipe.
package builtin

import (
	"os"
	"path/filepath"
	"testing"
)

func TestFileToolRejectsAmbiguousWriteWithoutDataLoss(t *testing.T) {
	w, _, _, tools := fileFixture(t)
	path := filepath.Join(w.cwd, "keep.txt")
	for _, raw := range []string{`{"path":"keep.txt","content":null}`, `{"path":"keep.txt","content":"one","content":"two"}`, `{"path":"keep.txt","content":"\ud800"}`} {
		if err := os.WriteFile(path, []byte("original bytes"), 0600); err != nil {
			t.Fatal(err)
		}
		result, err := executeFile(t, w, tools, "write_file", raw)
		if err == nil && !result.IsError {
			t.Error("ambiguous write accepted")
		}
		data, readErr := os.ReadFile(path)
		if readErr != nil || string(data) != "original bytes" {
			t.Fatal("invalid write changed existing content", string(data), readErr)
		}
	}
}
