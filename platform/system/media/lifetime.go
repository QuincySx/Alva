// INPUT: Explicit decoder calls, owned native processes and terminal owner shutdown
// OUTPUT: Processor Close/CleanupComplete with active-call joining and retained late-cleanup handles
// POS: [platform / OS primitive] Physical resource ownership is separate from decoder errors, media evidence and model policies
package media

import (
	"context"
	"errors"
	"sync"

	"github.com/QuincySx/Alva/platform/system/process"
)

// A failed timed cleanup must not release its global native-child slot or lose
// the only process handle. This is independent of the image/operation budget.
var decoderChildren = make(chan struct{}, 2)

func (p *Processor) enter() (func(), error) {
	if p == nil {
		return nil, errors.New("explicit media processor required")
	}
	p.mu.Lock()
	if p.closed {
		p.mu.Unlock()
		return nil, errors.New("media processor owner is closed; explicitly construct a new approved processor")
	}
	if p.lifetime == nil {
		p.lifetime, p.stop = context.WithCancel(context.Background())
	}
	if p.active == 0 {
		p.idle = make(chan struct{})
	}
	p.active++
	p.mu.Unlock()
	var once sync.Once
	return func() {
		once.Do(func() {
			p.mu.Lock()
			p.active--
			if p.active == 0 {
				close(p.idle)
			}
			p.mu.Unlock()
		})
	}, nil
}

func (p *Processor) ownerContext() context.Context {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.lifetime
}

func (p *Processor) startProcess(ctx context.Context, config process.Config) (*process.Process, error) {
	select {
	case decoderChildren <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-p.ownerContext().Done():
		return nil, context.Canceled
	}
	p.mu.Lock()
	if p.closed || ctx.Err() != nil {
		p.mu.Unlock()
		<-decoderChildren
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("media processor owner is closed")
	}
	child, err := process.Start(config)
	if err != nil {
		p.mu.Unlock()
		<-decoderChildren
		return nil, err
	}
	if p.children == nil {
		p.children = make(map[*process.Process]func())
	}
	p.children[child] = func() { <-decoderChildren }
	p.mu.Unlock()
	go func() { <-child.Done(); p.releaseChild(child) }()
	return child, nil
}

func (p *Processor) releaseChild(child *process.Process) {
	if !child.CleanupComplete() {
		return
	}
	p.mu.Lock()
	if release, ok := p.children[child]; ok {
		delete(p.children, child)
		release()
	}
	p.mu.Unlock()
}

// Close is terminal. A later play/seek or new attempt must construct its own
// Processor from the same explicitly approved immutable executable snapshot.
// Timeout retains pending children and active calls for an explicit Close retry.
func (p *Processor) Close(ctx context.Context) error {
	if p == nil {
		return nil
	}
	p.mu.Lock()
	p.closed = true
	if p.stop != nil {
		p.stop()
	}
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
		p.releaseChild(child)
	}
	if active > 0 {
		select {
		case <-idle:
		case <-ctx.Done():
			return errors.Join(err, ctx.Err())
		}
	}
	if !p.CleanupComplete() {
		return errors.Join(err, errors.New("media processor physical cleanup incomplete; owner retained"))
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
