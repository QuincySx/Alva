// INPUT: Explicit native CLI recipe, supplied storage, workspace capabilities and optional durable controller.
// OUTPUT: Per-turn Agent/plugin assembly and a process owner that joins execution before closing shared resources.
// POS: [L6 application] Keeps remote plugins fresh per turn, claims before startup and retains failed cleanup ownership.
package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sync"
	"time"

	filetools "github.com/QuincySx/Alva/extension/files"
	memoryplugin "github.com/QuincySx/Alva/extension/memory"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/harness/durable"
	durablejournal "github.com/QuincySx/Alva/harness/durable/journal"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/platform/storage"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/memory"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type cliResource interface {
	Close(context.Context) error
	CleanupComplete() bool
}
type cliNativeConfig struct {
	Model                    abi.LanguageModel
	Repository               storage.Repository
	Native                   storage.Session
	DataDirectory, Workspace string
	Mode                     security.PermissionMode
	Memory                   bool
	OpenMemory               func(context.Context) (storage.MemoryStore, error)
	Approval                 security.ApprovalHandler
	Durable                  bool
	Resume                   bool
	RouteHash                string
	CompatibilityFile        string
	Diagnostics              io.Writer
}
type cliNativeRuntime struct {
	config          cliNativeConfig
	log             abi.ConversationLog
	workspace       string
	files           *workspacefs.Workspace
	memory          storage.MemoryStore
	memoryPlugin    core.Plugin
	references      agentcontext.ReferenceBackend
	mu              sync.Mutex
	run             sync.Mutex
	cleanup         sync.Mutex
	closeGate       chan struct{}
	cancel          context.CancelFunc
	done            chan struct{}
	closed, cleaned bool
	resources       []cliResource
	release         func()
	launchSequence  uint64
	outcome         durable.Outcome
	resume          bool
}

func newCLINative(ctx context.Context, config cliNativeConfig) (*cliNativeRuntime, error) {
	r := &cliNativeRuntime{config: config, resume: config.Resume, memoryPlugin: harness.NewMemoryPlugin(), closeGate: make(chan struct{}, 1)}
	if config.Native != nil {
		r.log = config.Native
	} else {
		r.log = core.NewMemorySession("cli-volatile")
	}
	workspace := config.Workspace
	if workspace == "" {
		var err error
		workspace, err = os.Getwd()
		if err != nil {
			return nil, err
		}
	}
	absolute, err := filepath.Abs(workspace)
	if err != nil {
		return nil, err
	}
	r.workspace = absolute
	if config.Native == nil {
		r.references, err = agentcontext.NewMemoryReferences(r.log.ID(), 20<<20)
		if err != nil {
			return r, err
		}
	}
	if config.Model != nil && config.Workspace != "" {
		r.files, err = workspacefs.OpenWorkspace(absolute)
		if err != nil {
			return r, err
		}
	}
	if config.Memory {
		if config.OpenMemory == nil {
			return r, errors.New("explicit memory storage recipe required")
		}
		r.memory, err = config.OpenMemory(ctx)
		if err != nil {
			return r, err
		}
	}
	return r, nil
}

func (r *cliNativeRuntime) retain(resource cliResource) { r.resources = append(r.resources, resource) }
func (r *cliNativeRuntime) build(ctx context.Context) (*core.Agent, *durable.Controller, error) {
	var waits *durable.Controller
	var selected cliPluginSettings
	compatibility, err := r.compatibility(ctx)
	if err != nil {
		return nil, nil, err
	}
	compatibilitySHA := ""
	if compatibility != nil {
		compatibilitySHA = compatibility.Fingerprint()
	}
	log := r.log
	if r.config.Native != nil {
		_, release, err := r.config.Repository.BeginRun(ctx, cliOwner, log.ID())
		if err != nil {
			return nil, nil, err
		}
		r.release = release
		selected, _, err = loadCLIPlugins(ctx, r.config.Repository, r.config.Native)
		if err != nil {
			return nil, nil, err
		}
		backend, err := durablejournal.Open(ctx, r.config.Repository, cliOwner, log.ID())
		if err != nil {
			return nil, nil, err
		}
		snapshot, err := backend.Load(ctx)
		if err != nil {
			return nil, nil, err
		}
		pending, err := cliPendingLaunch(ctx, r.config.Repository, r.config.Native)
		if err != nil {
			return nil, nil, err
		}
		if pending != 0 && snapshot.State.Status != durable.Running && snapshot.State.Status != durable.Interrupted {
			state := snapshot.State
			state.Status = durable.Interrupted
			if _, err = backend.Commit(ctx, snapshot.Revision, state, nil); err != nil {
				return nil, nil, err
			}
			return nil, nil, durable.ErrInterrupted
		}
		if snapshot.State.Status == durable.Running || snapshot.State.Status == durable.Interrupted {
			return nil, nil, durable.ErrInterrupted
		}
		if snapshot.State.Status == durable.Waiting && !r.resume {
			return nil, nil, errors.New("waiting checkpoint requires explicit --resume-wait; new input was not submitted")
		}
		if r.resume && snapshot.State.Status != durable.Waiting {
			return nil, nil, errors.New("--resume-wait requires a waiting checkpoint")
		}
		if r.config.Durable {
			recipe, err := cliRecipeHash(r.config, r.workspace, selected, compatibilitySHA)
			if err != nil {
				return nil, nil, err
			}
			if err = bindCLIDurableRecipe(ctx, r.config.Repository, r.config.Native, recipe, r.resume); err != nil {
				return nil, nil, err
			}
			// Inspecting an unresolved/expired wait never launches plugin code.
			if r.resume {
				for _, request := range snapshot.State.Requests {
					if !time.Now().Before(request.ExpiresAt) {
						return nil, nil, durable.ErrExpired
					}
					if len(request.Result) == 0 {
						r.mu.Lock()
						r.outcome = durable.Outcome{Suspended: true, Requests: snapshot.State.Requests}
						r.mu.Unlock()
						return nil, nil, nil
					}
				}
			}
		}
	}
	if r.config.Durable {
		backend, err := durablejournal.Open(ctx, r.config.Repository, cliOwner, log.ID())
		if err != nil {
			return nil, nil, err
		}
		// This runner already owns the actual native lease before plugin startup.
		// Controller borrows it; the outer owner releases only after real cleanup.
		waits, err = durable.New(durable.Config{Backend: cliClaimedBackend{Backend: backend, runtime: r}, TTL: 24 * time.Hour})
		if err != nil {
			return nil, nil, err
		}
		log = backend.Log()
	}
	var builder *harness.Builder
	if r.config.Model == nil {
		builder = harness.NewDemoBuilder(log)
	} else {
		builder = harness.NewBuilder(r.config.Model, log)
	}
	builder.Workspace(r.workspace)
	var plugins []core.Plugin
	if r.config.Native != nil {
		var err error
		plugins, err = harness.SessionViews(ctx, r.config.Repository, cliOwner, log.ID())
		if err != nil {
			return nil, nil, err
		}
	}
	approval := r.config.Approval
	if waits != nil {
		approval = waits
	}
	var resolver security.PathResolver
	if r.files != nil {
		resolver = r.files
	}
	guard, err := security.NewGuard(r.workspace, nil, resolver, security.NewPermissionManager(r.config.Mode, approval))
	if err != nil {
		return nil, nil, err
	}
	tools := builder.DeclaredTools()
	var refs agentcontext.ReferenceBackend
	if r.config.Native != nil {
		refs, err = r.config.Repository.BoundReferences(ctx, cliOwner, log.ID())
	} else {
		refs = r.references
		plugins = append(plugins, harness.NewContextRetrievalPlugin(log.ID(), refs))
	}
	if err != nil {
		return nil, nil, err
	}
	var childFileTools []abi.Tool
	if r.files != nil {
		for _, tool := range filetools.FileTools(r.files, guard, refs, log.ID()) {
			builder.Tool(tool)
			tools = append(tools, tool)
			childFileTools = append(childFileTools, tool)
		}
	}
	memoryContribution := r.memoryPlugin
	if r.config.Memory {
		canonical, err := filepath.EvalSymlinks(r.workspace)
		if err != nil {
			return nil, nil, err
		}
		backend, err := r.memory.Bind("cli-workspace-" + cliSHA([]byte(canonical)))
		if err != nil {
			return nil, nil, err
		}
		p, err := memoryplugin.New(memoryplugin.Config{SessionID: log.ID(), Service: memory.NewService(backend, memory.NoopEmbeddingProvider{}), References: refs, Guard: guard, Workspace: r.files, Cwd: r.workspace})
		if err != nil {
			return nil, nil, err
		}
		memoryContribution = p
		tools = append(tools, p.Tools()...)
	}
	plugins = append(plugins, memoryContribution)
	compatPlugins, compatTools, err := r.compatibilityPlugins(ctx, compatibility, refs, guard, childFileTools)
	if err != nil {
		return nil, nil, err
	}
	plugins = append(plugins, compatPlugins...)
	tools = append(tools, compatTools...)
	if waits != nil {
		plugins = append(plugins, waits)
		tools = append(tools, waits.Tool())
	}
	if r.config.Native != nil {
		remote, remoteTools, err := openCLIPlugins(ctx, r.config.Repository, r.config.Native, selected, r.config.DataDirectory, r.workspace, guard, r.retain, func(sequence uint64) { r.launchSequence = sequence }, r.config.Diagnostics)
		if err != nil {
			return nil, nil, err
		}
		plugins = append(plugins, remote...)
		tools = append(tools, remoteTools...)
	}
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	builder.Plugin(harness.NewSecurityPlugin(guard, tools, plugins...))
	agent, err := builder.Build(ctx)
	return agent, waits, err
}

func (r *cliNativeRuntime) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) (err error) {
	if !r.run.TryLock() {
		return core.ErrRunInProgress
	}
	defer r.run.Unlock()
	r.mu.Lock()
	if r.closed || len(r.resources) != 0 || r.release != nil {
		r.mu.Unlock()
		return errors.New("CLI execution closed or prior resources need cleanup")
	}
	ctx, cancel := context.WithCancel(ctx)
	r.cancel, r.done, r.outcome = cancel, make(chan struct{}), durable.Outcome{}
	done := r.done
	r.mu.Unlock()
	defer func() {
		cancel()
		cleanup, stop := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		err = errors.Join(err, r.closeTurn(cleanup))
		stop()
		r.mu.Lock()
		r.cancel = nil
		close(done)
		r.mu.Unlock()
	}()
	agent, waits, err := r.build(ctx)
	if err != nil {
		return err
	}
	if agent == nil {
		return nil
	} // Unresolved waiting checkpoint; nothing was launched.
	if waits == nil {
		return agent.Run(ctx, input, emit)
	}
	var outcome durable.Outcome
	if r.resume {
		outcome, err = waits.Resume(ctx, agent, emit)
	} else {
		outcome, err = waits.Start(ctx, agent, input, emit)
	}
	r.mu.Lock()
	r.outcome = outcome
	if err == nil {
		r.resume = false
	}
	r.mu.Unlock()
	return err
}

func (r *cliNativeRuntime) closeTurn(ctx context.Context) error {
	r.cleanup.Lock()
	defer r.cleanup.Unlock()
	var result error
	complete := true
	for i := len(r.resources) - 1; i >= 0; i-- {
		result = errors.Join(result, r.resources[i].Close(ctx))
		complete = complete && r.resources[i].CleanupComplete()
	}
	if complete {
		r.resources = nil
		if r.launchSequence != 0 {
			if err := settleCLILaunch(ctx, r.config.Repository, r.config.Native, r.launchSequence, "resources_reaped"); err != nil {
				return errors.Join(result, err)
			}
			r.launchSequence = 0
		}
		if r.release != nil {
			r.release()
			r.release = nil
		}
	}
	return result
}
func (r *cliNativeRuntime) Close(ctx context.Context) error {
	select {
	case r.closeGate <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	}
	defer func() { <-r.closeGate }()
	r.mu.Lock()
	r.closed = true
	cancel, done := r.cancel, r.done
	r.mu.Unlock()
	if cancel != nil {
		cancel()
	}
	if done != nil {
		select {
		case <-done:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	err := r.closeTurn(ctx)
	if len(r.resources) != 0 || r.release != nil {
		return errors.Join(err, errors.New("CLI plugin cleanup incomplete"))
	}
	if r.memory != nil {
		if e := r.memory.Close(); e != nil {
			return errors.Join(err, e)
		}
		r.memory = nil
	}
	if r.files != nil {
		if e := r.files.Close(); e != nil {
			return errors.Join(err, e)
		}
		r.files = nil
	}
	r.mu.Lock()
	r.cleaned = true
	r.mu.Unlock()
	return err
}
func (r *cliNativeRuntime) CleanupComplete() bool { r.mu.Lock(); defer r.mu.Unlock(); return r.cleaned }

type cliClaimedBackend struct {
	durable.Backend
	runtime *cliNativeRuntime
}

func (b cliClaimedBackend) Claim(ctx context.Context) (func(), error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if b.runtime.release == nil {
		return nil, errors.New("durable execution lacks its native owner lease")
	}
	return func() {}, nil
}

// Retain the storage authority when execution has not physically joined. A
// caller may retry Close through the error's cleanup interface after timeout.
type cliCleanupError struct {
	cause      error
	runtime    *cliNativeRuntime
	repository storage.Repository
	mu         sync.Mutex
	complete   bool
}

func (e *cliCleanupError) Error() string {
	return "CLI cleanup incomplete; execution and repository remain owned: " + fmt.Sprint(e.cause)
}
func (e *cliCleanupError) Unwrap() error { return e.cause }
func (e *cliCleanupError) Close(ctx context.Context) error {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.complete {
		return nil
	}
	err := e.runtime.Close(ctx)
	if !e.runtime.CleanupComplete() {
		return err
	}
	if e.repository != nil {
		if closeErr := e.repository.Close(); closeErr != nil {
			return errors.Join(err, closeErr)
		}
	}
	e.complete = true
	return err
}
func (e *cliCleanupError) CleanupComplete() bool { e.mu.Lock(); defer e.mu.Unlock(); return e.complete }

func validateCLIWorkerSession(ctx context.Context, repository storage.Repository, log storage.Session) error {
	backend, err := durablejournal.Open(ctx, repository, cliOwner, log.ID())
	if err != nil {
		return err
	}
	snapshot, err := backend.Load(ctx)
	if err != nil {
		return err
	}
	if snapshot.State.Status != durable.Idle {
		return errors.New("inspect the native durable checkpoint before changing execution mode; no automatic replay")
	}
	if pending, err := cliPendingLaunch(ctx, repository, log); err != nil {
		return err
	} else if pending != 0 {
		return durable.ErrInterrupted
	}
	state, _, err := loadCLIPlugins(ctx, repository, log)
	if err != nil {
		return err
	}
	for _, pin := range state.Plugins {
		if pin.Enabled {
			return errors.New("enabled AEP plugins use native CLI ownership; disable them before selecting the whole-agent WASM worker")
		}
	}
	return nil
}
