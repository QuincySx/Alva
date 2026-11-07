// INPUT: Generated inert PDF fixtures, actual OS PDFKit/JXA and explicit read-only descriptors.
// OUTPUT: Real text/page extraction, limits, watchdog termination and canceled physical reap regressions.
// POS: [platform / OS primitive acceptance] No user documents, windows, permissions, accounts or model calls.
//go:build darwin

package document

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func pdfFixture(t *testing.T, pages int) *os.File {
	t.Helper()
	var out bytes.Buffer
	out.WriteString("%PDF-1.4\n")
	content := "BT /F1 12 Tf 72 720 Td (PDFKit fixture text) Tj ET\n"
	objects := []string{"<< /Type /Catalog /Pages 2 0 R >>", "", fmt.Sprintf("<< /Length %d >>\nstream\n%sendstream", len(content), content), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"}
	var kids strings.Builder
	for i := 0; i < pages; i++ {
		fmt.Fprintf(&kids, "%d 0 R ", i+5)
		objects = append(objects, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 3 0 R /Resources << /Font << /F1 4 0 R >> >> >>")
	}
	objects[1] = fmt.Sprintf("<< /Type /Pages /Kids [%s] /Count %d >>", kids.String(), pages)
	offsets := []int{0}
	for index, body := range objects {
		offsets = append(offsets, out.Len())
		fmt.Fprintf(&out, "%d 0 obj\n%s\nendobj\n", index+1, body)
	}
	xref := out.Len()
	fmt.Fprintf(&out, "xref\n0 %d\n0000000000 65535 f \n", len(offsets))
	for _, offset := range offsets[1:] {
		fmt.Fprintf(&out, "%010d 00000 n \n", offset)
	}
	fmt.Fprintf(&out, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets), xref)
	path := filepath.Join(t.TempDir(), "fixture.pdf")
	if err := os.WriteFile(path, out.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { file.Close() })
	return file
}
func closePDF(t *testing.T, p *Processor) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := p.Close(ctx); err != nil || !p.CleanupComplete() {
		t.Fatal("PDF process owner did not physically close", err)
	}
}
func TestPDFKitActualTextAndPageLimit(t *testing.T) {
	p := NewProcessor()
	defer closePDF(t, p)
	out, err := p.ExtractPDF(t.Context(), pdfFixture(t, 2))
	if err != nil || out.Pages != 2 || strings.Count(out.Text, "PDFKit fixture text") != 2 || out.Processor == "" {
		t.Fatal("OS PDFKit did not extract actual fixture text", out, err)
	}
	if _, err = p.ExtractPDF(t.Context(), pdfFixture(t, MaxPages+1)); !errors.Is(err, ErrPageLimit) {
		t.Fatal("page limit did not reject before text extraction", err)
	}
}
func TestPDFKitMemoryWatchdogKillsAndReaps(t *testing.T) {
	p := NewProcessor()
	p.memoryLimit = 1
	defer closePDF(t, p)
	_, err := p.ExtractPDF(t.Context(), pdfFixture(t, 1))
	if !errors.Is(err, ErrMemoryLimit) {
		t.Fatal("owned-memory watchdog did not stop native parser", err)
	}
	p.mu.Lock()
	remaining := len(p.children)
	p.mu.Unlock()
	if remaining != 0 {
		t.Fatal("memory-limited decoder retained a physically running child")
	}
}
func TestPDFKitCancellationJoinsActualDecoder(t *testing.T) {
	p := NewProcessor()
	defer closePDF(t, p)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	result := make(chan error, 1)
	file := pdfFixture(t, 1)
	go func() { _, err := p.ExtractPDF(ctx, file); result <- err }()
	deadline := time.Now().Add(3 * time.Second)
	started := false
	for time.Now().Before(deadline) {
		p.mu.Lock()
		started = len(p.children) > 0
		p.mu.Unlock()
		if started {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if !started {
		t.Fatal("native decoder never became an actual owned child")
	}
	cancel()
	select {
	case err := <-result:
		if !errors.Is(err, context.Canceled) {
			t.Fatal("native cancellation lost the actual request", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("native decoder cancellation did not join")
	}
	p.mu.Lock()
	remaining := len(p.children)
	p.mu.Unlock()
	if remaining != 0 {
		t.Fatal("canceled native decoder still had physical ownership")
	}
}
