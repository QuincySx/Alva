// INPUT: Typed-nil model/log/tool implementations and mutable kernel state
// OUTPUT: Constructor rejection and atomic failed replacement without nil method calls
// POS: [L1 kernel contracts] State setters preserve live executable capabilities when a missing implementation is proposed.
package abi

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type stateModel func(context.Context, ModelRequest, func(wire.StreamChunk) error) error

func (fn stateModel) Stream(ctx context.Context, request ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

type stateLog struct{ ConversationLog }

func (stateLog) ID() string { return "state-log" }

type absentLog struct{ ConversationLog }
type absentTool struct{ Tool }

func TestTypedNilStateInputsAndReplacementAreAtomic(t *testing.T) {
	var missing stateModel
	if _, err := NewAgentState(missing, stateLog{}, nil); err == nil {
		t.Fatal("typed-nil function model admitted")
	}
	called := 0
	model := stateModel(func(context.Context, ModelRequest, func(wire.StreamChunk) error) error { called++; return nil })
	if _, err := NewAgentState(model, (*absentLog)(nil), nil); err == nil {
		t.Fatal("typed-nil log admitted")
	}
	state, err := NewAgentState(model, stateLog{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := state.SetModel(missing); err == nil {
		t.Fatal("typed-nil model replaced live implementation")
	}
	if err := state.SetSession((*absentLog)(nil)); err == nil {
		t.Fatal("typed-nil log replaced live implementation")
	}
	if state.Session().ID() != "state-log" {
		t.Fatal("failed replacement mutated log")
	}
	if err := state.Model().Stream(t.Context(), ModelRequest{}, nil); err != nil || called != 1 {
		t.Fatal("failed replacement mutated model", err)
	}
	registry := state.Tools()
	before := registry.Snapshot().Version
	if err := registry.Register((*absentTool)(nil)); err == nil {
		t.Fatal("typed-nil tool registered")
	}
	if _, err := registry.Replace((*absentTool)(nil)); err == nil {
		t.Fatal("typed-nil tool replaced registered implementation")
	}
	if after := registry.Snapshot(); after.Version != before || len(after.Tools) != 0 {
		t.Fatal("failed tool mutation advanced registry")
	}
}
