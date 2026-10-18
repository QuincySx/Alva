// INPUT: context, errors, sync, time, kernel/abi, llm/wire
// OUTPUT: Private model invocation binding, admission and continuation coordination
// POS: [L2 kernel runtime] Binds actual authority and prevents published failed attempts from becoming successful streams.
package core

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type wrappedModel struct {
	actual     abi.LanguageModel
	runtime    abi.RuntimeContext
	middleware []abi.Middleware
	audit      *hookBatch
	admission  *hookBatch
}

type modelInvocation struct {
	mu        sync.Mutex
	active    bool
	published bool
	failure   error
}

func (invocation *modelInvocation) retainPublishedFailure(err error) {
	if err == nil {
		return
	}
	invocation.mu.Lock()
	defer invocation.mu.Unlock()
	if invocation.published {
		invocation.failure = mergeInvocationErrors(invocation.failure, err)
	}
}

// Ordinary onion layers wrap an existing failure. Keep one containing tree
// instead of doubling shared error subtrees at every layer.
func mergeInvocationErrors(current, next error) error {
	if current == nil {
		return next
	}
	if next == nil || errors.Is(current, next) {
		return current
	}
	if errors.Is(next, current) {
		return next
	}
	return errors.Join(current, next)
}

func cloneModelRequest(request abi.ModelRequest) abi.ModelRequest {
	request.Messages = wire.CloneMessages(request.Messages)
	request.Tools = cloneDefinitions(request.Tools)
	request.Config = wire.CloneModelConfig(request.Config)
	return request
}

func (model wrappedModel) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	actual := model.actual
	if actual == nil {
		actual = model.runtime.State.Model()
	}
	canonical := model.runtime
	canonical.InvocationModel = actual
	invocation := &modelInvocation{}
	// This is the logical output, not an attempt's private sampling/buffer emit.
	// There is no rollback for a chunk already delivered here.
	publish := func(chunk wire.StreamChunk) error {
		invocation.mu.Lock()
		if invocation.failure != nil {
			err := invocation.failure
			invocation.mu.Unlock()
			return err
		}
		invocation.published = true
		invocation.mu.Unlock()
		return emit(chunk)
	}
	var next abi.ModelFunc = func(ctx context.Context, _ abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error) (attemptErr error) {
		invocation.mu.Lock()
		if invocation.active {
			invocation.failure = mergeInvocationErrors(invocation.failure, errors.New("concurrent model next is not supported; buffer attempts in a model adapter"))
			err := invocation.failure
			invocation.mu.Unlock()
			return err
		}
		if invocation.failure != nil {
			err := invocation.failure
			invocation.mu.Unlock()
			return err
		}
		invocation.active = true
		invocation.mu.Unlock()
		defer func() {
			invocation.mu.Lock()
			invocation.active = false
			if attemptErr != nil && invocation.published {
				invocation.failure = mergeInvocationErrors(invocation.failure, attemptErr)
			}
			invocation.mu.Unlock()
		}()
		if err := ctx.Err(); err != nil {
			return err
		}
		if emit == nil {
			return errors.New("model next callback is required")
		}
		// Keep the checked request separate from every validator's owned copy.
		frozen := cloneModelRequest(request)
		if err := frozen.Config.Validate(); err != nil {
			return err
		}
		for i, layer := range model.middleware {
			if admission, ok := layer.(abi.ModelAdmissionHook); ok {
				started := time.Now()
				err := admission.AdmitModel(ctx, canonical, cloneModelRequest(frozen))
				model.admission.observe(layer, i, started, err)
				if err != nil {
					return err
				}
			}
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		// Each actual Stream owns its callback lease, independently of the whole
		// wrapper invocation. A late callback from an old attempt cannot leak
		// into a later attempt's still-open logical output.
		var callbackMu sync.Mutex
		open := true
		var callbackErr error
		attemptErr = actual.Stream(ctx, frozen, func(chunk wire.StreamChunk) error {
			callbackMu.Lock()
			defer callbackMu.Unlock()
			if !open {
				return errors.New("model stream callback called after stream returned")
			}
			if callbackErr != nil {
				return callbackErr
			}
			callbackErr = emit(wire.CloneStreamChunk(chunk))
			return callbackErr
		})
		callbackMu.Lock()
		open = false
		attemptErr = errors.Join(attemptErr, callbackErr, ctx.Err())
		callbackMu.Unlock()
		return attemptErr
	}
	for i := len(model.middleware) - 1; i >= 0; i-- {
		if wrapper, ok := model.middleware[i].(abi.ModelWrapper); ok {
			inner := next
			layer, index := model.middleware[i], i
			next = func(ctx context.Context, _ abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				var leaseMu sync.Mutex
				changed := sync.NewCond(&leaseMu)
				open, active := true, 0
				guardedNext := func(ctx context.Context, _ abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
					leaseMu.Lock()
					if !open {
						leaseMu.Unlock()
						return errors.New("model next called after wrapper returned")
					}
					if active > 0 {
						leaseMu.Unlock()
						invocation.mu.Lock()
						invocation.failure = mergeInvocationErrors(invocation.failure, errors.New("concurrent model next is not supported; buffer attempts in a model adapter"))
						err := invocation.failure
						invocation.mu.Unlock()
						return err
					}
					active++
					leaseMu.Unlock()
					defer func() { leaseMu.Lock(); active--; changed.Broadcast(); leaseMu.Unlock() }()
					err := inner(ctx, canonical, request, emit)
					invocation.retainPublishedFailure(err)
					return err
				}
				started := time.Now()
				err := wrapper.WrapModel(ctx, canonical, request, emit, guardedNext)
				leaseMu.Lock()
				open = false
				for active > 0 {
					changed.Wait()
				}
				leaseMu.Unlock()
				invocation.retainPublishedFailure(err)
				model.audit.observe(layer, index, started, err)
				return err
			}
		}
	}
	err := next(ctx, canonical, request, publish)
	invocation.mu.Lock()
	defer invocation.mu.Unlock()
	return mergeInvocationErrors(err, invocation.failure)
}
