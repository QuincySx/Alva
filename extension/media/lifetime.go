// INPUT: Actual native runtime, caller/owner cancellation and existing Tool/model invocations.
// OUTPUT: Run lifecycle, Close/CleanupComplete and private active-call joining for tools, model calls and receipts.
// POS: [L4 extension] Media ownership ends after callbacks/facts and decoder cleanup, independently of pending selection state.
package media

import (
	"context"
	"errors"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func (p *Plugin) OnRunStart(ctx context.Context, r abi.RuntimeContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if r.State == nil || !p.library.log.SameAuthority(r.State.Session()) || r.RunID == "" || r.Workspace != p.config.Cwd {
		return errors.New("media actual native run/workspace mismatch")
	}
	if p.config.ImageCapable && !p.matchesModel(r) {
		return errors.New("image capability declaration is not bound to the actual configured provider/model identity")
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.runID != "" || p.active != 0 {
		return errors.New("media plugin is active, closed or still cleaning up")
	}
	p.runID = r.RunID
	p.life, p.cancel = context.WithCancel(ctx)
	p.ending = false
	p.pending = nil
	return nil
}
func (p *Plugin) OnRunEnd(ctx context.Context, r abi.RuntimeContext, _ error) error {
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	if deadline, ok := ctx.Deadline(); ok {
		bounded, end := context.WithDeadline(cleanup, deadline)
		defer end()
		cleanup = bounded
	}
	return p.stop(cleanup, r.RunID, false)
}

// Explicit owner Close is terminal. Run end may reuse the no-processor recipe;
// an explicit native Processor retains its existing attempt-owned terminal Close.
func (p *Plugin) Close(ctx context.Context) error { return p.stop(ctx, "", true) }
func (p *Plugin) stop(ctx context.Context, expectedRun string, permanent bool) error {
	p.mu.Lock()
	if !permanent && (expectedRun == "" || p.runID != expectedRun) {
		p.mu.Unlock()
		return nil
	}
	if permanent {
		p.closed = true
	}
	p.ending = true
	if p.cancel != nil {
		p.cancel()
	}
	life, runID, idle, active := p.life, p.runID, p.idle, p.active
	p.mu.Unlock()
	var err error
	if p.config.Processor != nil {
		err = p.config.Processor.Close(ctx)
	}
	if active != 0 {
		select {
		case <-idle:
		case <-ctx.Done():
			return errors.Join(err, ctx.Err())
		}
	}
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	case <-ctx.Done():
		return errors.Join(err, ctx.Err())
	}
	p.mu.Lock()
	if p.life != life || p.runID != runID {
		p.mu.Unlock()
		return err
	}
	pending := p.pending
	p.pending = nil
	p.mu.Unlock()
	if pending != nil {
		_, receiptErr := p.library.receipt(ctx, *pending, "run_ended_before_submission_no_replay", nil)
		err = errors.Join(err, receiptErr)
	}
	p.mu.Lock()
	p.runID = ""
	p.life = nil
	p.cancel = nil
	p.ending = false
	p.mu.Unlock()
	return err
}
func (p *Plugin) CleanupComplete() bool {
	p.mu.Lock()
	active := p.active
	p.mu.Unlock()
	if active != 0 {
		return false
	}
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	default:
		return false
	}
	return p.config.Processor == nil || p.config.Processor.CleanupComplete()
}
func (p *Plugin) check(e abi.ToolExecutionContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.ending || p.life == nil || p.life.Err() != nil || p.runID == "" || p.runID != e.RunID || e.SessionID != p.config.SessionID || !p.library.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd {
		return errors.New("media does not belong to this actual active run")
	}
	return nil
}

// Keep model and Tool calls independent: joining their lifetime must not impose
// a new global serialization between model streams and optional tool consumers.
func (p *Plugin) begin(ctx context.Context, runID string) (context.Context, func(), error) {
	p.mu.Lock()
	if p.closed || p.ending || p.runID == "" || p.runID != runID || p.life == nil {
		p.mu.Unlock()
		return nil, nil, errors.New("media run is closed or ending")
	}
	life := p.life
	if p.active == 0 {
		p.idle = make(chan struct{})
	}
	p.active++
	p.mu.Unlock()
	operation, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(life, cancel)
	if life.Err() != nil {
		cancel()
	}
	finish := func() {
		stop()
		cancel()
		p.mu.Lock()
		p.active--
		if p.active == 0 {
			close(p.idle)
		}
		p.mu.Unlock()
	}
	if err := operation.Err(); err != nil {
		finish()
		return nil, nil, err
	}
	return operation, finish, nil
}
