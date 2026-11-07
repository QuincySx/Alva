// INPUT: Cold document owners and disposable over-budget file descriptors.
// OUTPUT: No-start construction, truthful input-budget rejection and joined cold Close.
// POS: [platform / OS primitive acceptance] Uses fixtures only and never acquires a decoder dependency or account.
package document

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestProcessorColdCloseAndInputBounds(t *testing.T) {
	p := NewProcessor()
	if p.CleanupComplete() {
		t.Fatal("open owner falsely marked closed")
	}
	file, err := os.Create(filepath.Join(t.TempDir(), "too-large.pdf"))
	if err != nil {
		t.Fatal(err)
	}
	if err = file.Truncate(MaxInputBytes + 1); err != nil {
		t.Fatal(err)
	}
	file.Close()
	source, err := os.Open(file.Name())
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	if _, err = p.ExtractPDF(t.Context(), source); err == nil {
		t.Fatal("over-budget PDF entered native parsing")
	}
	if len(p.children) != 0 {
		t.Fatal("input validation started a child")
	}
	if err = p.Close(context.Background()); err != nil || !p.CleanupComplete() {
		t.Fatal("cold owner did not close", err)
	}
}
