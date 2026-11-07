// INPUT: A pinned read-only PDF descriptor, fixed Apple PDFKit/JXA program and owned-process memory observations.
// OUTPUT: At most 512 pages/2 MiB UTF-8 text under timeout, output and observed-memory watchdogs.
// POS: [platform / OS primitive] Isolates PDFKit native allocations and joins actual child/readers before releasing ownership.
//go:build darwin

package document

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/process"
)

// Apple documents per-page text, page-count and lock state separately:
// https://developer.apple.com/documentation/pdfkit/pdfpage/string
// https://developer.apple.com/documentation/pdfkit/pdfdocument/pagecount
// The fixed program uses JavaScriptObjC only, with no Application/AppleEvents,
// eval, document actions, user libraries or document-provided program source.
// No source path is interpolated: fd 3 is the explicitly granted read-only file.
const pdfProgram = `ObjC.import('/System/Library/Frameworks/Foundation.framework');
ObjC.import('/System/Library/Frameworks/PDFKit.framework');
function run(argv) {
  const doc = $.PDFDocument.alloc.initWithURL($.NSURL.fileURLWithPath('/dev/fd/3'));
  if (!doc || doc.isNil() || doc.isLocked) throw Error('ALVA_PDF_INVALID');
  const pages = Number(doc.pageCount);
  if (pages < 1) throw Error('ALVA_PDF_INVALID');
  if (pages > 512) throw Error('ALVA_PDF_PAGES');
  let bytes = 0;
  let texts = [];
  for (let i = 0; i < pages; i++) {
    const page = doc.pageAtIndex(i);
    const text = page.string;
    if (text && !text.isNil()) {
      const size = Number(text.lengthOfBytesUsingEncoding($.NSUTF8StringEncoding));
      if (bytes + size + 1 > 2097152) throw Error('ALVA_PDF_TEXT');
      bytes += size + 1;
      texts.push(ObjC.unwrap(text));
    } else {
      bytes += 1;
      texts.push('');
    }
  }
  const body = texts.join('\n');
  if (body.trim().length === 0) throw Error('ALVA_PDF_INVALID');
  const stdout = $.NSFileHandle.fileHandleWithStandardOutput;
  stdout.writeData($(JSON.stringify({pages: pages}) + '\n').dataUsingEncoding($.NSUTF8StringEncoding));
  stdout.writeData($(body).dataUsingEncoding($.NSUTF8StringEncoding));
}`

type pdfCaptured struct {
	data   []byte
	err    error
	stdout bool
}

func (p *Processor) extract(ctx context.Context, source *os.File) (Output, error) {
	child, err := p.start(ctx, process.Config{Command: "/usr/bin/osascript", Args: []string{"-l", "JavaScript", "-e", pdfProgram}, Cwd: "/", ReadFiles: []*os.File{source}})
	if err != nil {
		return Output{}, err
	}
	_ = child.Stdin.Close()
	captured := make(chan pdfCaptured, 2)
	read := func(stream io.Reader, stdout bool, limit int) {
		data, err := io.ReadAll(io.LimitReader(stream, int64(limit)+1))
		if len(data) > limit {
			data = data[:limit]
			if stdout {
				err = errors.Join(err, ErrOutputLimit)
			} else {
				err = errors.Join(err, errors.New("PDF decoder diagnostics exceeded 16 KiB"))
			}
		}
		captured <- pdfCaptured{data, err, stdout}
	}
	go read(child.Stdout, true, MaxTextBytes+4096)
	go read(child.Stderr, false, 16<<10)
	ticker := time.NewTicker(memoryInterval)
	defer ticker.Stop()
	remaining := 2
	var stdout, stderr []byte
	var runErr error
	closing := false
	closeChild := func() {
		if !closing {
			closing = true
			child.BeginClose()
		}
	}
	waitResult := make(chan error, 1)
	waiting, waited := false, false
	ctxDone := ctx.Done()
	for remaining > 0 || !waited {
		if remaining == 0 && !waiting {
			waiting = true
			go func() { waitResult <- child.Wait(ctx) }()
		}
		select {
		case result := <-captured:
			remaining--
			if result.stdout {
				stdout = result.data
			} else {
				stderr = result.data
			}
			if result.err != nil && !(closing && errors.Is(result.err, os.ErrClosed)) {
				runErr = errors.Join(runErr, result.err)
				closeChild()
			}
		case err := <-waitResult:
			runErr = errors.Join(runErr, err)
			waited = true
		case <-ticker.C:
			memory, err := child.MemoryBytes()
			if err != nil && !errors.Is(err, os.ErrClosed) {
				runErr = errors.Join(runErr, errors.New("PDF decoder memory inspection unavailable; processing stopped"))
				closeChild()
			} else if memory > p.memoryLimit {
				runErr = errors.Join(runErr, ErrMemoryLimit)
				closeChild()
			}
		case <-ctxDone:
			runErr = errors.Join(runErr, ctx.Err())
			ctxDone = nil
			closeChild()
		}
	}
	cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	runErr = errors.Join(runErr, child.Close(cleanup))
	p.release(child)
	code, ended := child.ExitCode()
	if !ended || code != 0 || !child.CleanupComplete() {
		switch {
		case bytes.Contains(stderr, []byte("ALVA_PDF_PAGES")):
			runErr = errors.Join(runErr, ErrPageLimit)
		case bytes.Contains(stderr, []byte("ALVA_PDF_TEXT")):
			runErr = errors.Join(runErr, ErrOutputLimit)
		default:
			runErr = errors.Join(runErr, ErrInvalidPDF)
		}
	}
	if runErr != nil {
		return Output{}, runErr
	}
	newline := bytes.IndexByte(stdout, '\n')
	if newline < 0 || newline > 4096 {
		return Output{}, ErrInvalidPDF
	}
	var header struct {
		Pages int `json:"pages"`
	}
	if json.Unmarshal(stdout[:newline], &header) != nil || header.Pages < 1 || header.Pages > MaxPages {
		return Output{}, ErrInvalidPDF
	}
	body := stdout[newline+1:]
	if len(body) > MaxTextBytes {
		return Output{}, ErrOutputLimit
	}
	if !utf8.Valid(body) || strings.TrimSpace(string(body)) == "" {
		return Output{}, ErrInvalidPDF
	}
	return Output{Text: string(body), Pages: header.Pages, Processor: "macOS PDFKit via isolated fixed JavaScriptObjC worker"}, nil
}
