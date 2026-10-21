// INPUT: Explicit workspace-relative compatibility config and native session capabilities.
// OUTPUT: Cold source inspection and one optional compatibility/subagent contribution per native turn.
// POS: [L6 application] Opt-in format loading without global discovery or core interface changes.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"path/filepath"
	"time"

	"github.com/QuincySx/Alva/extension/compatibility"
	"github.com/QuincySx/Alva/extension/subagent"
	"github.com/QuincySx/Alva/harness"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func validCLICompatibilityPath(value string) bool {
	return compatibility.ValidPath(value) && value != "."
}
func inspectCLICompatibility(ctx context.Context, workspace, file string, output io.Writer) (err error) {
	workspace, err = filepath.Abs(workspace)
	if err != nil {
		return err
	}
	snapshot, err := compatibility.OpenFile(ctx, workspace, file)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, snapshot.Close(context.WithoutCancel(ctx))) }()
	return json.NewEncoder(output).Encode(snapshot.Receipt())
}

func (r *cliNativeRuntime) compatibility(ctx context.Context) (*compatibility.Snapshot, error) {
	if r.config.CompatibilityFile == "" {
		return nil, nil
	}
	snapshot, err := compatibility.OpenFile(ctx, r.workspace, r.config.CompatibilityFile)
	if err != nil {
		return nil, err
	}
	r.retain(snapshot)
	if len(snapshot.Receipt().Agents) > 0 && r.config.Native == nil {
		return nil, errors.New("compatibility agent definitions require explicit --data-dir for independent native child sessions")
	}
	return snapshot, nil
}

func (r *cliNativeRuntime) compatibilityPlugins(ctx context.Context, snapshot *compatibility.Snapshot, refs agentcontext.ReferenceBackend, guard *security.Guard, fileTools []abi.Tool) ([]core.Plugin, []abi.Tool, error) {
	if snapshot == nil {
		return nil, nil, nil
	}
	plugin := snapshot.Plugin(r.log.ID(), refs, nil)
	plugins, tools := []core.Plugin{plugin}, plugin.Tools()
	if len(snapshot.Receipt().Agents) > 0 {
		factory, err := harness.NewNativeChildFactory(ctx, harness.NativeChildConfig{Repository: r.config.Repository, Owner: cliOwner, ParentID: r.log.ID(), Cwd: r.workspace, Guard: guard, Bindings: harness.NativeFileBindings(fileTools, r.files)})
		if err != nil {
			return nil, nil, err
		}
		children, err := subagent.New(subagent.Config{SessionID: r.log.ID(), Factory: factory, Guard: guard, References: refs, Templates: snapshot.Templates(), Timeout: 2 * time.Minute, MaxIterations: 16, MaxDepth: 3, MaxChildren: 8, MaxParallelModels: 2})
		if err != nil {
			return nil, nil, err
		}
		r.retain(children)
		plugins = append(plugins, children)
		tools = append(tools, children.Tool())
	}
	return plugins, tools, nil
}
