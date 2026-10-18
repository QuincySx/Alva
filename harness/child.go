// INPUT: context, errors, filepath, reflect, time, agent/core/context/security, extension/builtin/mcp/subagent, storage/session.
// OUTPUT: NativeChildFactory, NativeChildConfig, ChildToolBinding, BoundChildTool, NativeFileBindings and MCPChildBinding.
// POS: [L5 harness] Native child assembly creates independent durable authorities and explicitly rebinds only actual direct-parent capabilities.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"time"

	"github.com/QuincySx/Alva/extension/builtin"
	managedmcp "github.com/QuincySx/Alva/extension/mcp"
	"github.com/QuincySx/Alva/extension/subagent"
	"github.com/QuincySx/Alva/platform/storage/session"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type ChildBinding struct {
	ParentID, SessionID, Workspace string
	References                     agentcontext.ReferenceBackend
	Guard                          *security.Guard
}
type ChildToolBinder func(context.Context, ChildBinding) (BoundChildTool, error)
type ChildToolBinding struct {
	Parent abi.Tool
	Bind   ChildToolBinder
}
type BoundChildTool struct {
	Tool abi.Tool
	// Plugin, when nonnil, registers Tool itself; the factory does not also add it.
	Plugin core.Plugin
	Close  func(context.Context) error
	// Next explicitly grants permission to rebind this capability for a further
	// direct descendant. Nil means this child cannot delegate it again.
	Next ChildToolBinder
}
type NativeChildConfig struct {
	Repository           *session.Repository
	Owner, ParentID, Cwd string
	Guard                *security.Guard
	Bindings             []ChildToolBinding
	Locks                *core.ToolLockRegistry
	ToolTimeout          time.Duration
}
type NativeChildFactory struct{ config NativeChildConfig }

func (factory *NativeChildFactory) GrantableTools() []abi.Tool {
	tools := make([]abi.Tool, 0, len(factory.config.Bindings))
	for _, binding := range factory.config.Bindings {
		tools = append(tools, binding.Parent)
	}
	return tools
}

func NewNativeChildFactory(ctx context.Context, config NativeChildConfig) (*NativeChildFactory, error) {
	if config.Repository == nil || config.Guard == nil || config.Owner == "" || config.ParentID == "" || !filepath.IsAbs(config.Cwd) || len(config.Bindings) > 128 {
		return nil, errors.New("explicit native child repository, actual parent, absolute cwd, guard and bounded bindings required")
	}
	if _, err := config.Repository.OpenSession(ctx, config.Owner, config.ParentID); err != nil {
		return nil, err
	}
	if config.ToolTimeout == 0 {
		config.ToolTimeout = 30 * time.Second
	}
	if config.ToolTimeout <= 0 || config.ToolTimeout > 30*time.Minute {
		return nil, errors.New("invalid child tool timeout")
	}
	seen := map[string]bool{}
	for _, binding := range config.Bindings {
		if binding.Parent == nil || binding.Bind == nil || binding.Parent.Name() == "agent" || seen[binding.Parent.Name()] || !reflect.ValueOf(binding.Parent).Comparable() {
			return nil, errors.New("child bindings require distinct comparable actual parent tools and explicit binders")
		}
		seen[binding.Parent.Name()] = true
	}
	config.Bindings = append([]ChildToolBinding(nil), config.Bindings...)
	return &NativeChildFactory{config: config}, nil
}

func (factory *NativeChildFactory) Prepare(ctx context.Context, request subagent.ChildRequest) (child subagent.Child, resultErr error) {
	c := factory.config
	if request.Parent == nil || request.Parent.ID() != c.ParentID || request.Scope == nil || request.Model == nil || request.Budget == nil || request.Workspace != c.Cwd || request.ParentRunID == "" || request.SpawnCallID == "" {
		return child, errors.New("native child request does not match actual configured parent/cwd/scope")
	}
	// Verify the supplied log is this repository's actual authority, not an
	// arbitrary same-ID implementation offered by a contributed tool.
	actual, err := c.Repository.OpenSession(ctx, c.Owner, c.ParentID)
	if err != nil {
		return child, &abi.SessionError{Operation: "open actual parent", SessionID: c.ParentID, Err: err}
	}
	if !actual.SameAuthority(request.Parent) {
		return child, errors.New("native child parent is not the bound repository authority")
	}
	var bindings []ChildToolBinding
	seen := map[string]bool{}
	for _, grant := range request.Grants {
		if grant == nil || seen[grant.Name()] {
			return child, errors.New("invalid duplicate child capability grant")
		}
		seen[grant.Name()] = true
		found := false
		for _, binding := range c.Bindings {
			if sameTool(grant, binding.Parent) {
				bindings = append(bindings, binding)
				found = true
				break
			}
		}
		if !found {
			return child, fmt.Errorf("actual tool %q has no explicit child rebinding authority", grant.Name())
		}
	}
	creation, err := c.Repository.CreateChild(ctx, c.Owner, c.ParentID, request.Scope.SessionID(), request.ParentRunID, request.SpawnCallID, request.Scope.Snapshot().ID)
	if err != nil {
		return child, &abi.SessionError{Operation: "create child/link", SessionID: c.ParentID, Err: err}
	}
	log := creation.Session
	var closers []func(context.Context) error
	closeAll := func(cleanup context.Context) error {
		var err error
		for i := len(closers) - 1; i >= 0; i-- {
			err = errors.Join(err, closers[i](cleanup))
		}
		return err
	}
	// Failed setup retains the created child and a truthful failure fact; no
	// deletion, retry or fake successful execution can hide a partial setup.
	defer func() {
		if resultErr == nil {
			return
		}
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		if err := closeAll(cleanup); err != nil {
			resultErr = errors.Join(resultErr, subagent.ErrChildCleanup, err)
		}
		resultErr = errors.Join(resultErr, childLifecycle(cleanup, log, creation.Link, "prepare_failed", resultErr))
	}()
	refs, err := c.Repository.BoundReferences(ctx, c.Owner, log.ID())
	if err != nil {
		return child, &abi.SessionError{Operation: "bind child references", SessionID: log.ID(), Err: err}
	}
	plugins, err := NativeSessionPlugins(ctx, c.Repository, c.Owner, log.ID())
	if err != nil {
		return child, &abi.SessionError{Operation: "bind child native plugins", SessionID: log.ID(), Err: err}
	}
	builder := NewBuilder(request.Model, log).Workspace(c.Cwd).SystemPrompt(request.Scope.SystemPrompt()).MaxIterations(request.Scope.MaxIterations()).MaxResponseBytes(1 << 20)
	builder.Middleware(subagent.ModelBudgetMiddleware{Budget: request.Budget}).Middleware(kernel.ToolTimeoutMiddleware{Timeout: c.ToolTimeout})
	if c.Locks != nil {
		builder.Middleware(core.ToolLockMiddleware{Registry: c.Locks})
	}
	var trusted []abi.Tool
	var next []ChildToolBinding
	for _, binding := range bindings {
		bound, err := binding.Bind(ctx, ChildBinding{ParentID: c.ParentID, SessionID: log.ID(), Workspace: c.Cwd, References: refs, Guard: c.Guard})
		if bound.Close != nil {
			closers = append(closers, bound.Close)
		}
		if err != nil {
			return child, err
		}
		if bound.Tool == nil || bound.Tool.Name() != binding.Parent.Name() || sameTool(bound.Tool, binding.Parent) {
			return child, errors.New("child binder must return a fresh capability with the same explicit tool identity")
		}
		trusted = append(trusted, bound.Tool)
		if bound.Plugin != nil {
			plugins = append(plugins, bound.Plugin)
		} else {
			builder.Tool(bound.Tool)
		}
		if bound.Next != nil {
			next = append(next, ChildToolBinding{Parent: bound.Tool, Bind: bound.Next})
		}
	}
	childConfig := c
	childConfig.ParentID = log.ID()
	childConfig.Bindings = next
	childFactory, err := NewNativeChildFactory(ctx, childConfig)
	if err != nil {
		return child, err
	}
	snapshot := request.Scope.Snapshot()
	spawn, err := subagent.New(subagent.Config{SessionID: log.ID(), Factory: childFactory, Guard: c.Guard, References: refs, Scope: request.Scope, Budget: request.Budget, Timeout: request.Scope.Timeout(), MaxIterations: request.Scope.MaxIterations(), MaxDepth: snapshot.MaxDepth, Templates: request.Templates, Models: request.Models, Communications: request.Communications})
	if err != nil {
		return child, err
	}
	plugins = append(plugins, spawn)
	trusted = append(trusted, spawn.Tool())
	securityPlugin := NewSecurityPlugin(c.Guard, trusted, plugins...)
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	builder.Plugin(securityPlugin)
	if len(request.Hooks) > 0 {
		if len(request.Hooks) > 16 {
			return child, errors.New("child communication hooks exceed 16")
		}
		// This store is fresh and never imports parent entries/Bus. The bounded
		// child loop and reference projection remain distinct from token policy.
		hooks := append([]abi.ContextHooks{agentcontext.RulesHooks{}}, request.Hooks...)
		builder.ContextSystem(&abi.ContextSystem{Hooks: agentcontext.HooksChain{Hooks: hooks}, Handle: agentcontext.NewStore(0, 0, nil, nil), Apply: agentcontext.DefaultApply{References: refs}})
	}
	agent, err := builder.Build(ctx)
	if err != nil {
		return child, err
	}
	child = subagent.Child{Runner: &nativeChildRunner{inner: agent, log: log, link: creation.Link}, Session: log, Link: creation.Link, Close: func(cleanup context.Context) error {
		err := closeAll(cleanup)
		status := "closed"
		if err != nil {
			status = "cleanup_failed"
		}
		return errors.Join(err, childLifecycle(cleanup, log, creation.Link, status, err))
	}}
	return child, nil
}

func sameTool(left, right abi.Tool) bool {
	return left != nil && right != nil && reflect.TypeOf(left) == reflect.TypeOf(right) && reflect.ValueOf(left).Comparable() && reflect.ValueOf(right).Comparable() && left == right
}

type nativeChildRunner struct {
	inner Runner
	log   abi.ConversationLog
	link  abi.RecordReference
}

func (runner *nativeChildRunner) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	if err := childLifecycle(ctx, runner.log, runner.link, "started", nil); err != nil {
		return err
	}
	err := runner.inner.Run(ctx, input, emit)
	status := "completed"
	if err != nil {
		status = "failed"
		if abi.OnlyCancellation(err) {
			status = "cancelled"
		}
	}
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	return errors.Join(err, childLifecycle(cleanup, runner.log, runner.link, status, err))
}
func childLifecycle(ctx context.Context, log abi.ConversationLog, link abi.RecordReference, status string, cause error) error {
	message := ""
	if cause != nil {
		message = cause.Error()
		if len(message) > 2048 {
			message = "Failure details exceed this lifecycle receipt; see actual committed child outcomes"
		}
	}
	phase := "sdk_run"
	if status == "prepare_failed" {
		phase = "assembly"
	}
	if status == "closed" || status == "cleanup_failed" {
		phase = "resource_cleanup"
	}
	payload, _ := json.Marshal(map[string]any{"status": status, "phase": phase, "parent_link": link, "error": message, "notice": "Actual scoped child lifecycle; sdk_run completed does not imply resource cleanup completed. Parent link is metadata, not permission to read another session"})
	_, err := log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.agent", Type: "child_lifecycle", SchemaVersion: 1, Payload: payload}})
	if err != nil {
		return &abi.SessionError{Operation: "commit child lifecycle", SessionID: log.ID(), Err: err}
	}
	if err = log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush child lifecycle", SessionID: log.ID(), Err: err}
	}
	return nil
}

// NativeFileBindings only recognizes the supplied actual parent tool values;
// it does not add tools missing from that list. The pinned workspace is owned
// by the product, and is not closed by a child.
func NativeFileBindings(parentTools []abi.Tool, workspace *builtin.Workspace) []ChildToolBinding {
	var bindings []ChildToolBinding
	for _, tool := range parentTools {
		parent := tool
		var bind ChildToolBinder
		bind = func(_ context.Context, b ChildBinding) (BoundChildTool, error) {
			for _, candidate := range builtin.FileTools(workspace, b.Guard, b.References, b.SessionID) {
				if candidate.Name() == parent.Name() {
					return BoundChildTool{Tool: candidate, Next: bind}, nil
				}
			}
			return BoundChildTool{}, errors.New("parent tool is not an explicitly supported native file capability")
		}
		bindings = append(bindings, ChildToolBinding{Parent: parent, Bind: bind})
	}
	return bindings
}
func MCPChildBinding(plugin *managedmcp.Plugin) ChildToolBinding {
	if plugin == nil {
		return ChildToolBinding{}
	}
	return ChildToolBinding{Parent: plugin.Tool(), Bind: func(ctx context.Context, b ChildBinding) (BoundChildTool, error) {
		child, err := plugin.RebindChild(ctx, b.SessionID)
		if err != nil {
			return BoundChildTool{}, err
		}
		next := MCPChildBinding(child)
		return BoundChildTool{Tool: child.Tool(), Plugin: child, Close: child.Close, Next: next.Bind}, nil
	}}
}
