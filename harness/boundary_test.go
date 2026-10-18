// INPUT: Real native repositories, foreign same-ID logs and a Builder mutated from a Plugin identity callback.
// OUTPUT: Native recipe authority and fixed per-build harness recipe regressions.
// POS: [L5 harness] Assembly callbacks cannot borrow native session authority or alter the build already in progress.
package harness

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestNativeRecipeRejectsSameIDForeignLogBeforeModel(t *testing.T) {
	repo, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err := repo.Create(t.Context(), "owner", "native", ""); err != nil {
		t.Fatal(err)
	}
	plugins, err := NativeSessionPlugins(t.Context(), repo, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	calls := 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	builder := NewBuilder(model, core.NewMemorySession("native")).Workspace(t.TempDir())
	for _, p := range plugins {
		builder.Plugin(p)
	}
	agent, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(event abi.Event) error { return nil }); err == nil {
		t.Error("same-ID memory log borrowed native recipe authority")
	}
	if calls != 0 {
		t.Fatal("foreign recipe reached actual model", calls)
	}
}

type recipeMutationPlugin struct{ once func() }

func (p *recipeMutationPlugin) Name() string {
	if p.once != nil {
		f := p.once
		p.once = nil
		f()
	}
	return "recipe-mutation"
}
func (*recipeMutationPlugin) Register(context.Context, *core.Registrar) error { return nil }
func TestHarnessBuildFreezesToolsBeforePluginCallbacks(t *testing.T) {
	builder := NewBuilder(demoModel{}, core.NewMemorySession("recipe")).Workspace(t.TempDir())
	p := &recipeMutationPlugin{once: func() { builder.Tool(echoTool{}) }}
	builder.Plugin(p)
	for build := 0; build < 2; build++ {
		agent, err := builder.Build(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		found := false
		for _, tool := range agent.Snapshot().Tools {
			if tool == "demo_echo" {
				found = true
			}
		}
		if found != (build == 1) {
			t.Fatal("identity callback changed current build rather than the next recipe", build, found)
		}
	}
}
func TestFailedParallelRunCannotReleaseAnotherNativeClaim(t *testing.T) {
	repo, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(t.Context(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	state, _ := abi.NewAgentState(demoModel{}, log, nil)
	first := abi.RuntimeContext{State: state, RunID: "first"}
	second := abi.RuntimeContext{State: state, RunID: "second"}
	claim := &nativeRunClaim{repository: repo, owner: "owner", id: "native"}
	if err := claim.OnRunStart(t.Context(), first); err != nil {
		t.Fatal(err)
	}
	defer claim.OnRunEnd(context.Background(), first, nil)
	if err := claim.OnRunStart(t.Context(), second); !errors.Is(err, session.ErrActive) {
		t.Fatal("parallel claim unexpectedly admitted", err)
	}
	if err := claim.OnRunEnd(t.Context(), second, nil); err != nil {
		t.Fatal(err)
	}
	_, release, err := repo.BeginRun(t.Context(), "owner", "native")
	if err == nil {
		release()
		t.Fatal("failed parallel run released the active owner's native claim")
	}
	if !errors.Is(err, session.ErrActive) {
		t.Fatal(err)
	}
}
