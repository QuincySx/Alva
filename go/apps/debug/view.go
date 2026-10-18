// INPUT: Actual UI-thread dispatcher, observer lifetime and registered snapshot/action/window capture callbacks
// OUTPUT: View with bounded scheduling, revision checks and observable true action settlement
// POS: [L6 diagnostics] A diagnostic observer cannot execute a queued action after cancellation or allocate unbounded UI closures.
package debug

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sync/atomic"
)

type View struct {
	life       context.Context
	detach     context.CancelFunc
	enqueue    func(func())
	snapshot   func() Snapshot
	apply      func(string, json.RawMessage) error
	capture    func() ([]byte, error)
	gate       chan struct{}
	trackFrame bool
	hasFrame   bool
	frame      [32]byte
	invalidate func()
}
type viewResult struct {
	snapshot Snapshot
	image    []byte
	action   ActionResult
	err      error
}

func NewView(owner context.Context, enqueue func(func()), snapshot func() Snapshot, apply func(string, json.RawMessage) error, capture func() ([]byte, error)) (*View, error) {
	if owner == nil || enqueue == nil || snapshot == nil {
		return nil, errors.New("actual view owner and UI-thread snapshot dispatcher required")
	}
	life, detach := context.WithCancel(owner)
	return &View{life: life, detach: detach, enqueue: enqueue, snapshot: snapshot, apply: apply, capture: capture, gate: make(chan struct{}, 1)}, nil
}

// TrackRendering is bound on the UI thread before capture is exposed. The
// returned callback runs at the end of the actual view Build, not from HTTP.
// A rasterizer retaining an older scene must not silently return stale pixels.
func (v *View) TrackRendering(invalidate func()) (rendered, changed func()) {
	v.trackFrame = true
	v.invalidate = invalidate
	return func() { hash, err := v.presentationHash(); v.frame = hash; v.hasFrame = err == nil }, func() { v.hasFrame = false }
}

func (v *View) presentationHash() ([32]byte, error) {
	snapshot := v.snapshot()
	snapshot.ViewID = ""
	snapshot.Revision = ""
	data, err := json.Marshal(snapshot)
	if err != nil || len(data) > 128<<10 {
		return [32]byte{}, errors.New("rendered view snapshot exceeds budget")
	}
	return sha256.Sum256(data), nil
}

func (v *View) schedule(ctx context.Context, fn func() viewResult) (<-chan viewResult, error) {
	if v.life.Err() != nil {
		return nil, ErrUnavailable
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	select {
	case v.gate <- struct{}{}:
	default:
		return nil, ErrBusy
	}
	done := make(chan viewResult, 1)
	var state atomic.Int32 // queued=0; running=1; settled/cancelled=2.
	stopRequest := context.AfterFunc(ctx, func() {
		if state.CompareAndSwap(0, 2) {
			done <- viewResult{err: ctx.Err()}
		}
	})
	stopOwner := context.AfterFunc(v.life, func() {
		if state.CompareAndSwap(0, 2) {
			done <- viewResult{err: ErrUnavailable}
		}
	})
	v.enqueue(func() {
		defer func() { <-v.gate; stopRequest(); stopOwner() }()
		if !state.CompareAndSwap(0, 1) {
			return
		}
		if ctx.Err() != nil {
			state.Store(2)
			done <- viewResult{err: ctx.Err()}
			return
		}
		if v.life.Err() != nil {
			state.Store(2)
			done <- viewResult{err: ErrUnavailable}
			return
		}
		result := fn()
		state.Store(2)
		done <- result
	})
	return done, nil
}

func (v *View) current(id string) (Snapshot, error) {
	snapshot := v.snapshot()
	snapshot.ViewID = id
	snapshot.Revision = ""
	budget := 128
	if err := validateNode(snapshot.Tree, 0, &budget); err != nil {
		return Snapshot{}, err
	}
	data, err := json.Marshal(snapshot)
	if err != nil || len(data) > 128<<10 {
		return Snapshot{}, errors.New("view snapshot exceeds 128 KiB")
	}
	// Freeze maps/slices while still on the UI thread. HTTP serialization must
	// not race a later frame or turn exact integer state into float64 previews.
	var frozen Snapshot
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	if err := decoder.Decode(&frozen); err != nil {
		return Snapshot{}, err
	}
	hash := sha256.Sum256(data)
	frozen.Revision = hex.EncodeToString(hash[:])
	return frozen, nil
}

func (v *View) read(ctx context.Context, id string) (Snapshot, error) {
	done, err := v.schedule(ctx, func() viewResult { s, e := v.current(id); return viewResult{snapshot: s, err: e} })
	if err != nil {
		return Snapshot{}, err
	}
	select {
	case r := <-done:
		return r.snapshot, r.err
	case <-ctx.Done():
		return Snapshot{}, ctx.Err()
	case <-v.life.Done():
		return Snapshot{}, ErrUnavailable
	}
}

func (v *View) action(ctx context.Context, a Action) (<-chan viewResult, error) {
	return v.schedule(ctx, func() viewResult {
		current, err := v.current(a.ViewID)
		if err != nil {
			return viewResult{err: err}
		}
		if current.Revision != a.Revision {
			return viewResult{err: ErrStale}
		}
		if v.apply == nil {
			return viewResult{err: errors.New("view has no declared action handler")}
		}
		if v.trackFrame {
			v.hasFrame = false
		}
		if err := v.apply(a.Method, a.Args); err != nil {
			return viewResult{err: err}
		}
		after, err := v.current(a.ViewID)
		// The effect already happened. A later snapshot failure must not make a
		// retry execute it again or falsely describe the mutation as rejected.
		result := ActionResult{OperationID: a.OperationID, Status: "applied", Revision: after.Revision}
		if err != nil {
			result.Error = "action applied; follow-up snapshot unavailable"
		}
		return viewResult{action: result}
	})
}

func (v *View) image(ctx context.Context) ([]byte, error) {
	if v.capture == nil {
		return nil, errors.New("registered view has no window capture")
	}
	done, err := v.schedule(ctx, func() viewResult {
		if v.trackFrame {
			hash, err := v.presentationHash()
			if err != nil {
				return viewResult{err: err}
			}
			if !v.hasFrame || v.frame != hash {
				if v.invalidate != nil {
					v.invalidate()
				}
				return viewResult{err: ErrFramePending}
			}
		}
		data, e := v.capture()
		return viewResult{image: data, err: e}
	})
	if err != nil {
		return nil, err
	}
	select {
	case r := <-done:
		return r.image, r.err
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-v.life.Done():
		return nil, ErrUnavailable
	}
}
