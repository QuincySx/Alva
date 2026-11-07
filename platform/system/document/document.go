// INPUT: Explicit read-only PDF files, fixed native decoder configuration and caller cancellation.
// OUTPUT: Processor-owned bounded PDF text extraction and physical decoder cleanup.
// POS: [platform / OS primitive] Native document parsing is isolated from the desktop process and never executes document actions or user programs.
package document

import (
	"context"
	"errors"
	"os"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

const MaxInputBytes int64 = 32 << 20
const MaxTextBytes = 2 << 20
const MaxPages = 512
const MaxMemoryBytes uint64 = 256 << 20
const memoryInterval = 20 * time.Millisecond

var ErrUnsupported = errors.New("native PDF text extraction is unavailable on this platform")
var ErrMemoryLimit = errors.New("PDF decoder exceeded its 256 MiB observed memory budget")
var ErrOutputLimit = errors.New("PDF text exceeds the 2 MiB extraction budget")
var ErrPageLimit = errors.New("PDF exceeds the 512-page extraction budget")
var ErrInvalidPDF = errors.New("PDF is invalid, locked, or has no extractable text")

// Output describes actual PDFKit text extraction; it is not OCR, page rendering,
// a layout/table conversion, or delivery to a model.
type Output struct {
	Text      string
	Pages     int
	Processor string
}

type Processor struct {
	mu          sync.Mutex
	closed      bool
	active      int
	idle        chan struct{}
	children    map[*process.Process]struct{}
	lifetime    context.Context
	stop        context.CancelFunc
	memoryLimit uint64
}

// NewProcessor is cold: no framework, subprocess, source file or account is
// touched until an explicitly supplied source is extracted.
func NewProcessor() *Processor {
	ctx, cancel := context.WithCancel(context.Background())
	return &Processor{lifetime: ctx, stop: cancel, memoryLimit: MaxMemoryBytes, children: make(map[*process.Process]struct{})}
}

var decoderSlots = make(chan struct{}, 2)

func (p *Processor) enter(ctx context.Context) (func(), error) {
	if p == nil {
		return nil, ErrUnsupported
	}
	p.mu.Lock()
	if p.closed {
		p.mu.Unlock()
		return nil, errors.New("document processor is closed")
	}
	if p.active == 0 {
		p.idle = make(chan struct{})
	}
	p.active++
	p.mu.Unlock()
	return func() {
		p.mu.Lock()
		p.active--
		if p.active == 0 {
			close(p.idle)
		}
		p.mu.Unlock()
	}, ctx.Err()
}
func (p *Processor) start(ctx context.Context, config process.Config) (*process.Process, error) {
	select {
	case decoderSlots <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-p.lifetime.Done():
		return nil, context.Canceled
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || ctx.Err() != nil {
		<-decoderSlots
		return nil, context.Canceled
	}
	child, err := process.Start(config)
	if err != nil {
		<-decoderSlots
		return nil, err
	}
	p.children[child] = struct{}{}
	go func() { <-child.Done(); p.release(child) }()
	return child, nil
}
func (p *Processor) release(child *process.Process) {
	if !child.CleanupComplete() {
		return
	}
	p.mu.Lock()
	if _, ok := p.children[child]; ok {
		delete(p.children, child)
		<-decoderSlots
	}
	p.mu.Unlock()
}

func (p *Processor) ExtractPDF(ctx context.Context, source *os.File) (Output, error) {
	if source == nil {
		return Output{}, errors.New("an explicit read-only PDF file is required")
	}
	info, err := source.Stat()
	if err != nil {
		return Output{}, err
	}
	if !info.Mode().IsRegular() || info.Size() < 1 || info.Size() > MaxInputBytes {
		return Output{}, errors.New("PDF text processing requires a regular file of 1 byte..32 MiB")
	}
	end, err := p.enter(ctx)
	if end != nil {
		defer end()
	}
	if err != nil {
		return Output{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	stop := context.AfterFunc(p.lifetime, cancel)
	defer stop()
	return p.extract(ctx, source)
}

// Close cancels admission and joins actual decoder/read/watchdog work. A
// timeout or failed physical cleanup retains the original child and slot for
// an explicit retry; it never declares a resource-free owner prematurely.
func (p *Processor) Close(ctx context.Context) error {
	if p == nil {
		return nil
	}
	p.mu.Lock()
	p.closed = true
	p.stop()
	children := make([]*process.Process, 0, len(p.children))
	for child := range p.children {
		children = append(children, child)
	}
	idle, active := p.idle, p.active
	p.mu.Unlock()
	for _, child := range children {
		child.BeginClose()
	}
	var err error
	for _, child := range children {
		err = errors.Join(err, child.Close(ctx))
		p.release(child)
	}
	if active > 0 {
		select {
		case <-idle:
		case <-ctx.Done():
			return errors.Join(err, ctx.Err())
		}
	}
	if !p.CleanupComplete() {
		return errors.Join(err, errors.New("document decoder physical cleanup incomplete; original owner retained"))
	}
	return err
}
func (p *Processor) CleanupComplete() bool {
	if p == nil {
		return true
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.closed && p.active == 0 && len(p.children) == 0
}
