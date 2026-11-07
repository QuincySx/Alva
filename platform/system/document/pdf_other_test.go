// INPUT: An explicitly selected regular fixture on a platform without the native PDFKit worker.
// OUTPUT: Unsupported extraction without process creation or dependency acquisition.
// POS: [platform / OS primitive acceptance] Cross-builds preserve an honest unsupported path.
//go:build !darwin

package document

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestPDFUnsupportedPlatformDoesNotStartWorker(t *testing.T) {
	p := NewProcessor()
	defer p.Close(t.Context())
	path := filepath.Join(t.TempDir(), "fixture.pdf")
	if err := os.WriteFile(path, []byte("%PDF-1.4\nfixture"), 0600); err != nil {
		t.Fatal(err)
	}
	source, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	if _, err = p.ExtractPDF(t.Context(), source); !errors.Is(err, ErrUnsupported) {
		t.Fatal("unsupported platform guessed or acquired a decoder", err)
	}
	if len(p.children) != 0 {
		t.Fatal("unsupported platform started a child")
	}
}
