// INPUT: Generated inert PDF bytes, real immutable catalog and the actual explicitly injected OS document owner.
// OUTPUT: Captured PDF originals, authenticated private derivatives and bounded text-only prompt preparation.
// POS: [L7 Hub product acceptance] Uses system PDFKit only on disposable fixtures; no model, user file or account.
//go:build darwin

package product

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/system/document"
)

func TestComposerFileNativePDFDerivativeRetainsOriginalAndActualProcessing(t *testing.T) {
	s, _ := fixture(t)
	s.Documents = document.NewProcessor()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := s.Documents.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	content := "BT /F1 12 Tf 72 720 Td (Actual native PDF text) Tj ET\n"
	objects := []string{"<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>", "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>", fmt.Sprintf("<< /Length %d >>\nstream\n%sendstream", len(content), content), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"}
	var pdf bytes.Buffer
	pdf.WriteString("%PDF-1.4\n")
	offsets := []int{0}
	for i, body := range objects {
		offsets = append(offsets, pdf.Len())
		fmt.Fprintf(&pdf, "%d 0 obj\n%s\nendobj\n", i+1, body)
	}
	xref := pdf.Len()
	fmt.Fprintf(&pdf, "xref\n0 %d\n0000000000 65535 f \n", len(offsets))
	for _, offset := range offsets[1:] {
		fmt.Fprintf(&pdf, "%010d 00000 n \n", offset)
	}
	fmt.Fprintf(&pdf, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets), xref)
	path := filepath.Join(t.TempDir(), "native.pdf")
	if err := os.WriteFile(path, pdf.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	f, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "native-pdf"}, path)
	if err != nil || f.Text || f.TextKey == "" || !strings.Contains(f.Processing, "PDF text extracted from 1 pages") || !strings.Contains(f.Processing, "not a kernel hard memory cap") {
		t.Fatal("PDF was not actually processed with honest limits", f, err)
	}
	selection := SelectedComposerFile(f)
	original, err := s.ReadComposerFile(t.Context(), selection, false, 0, 4096)
	if err != nil || !bytes.Equal(original.Data, pdf.Bytes()) {
		t.Fatal("PDF processing replaced original bytes", err)
	}
	derived, err := s.ReadComposerFile(t.Context(), selection, true, 0, 4096)
	if err != nil || !strings.Contains(string(derived.Data), "Actual native PDF text") {
		t.Fatal("native PDF text was not retained", string(derived.Data), err)
	}
	manifests, prompt, err := s.PrepareComposerFileContext(t.Context(), []ComposerFileSelection{selection}, 4096)
	if err != nil || !strings.Contains(manifests[0].InlineText, "Actual native PDF text") || strings.Contains(prompt, "%PDF-1.4") {
		t.Fatal("PDF prompt preparation used raw PDF or lost actual text", err)
	}
}
