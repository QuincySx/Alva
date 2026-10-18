// INPUT: context, channels, atomic/sync, actual SDK loop and model-wrapper callbacks
// OUTPUT: Real entered-model join, closed next/callback leases and supported retry regressions
// POS: [L2.5 SDK assembly] No early wrapper return can manufacture model completion; synchronous multi-next/retry remains supported.
package core_test

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestModelWrapperEarlyReturnJoinsActuallyEnteredNext(t *testing.T) {
	entered, release, returned, innerDone := make(chan struct{}), make(chan struct{}), make(chan struct{}), make(chan struct{})
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		close(entered)
		<-release
		defer close(innerDone)
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := &runtimeHooks{wrapper: func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		go func() { _ = next(ctx, r, q, emit) }()
		<-entered
		close(returned)
		return nil
	}}
	agent := build(t, core.NewBuilder(model, core.NewMemorySession("model-join")).Middleware(hook))
	done := make(chan error, 1)
	go func() { done <- agent.Run(context.Background(), nil, discard) }()
	<-returned
	select {
	case err := <-done:
		t.Fatal("returned before actual next exited", err)
	case <-time.After(20 * time.Millisecond):
	}
	close(release)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	select {
	case <-innerDone:
	default:
		t.Fatal("inner still alive after Run")
	}
}
func TestSavedModelNextAndCallbackCannotExecuteAfterReturn(t *testing.T) {
	var calls atomic.Int32
	var saved func() error
	var callback func(wire.StreamChunk) error
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls.Add(1)
		callback = emit
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := &runtimeHooks{wrapper: func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		saved = func() error { return next(ctx, r, q, emit) }
		return next(ctx, r, q, emit)
	}}
	session := core.NewMemorySession("late-model")
	agent := build(t, core.NewBuilder(model, session).Middleware(hook))
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	before := len(records(t, session))
	if err := saved(); err == nil || !strings.Contains(err.Error(), "after wrapper returned") {
		t.Fatal("late next was not rejected", err)
	}
	if err := callback(wire.StreamChunk{Kind: wire.ChunkText, Text: "late body"}); err == nil || !strings.Contains(err.Error(), "after stream returned") {
		t.Fatal("late callback altered completed stream", err)
	}
	if calls.Load() != 1 || len(records(t, session)) != before {
		t.Fatal("late execution changed facts", calls.Load())
	}
}
func TestModelWrapperCanRecoverRealTransportFailureByRetry(t *testing.T) {
	var calls atomic.Int32
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if calls.Add(1) == 1 {
			return errors.New("retryable transport fixture")
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "recovered"}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := &runtimeHooks{wrapper: func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		if err := next(ctx, r, q, emit); err != nil {
			return next(ctx, r, q, emit)
		}
		return nil
	}}
	if err := build(t, core.NewBuilder(model, core.NewMemorySession("retry-model")).Middleware(hook)).Run(context.Background(), nil, discard); err != nil || calls.Load() != 2 {
		t.Fatal("supported retry was forbidden", err, calls.Load())
	}
}
func TestConcurrentModelNextIsRejectedBeforeSecondExecution(t *testing.T) {
	var calls atomic.Int32
	entered, release := make(chan struct{}), make(chan struct{})
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls.Add(1)
		close(entered)
		<-release
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := &runtimeHooks{wrapper: func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		done := make(chan struct{})
		go func() { defer close(done); _ = next(ctx, r, q, emit) }()
		<-entered
		second := next(ctx, r, q, emit)
		close(release)
		<-done
		if second == nil || !strings.Contains(second.Error(), "concurrent model next") {
			t.Error("concurrent continuation accepted", second)
		}
		return nil // A protocol violation must survive attempted suppression.
	}}
	err := build(t, core.NewBuilder(model, core.NewMemorySession("parallel-model")).Middleware(hook)).Run(context.Background(), nil, discard)
	if err == nil || !strings.Contains(err.Error(), "concurrent model next") || calls.Load() != 1 {
		t.Fatal("concurrent invocation admitted or suppressed", err, calls.Load())
	}
}
