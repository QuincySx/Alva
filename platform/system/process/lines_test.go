// INPUT: Deliberately split credential/Unicode bytes, EOF and explicit framing budgets
// OUTPUT: Complete-line, exact delimiters, callback error and bounded overflow regressions
// POS: [platform / OS primitive] Validates stream framing independently of product redaction or process authorization
package process

import (
	"bytes"
	"errors"
	"io"
	"strings"
	"testing"
)

type fragmentedReader struct{ parts [][]byte }

func (r *fragmentedReader) Read(p []byte) (int, error) {
	if len(r.parts) == 0 {
		return 0, io.EOF
	}
	part := r.parts[0]
	n := copy(p, part)
	if n == len(part) {
		r.parts = r.parts[1:]
	} else {
		r.parts[0] = part[n:]
	}
	return n, nil
}
func TestStreamLinesPreserveAcrossArbitraryReadAndUnicodeBoundaries(t *testing.T) {
	source := []byte("api_key=known-secret\n日志完整\npartial-tail")
	var parts [][]byte
	for _, b := range source {
		parts = append(parts, []byte{b})
	}
	var lines []StreamLine
	if err := ReadLines(&fragmentedReader{parts}, 128, func(line StreamLine) error { lines = append(lines, line); return nil }); err != nil {
		t.Fatal(err)
	}
	if len(lines) != 3 || !lines[0].Complete || !lines[1].Complete || lines[2].Complete || string(lines[0].Data) != "api_key=known-secret\n" || string(lines[1].Data) != "日志完整\n" {
		t.Fatal("read chunks were mistaken for complete records", lines)
	}
	var all []byte
	for _, line := range lines {
		all = append(all, line.Data...)
	}
	if !bytes.Equal(source, all) {
		t.Fatal("original bytes lost")
	}
}
func TestStreamLineOverflowAndObserverFailureAreExplicit(t *testing.T) {
	var frames []StreamLine
	err := ReadLines(strings.NewReader(strings.Repeat("x", 1024)), 16, func(line StreamLine) error { frames = append(frames, line); return nil })
	if !errors.Is(err, ErrLineLimit) || len(frames) != 1 || len(frames[0].Data) != 16 || !frames[0].Truncated || frames[0].Complete {
		t.Fatal("overflow silently lost", err)
	}
	want := errors.New("observer failed")
	if err = ReadLines(strings.NewReader("one\ntwo\n"), 128, func(StreamLine) error { return want }); !errors.Is(err, want) {
		t.Fatal("observer failure hidden", err)
	}
}

func TestStreamLineExplicitMediaBudgetHasAHardMaximum(t *testing.T) {
	// Large native media records need an explicit budget; ordinary callers keep
	// their existing smaller budgets. The reader does not allocate the limit.
	if err := ReadLines(strings.NewReader("bounded\n"), 64<<20, func(line StreamLine) error {
		if string(line.Data) != "bounded\n" || !line.Complete {
			t.Fatal("large explicit budget changed framing", line)
		}
		return nil
	}); err != nil {
		t.Fatal("explicit media budget was rejected", err)
	}
	called := false
	if err := ReadLines(strings.NewReader("must not read\n"), (64<<20)+1, func(StreamLine) error { called = true; return nil }); err == nil || called {
		t.Fatal("reader accepted a budget beyond the hard maximum", called, err)
	}
}
