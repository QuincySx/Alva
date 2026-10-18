// INPUT: Explicit product directory, provider and approved external executable snapshots
// OUTPUT: Shared bootstrap.Open Application assembly and owned shutdown for desktop/server
// POS: [L7 Hub product] One Hub product recipe above engine adapters; no UI dependency or startup execution
package bootstrap

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/apps/gateway"
	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	productstore "github.com/QuincySx/Alva/go/apps/hub/sqlite"
	"github.com/QuincySx/Alva/go/apps/hub/worktree"
	browserextension "github.com/QuincySx/Alva/go/extension/browser"
	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/extension/hooks"
	"github.com/QuincySx/Alva/go/extension/loader"
	managedmcp "github.com/QuincySx/Alva/go/extension/mcp"
	mediaextension "github.com/QuincySx/Alva/go/extension/media"
	memoryplugin "github.com/QuincySx/Alva/go/extension/memory"
	"github.com/QuincySx/Alva/go/extension/shell"
	skillextension "github.com/QuincySx/Alva/go/extension/skill"
	"github.com/QuincySx/Alva/go/extension/subagent"
	"github.com/QuincySx/Alva/go/extension/tooling"
	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/integration/engine/alva"
	vendorengine "github.com/QuincySx/Alva/go/integration/engine/process"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	mcpprotocol "github.com/QuincySx/Alva/go/integration/protocol/mcp"
	"github.com/QuincySx/Alva/go/integration/protocol/skill"
	"github.com/QuincySx/Alva/go/platform/host"
	browserhost "github.com/QuincySx/Alva/go/platform/host/browser"
	memorysqlite "github.com/QuincySx/Alva/go/platform/storage/memory"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	private "github.com/QuincySx/Alva/go/platform/storage/sqlite"
	nativemedia "github.com/QuincySx/Alva/go/platform/system/media"
	ownedprocess "github.com/QuincySx/Alva/go/platform/system/process"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/memory"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

type ExternalEngine struct {
	Vendor             string
	Command            string
	PrefixArgs         []string
	Env                map[string]string
	ApprovedHostAccess bool
}
type Config struct {
	DataDirectory string
	Demo          bool
	ProviderKind  provider.Kind
	Provider      provider.Config
	// ImageCapable is an operator declaration for exactly ProviderKind/Model.
	// No protocol, model name or external route implies this capability.
	ImageCapable          bool
	External              []ExternalEngine
	GitExecutable         string
	GitHostAccessApproved bool
}
type Application struct {
	Store      *productstore.Store
	Repository *session.Repository
	Runtime    *hub.Runtime
	Approvals  *host.ApprovalBroker
	Questions  *host.QuestionBroker
	Shells     *shell.Sessions
	Targets    []hub.Target
	Service    *product.HubService
	external   []*vendorengine.Adapter
	closeMu    sync.Mutex
	closed     bool
	closeErr   error
	memoryMu   sync.Mutex
	memory     *memorysqlite.Store
	pendingMu  sync.Mutex
	pending    map[*ownedRunner]struct{}
}

// Open reads only the explicit new Go product databases. It creates no session,
// executes no model/CLI/MCP process, and discovers no ambient account material.
func Open(ctx context.Context, config Config) (_ *Application, result error) {
	if config.DataDirectory == "" {
		return nil, errors.New("explicit data directory required")
	}
	if len(config.External) > 3 {
		return nil, errors.New("at most three configured external engines")
	}
	if config.Demo && config.ImageCapable {
		return nil, errors.New("demo model does not claim image capability; operator declaration requires an explicit live route")
	}
	if !config.Demo {
		if config.ProviderKind == "" {
			return nil, errors.New("explicit provider protocol required for live mode")
		}
		if _, err := provider.Build(config.ProviderKind, config.Provider); err != nil {
			return nil, err
		}
	} else if config.Provider.Endpoint != "" || config.Provider.Model != "" || config.Provider.APIKey != "" || len(config.Provider.Headers) > 0 {
		return nil, errors.New("provider configuration cannot be silently ignored in demo mode")
	}
	headers := make(map[string]string, len(config.Provider.Headers))
	for k, v := range config.Provider.Headers {
		headers[k] = v
	}
	config.Provider.Headers = headers
	seen := make(map[string]bool)
	for _, entry := range config.External {
		if !entry.ApprovedHostAccess || entry.Command == "" {
			return nil, errors.New("explicit external executable and host/account access approval required")
		}
		if (entry.Vendor != "claude" && entry.Vendor != "codex" && entry.Vendor != "pi") || seen[entry.Vendor] {
			return nil, errors.New("unique supported external engine identity required")
		}
		seen[entry.Vendor] = true
	}
	dir, err := private.EnsurePrivateDir(config.DataDirectory)
	if err != nil {
		return nil, err
	}
	a := &Application{}
	defer func() {
		if result != nil {
			cleanup, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			result = errors.Join(result, a.Close(cleanup))
		}
	}()
	store, err := productstore.Open(ctx, filepath.Join(dir, "product"))
	if err != nil {
		return nil, err
	}
	a.Store = store
	repository, err := session.Open(ctx, filepath.Join(dir, "native"))
	if err != nil {
		return nil, err
	}
	a.Repository = repository
	a.Shells = shell.NewSessions()
	runtime := hub.New(store)
	a.Runtime = runtime
	approvals := host.NewApprovalBroker(32)
	a.Approvals = approvals
	a.Questions, err = host.NewQuestionBroker(32)
	if err != nil {
		return nil, err
	}
	toolLocks, err := core.NewToolLockRegistry(128)
	if err != nil {
		return nil, err
	}
	adapter := alva.NewConfigured(func(ctx context.Context, nativeID string, request engine.Request, resume bool) (_ harness.Runner, factoryErr error) {
		a.pendingMu.Lock()
		var previous []*ownedRunner
		for resource := range a.pending {
			previous = append(previous, resource)
		}
		a.pendingMu.Unlock()
		for _, resource := range previous {
			if resource.ownerID == request.OwnerID && !resource.CleanupComplete() {
				return nil, errors.New("previous native resources are not physically cleaned up; close/retry cleanup before another Turn")
			}
		}
		owned := &ownedRunner{nativeID: nativeID, ownerID: request.OwnerID}
		owned.onClosed = func() { a.pendingMu.Lock(); delete(a.pending, owned); a.pendingMu.Unlock() }
		a.pendingMu.Lock()
		if a.pending == nil {
			a.pending = make(map[*ownedRunner]struct{})
		}
		a.pending[owned] = struct{}{}
		a.pendingMu.Unlock()
		var loaded []attemptResource
		var workspace *builtin.Workspace
		var skillFiles *skill.FS
		defer func() {
			if factoryErr != nil {
				owned.workspace, owned.skills, owned.resources = workspace, skillFiles, loaded
				factoryErr = errors.Join(factoryErr, owned.Close(context.WithoutCancel(ctx)))
			}
		}()
		// A persisted route is a factual claim, not a label. Refuse a different
		// model/provider/account/billing route instead of silently using this closure.
		expected := hub.Target{ModelID: "local-demo", ProviderID: "local", AccountSource: "local-fixture", BillingRoute: "no-billing"}
		if !config.Demo {
			expected.ModelID = config.Provider.Model
			expected.ProviderID = string(config.ProviderKind)
			expected.AccountSource = "explicit-stdin-or-unauthenticated"
			expected.BillingRoute = config.Provider.Endpoint
		}
		if request.Model != expected.ModelID || request.Provider != expected.ProviderID || request.AccountSource != expected.AccountSource || request.BillingRoute != expected.BillingRoute {
			return nil, errors.New("saved Alva route differs from explicit process configuration; choose the configured route or start a new conversation")
		}
		var log *session.Session
		var err error
		if resume {
			log, err = repository.OpenSession(ctx, request.OwnerID, nativeID)
		} else {
			log, err = repository.Create(ctx, request.OwnerID, nativeID, "")
		}
		if err != nil {
			return nil, err
		}
		plugins, err := harness.NativeSessionPlugins(ctx, repository, request.OwnerID, nativeID)
		if err != nil {
			return nil, err
		}
		builder := harness.NewDemoBuilder(log)
		if !config.Demo {
			model, buildErr := provider.Build(config.ProviderKind, config.Provider)
			err = buildErr
			if err != nil {
				return nil, err
			}
			builder = harness.NewBuilder(model, log)
		}
		builder.Workspace(request.Workspace)
		if request.SystemPrompt != "" {
			builder.SystemPrompt(request.SystemPrompt)
		}
		if request.MaxTurns != 0 {
			builder.MaxIterations(request.MaxTurns)
		}
		builder.Plugin(product.SharedLocksPlugin{Registry: toolLocks})
		workspace, err = builtin.OpenWorkspace(request.Workspace)
		if err != nil {
			return nil, err
		}
		guard, err := security.NewGuard(request.Workspace, nil, workspace, security.NewPermissionManager(security.Ask, approvals))
		if err != nil {
			return nil, err
		}
		backend, err := repository.BoundReferences(ctx, request.OwnerID, nativeID)
		if err != nil {
			return nil, err
		}
		tools := builtin.FileTools(workspace, guard, backend, nativeID)
		history := &product.HistoryTool{Store: store, ProductID: request.OwnerID, NativeID: nativeID}
		builder.Plugin(product.HistoryPlugin{Tool: history})
		trustedTools := append(append(append([]abi.Tool(nil), builder.DeclaredTools()...), tools...), history)
		teamTool := &product.TeamTool{Service: a.Service, ProductID: request.OwnerID, NativeID: nativeID, Guard: guard}
		builder.Plugin(product.TeamPlugin{Tool: teamTool})
		trustedTools = append(trustedTools, teamTool)
		settings, err := store.Settings(ctx, request.OwnerID)
		if err != nil {
			return nil, err
		}
		if settings.MediaEnabled {
			var processor *nativemedia.Processor
			if settings.MediaDecoder.Enabled {
				if !settings.MediaDecoder.HostAccessApproved {
					return nil, errors.New("media decoder snapshot lacks explicit host approval")
				}
				decoderDirectory, err := private.EnsurePrivateDir(filepath.Join(dir, "media-decoder"))
				if err != nil {
					return nil, err
				}
				processor, err = nativemedia.NewProcessor(nativemedia.ProcessorConfig{FFmpeg: settings.MediaDecoder.FFmpeg, FFprobe: settings.MediaDecoder.FFprobe, TempDirectory: decoderDirectory})
				if err != nil {
					return nil, err
				}
			}
			plugin, err := mediaextension.New(ctx, mediaextension.Config{Repository: repository, Owner: request.OwnerID, SessionID: nativeID, Cwd: request.Workspace, Route: hub.RouteKey(hub.Target{EngineID: "alva", ModelID: request.Model, ProviderID: request.Provider, AccountSource: request.AccountSource, BillingRoute: request.BillingRoute}), Guard: guard, ImageCapable: config.ImageCapable, ProviderID: request.Provider, ModelID: request.Model, Processor: processor})
			if err != nil {
				return nil, err
			}
			loaded = append(loaded, plugin)
			plugins = append(plugins, plugin)
			trustedTools = append(trustedTools, plugin.Tool())
		}
		reportCommand := func(ctx context.Context, sessionID, id, name string) error {
			if err := ctx.Err(); err != nil {
				return err
			}
			if sessionID != nativeID || request.ReportActivity == nil {
				return errors.New("native command requires actual session and durable attempt activity admission")
			}
			return request.ReportActivity(engine.Activity{ID: id, Name: name, Effect: engine.EffectUnknown})
		}
		if err = a.optionalTools(ctx, request.OwnerID, nativeID, request.Workspace, settings.Tools, workspace, guard, toolLocks, reportCommand, request.RetainWorkspace, &plugins, &trustedTools, &loaded); err != nil {
			return nil, err
		}
		if settings.Browser.Enabled {
			browserDirectory, err := private.EnsurePrivateDir(filepath.Join(dir, "browser-profiles"))
			if err != nil {
				return nil, err
			}
			launch := settings.Browser
			plugin, err := browserextension.New(ctx, browserextension.Config{Repository: repository, Owner: request.OwnerID, SessionID: nativeID, Cwd: request.Workspace, Guard: guard, ApprovedHostAccess: launch.HostAccessApproved, OperationTimeout: time.Duration(launch.OperationTimeoutMS) * time.Millisecond, Launch: browserhost.Config{Executable: launch.Executable, Cwd: request.Workspace, TempRoot: browserDirectory, Env: launch.Env, Headless: launch.Headless, Proxy: launch.Proxy, StartupTimeout: time.Duration(launch.StartupTimeoutMS) * time.Millisecond}, Report: func(ctx context.Context, a browserextension.Activity) error {
				return reportCommand(ctx, a.SessionID, a.ID, a.Name)
			}})
			if err != nil {
				return nil, err
			}
			loaded = append(loaded, plugin)
			plugins = append(plugins, plugin)
			trustedTools = append(trustedTools, plugin.Tools()...)
		}
		var hookEntries []hooks.Entry
		for _, entry := range settings.Commands.Hooks {
			if entry.Enabled {
				hookEntries = append(hookEntries, hooks.Entry{ID: entry.ID, Event: hooks.Event(entry.Event), Matcher: entry.Matcher, Process: ownedprocess.Config{Command: entry.Command, Args: entry.Args, Env: entry.Env, Cwd: request.Workspace}, Timeout: time.Duration(entry.TimeoutMS) * time.Millisecond, ApprovedHostAccess: entry.HostAccessApproved})
			}
		}
		if len(hookEntries) > 0 {
			plugin, err := hooks.New(ctx, hooks.Config{Repository: repository, Owner: request.OwnerID, SessionID: nativeID, Cwd: request.Workspace, Entries: hookEntries, Locks: toolLocks, Report: func(ctx context.Context, a hooks.Activity) error {
				return reportCommand(ctx, a.SessionID, a.ID, a.Name)
			}})
			if err != nil {
				return nil, err
			}
			loaded = append(loaded, plugin)
			plugins = append(plugins, plugin)
		}
		var diagnosticProfiles []tooling.Profile
		for _, profile := range settings.Commands.Diagnostics {
			if profile.Enabled {
				diagnosticProfiles = append(diagnosticProfiles, tooling.Profile{ID: profile.ID, Description: profile.Description, Process: ownedprocess.Config{Command: profile.Command, Args: profile.Args, Env: profile.Env, Cwd: request.Workspace}, Timeout: time.Duration(profile.TimeoutMS) * time.Millisecond, ApprovedHostAccess: profile.HostAccessApproved})
			}
		}
		if len(diagnosticProfiles) > 0 {
			plugin, err := tooling.New(ctx, tooling.Config{Repository: repository, Owner: request.OwnerID, SessionID: nativeID, Cwd: request.Workspace, Guard: guard, Profiles: diagnosticProfiles, Report: func(ctx context.Context, a tooling.Activity) error {
				return reportCommand(ctx, a.SessionID, a.ID, a.Name)
			}})
			if err != nil {
				return nil, err
			}
			plugins = append(plugins, plugin)
			trustedTools = append(trustedTools, plugin.Tool())
		}
		var servers []mcpprotocol.Config
		childBindings := harness.NativeFileBindings(tools, workspace)
		if settings.PersistentMemory {
			memoryBackend, err := a.workspaceMemory(ctx, request.Workspace)
			if err != nil {
				return nil, err
			}
			plugin, err := memoryplugin.New(memoryplugin.Config{SessionID: nativeID, Service: memory.NewService(memoryBackend, memory.NoopEmbeddingProvider{}), References: backend, Guard: guard, Workspace: workspace, Cwd: request.Workspace})
			if err != nil {
				return nil, err
			}
			plugins = append(plugins, plugin)
			trustedTools = append(trustedTools, plugin.Tools()...)
		}
		if settings.SkillsEnabled {
			skillFiles, err = skill.OpenWithin(request.Workspace, ".alva/skills", false)
			if err != nil {
				return nil, err
			}
			plugin := skillextension.New(skillFiles, nativeID, backend)
			plugins = append(plugins, plugin)
			trustedTools = append(trustedTools, plugin.Tools()...)
		}
		for _, server := range settings.MCP {
			if server.Enabled && server.HostAccessApproved {
				servers = append(servers, mcpprotocol.Config{ID: server.ID, Process: ownedprocess.Config{Command: server.Command, Args: server.Args, Env: server.Env, Cwd: request.Workspace}})
			}
		}
		if len(servers) > 0 {
			mcp, err := managedmcp.New(ctx, managedmcp.Config{Repository: repository, Owner: request.OwnerID, NativeID: nativeID, Guard: guard, Servers: servers})
			if err != nil {
				return nil, err
			}
			plugins = append(plugins, mcp)
			loaded = append(loaded, mcp)
			trustedTools = append(trustedTools, mcp.Tool())
			childBindings = append(childBindings, harness.MCPChildBinding(mcp))
		}
		if settings.SubagentsEnabled {
			factory, childErr := harness.NewNativeChildFactory(ctx, harness.NativeChildConfig{Repository: repository, Owner: request.OwnerID, ParentID: nativeID, Cwd: request.Workspace, Guard: guard, Bindings: childBindings, Locks: toolLocks, ToolTimeout: 30 * time.Second})
			if childErr == nil {
				var plugin *subagent.Plugin
				plugin, childErr = subagent.New(subagent.Config{SessionID: nativeID, Factory: factory, Guard: guard, References: backend, Timeout: 2 * time.Minute, MaxIterations: 16, MaxDepth: 3, MaxChildren: 8, MaxParallelModels: 2})
				if childErr == nil {
					plugins = append(plugins, plugin)
					trustedTools = append(trustedTools, plugin.Tool())
				}
			}
			if childErr != nil {
				return nil, childErr
			}
		}
		for _, entry := range settings.AEP {
			if !entry.Enabled {
				continue
			}
			parent, err := private.EnsurePrivateDir(filepath.Join(dir, "aep-snapshots"))
			if err != nil {
				return nil, err
			}
			bundle, err := product.StagePinnedAEP(ctx, entry, parent)
			if err != nil {
				return nil, err
			}
			plugin, err := loader.OpenPlugin(ctx, loader.PluginConfig{Bundle: bundle, Interpreter: entry.Interpreter, InterpreterArgs: entry.InterpreterArgs, Env: entry.Env, Workspace: request.Workspace, Repository: repository, Owner: request.OwnerID, NativeID: nativeID, Guard: guard, ApprovedHostAccess: entry.HostAccessApproved, BeforeLaunch: func(ctx context.Context) error {
				if err := ctx.Err(); err != nil {
					return err
				}
				if request.ReportActivity == nil {
					return errors.New("native AEP requires a durable actual-attempt activity boundary")
				}
				return request.ReportActivity(engine.Activity{ID: "aep:" + entry.SHA256, Name: "Approved AEP " + entry.Name, Effect: engine.EffectUnknown})
			}})
			if err != nil {
				return nil, errors.Join(err, nativeCloseFailure(nativeID, "close rejected AEP snapshot", bundle.Close()))
			}
			loaded = append(loaded, plugin)
			plugins = append(plugins, plugin)
			trustedTools = append(trustedTools, plugin.Tool())
		}
		for _, plugin := range plugins {
			builder.Plugin(plugin)
		}
		builder.Plugin(harness.NewSecurityPlugin(guard, trustedTools, plugins...))
		for _, tool := range tools {
			builder.Tool(tool)
		}
		runner, err := builder.Build(ctx)
		if err != nil {
			return nil, err
		}
		owned.Runner, owned.workspace, owned.skills, owned.resources = runner, workspace, skillFiles, loaded
		return owned, nil
	}, func(ctx context.Context, nativeID, requestID string, decision engine.PermissionDecision) error {
		if a.Service == nil {
			return errors.New("product permission service unavailable")
		}
		return a.Service.RespondNativeApproval(ctx, "alva", nativeID, requestID, decision.Allow)
	}, alva.FactoryOptions{SystemPrompt: true, MaxTurns: true})
	inspector, err := alva.NewRepositoryInspector(repository)
	if err != nil {
		return nil, err
	}
	descriptor := adapter.Descriptor()
	descriptor.Inspector = inspector
	if !config.Demo {
		descriptor.Config.ProviderProtocol = string(config.ProviderKind)
		descriptor.Config.SupportsCustomHTTPHeaders = true
	}
	if err = runtime.RegisterDescriptor("alva", descriptor); err != nil {
		return nil, err
	}
	target := hub.Target{EngineID: "alva", ModelID: "local-demo", ProviderID: "local", AccountSource: "local-fixture", BillingRoute: "no-billing"}
	if !config.Demo {
		target.ModelID = config.Provider.Model
		target.ProviderID = string(config.ProviderKind)
		target.AccountSource = "explicit-stdin-or-unauthenticated"
		target.BillingRoute = config.Provider.Endpoint
	}
	service, err := product.NewHubService(store, runtime, repository, target, dir, config.Demo)
	if err != nil {
		return nil, err
	}
	service.Approvals = approvals
	service.Questions = a.Questions
	if !config.Demo {
		service.Gateway, err = gateway.NewLocal(map[string]gateway.Route{"alva": {Kind: config.ProviderKind, Provider: config.Provider}})
		if err != nil {
			return nil, err
		}
	}
	for _, entry := range config.External {
		adapter, err := vendorengine.New(vendorengine.Config{Vendor: entry.Vendor, Command: entry.Command, PrefixArgs: entry.PrefixArgs, Env: entry.Env, StateDirectory: filepath.Join(dir, "engines", entry.Vendor), ApprovedHostAccess: entry.ApprovedHostAccess})
		if err != nil {
			return nil, err
		}
		a.external = append(a.external, adapter)
		if err = runtime.RegisterDescriptor(entry.Vendor, adapter.Descriptor()); err != nil {
			return nil, err
		}
		service.Targets = append(service.Targets, hub.Target{EngineID: entry.Vendor, ModelID: "engine-default", ProviderID: "engine-default", AccountSource: "explicit executable; private HOME", BillingRoute: "external runtime account"})
	}
	if config.GitExecutable != "" || config.GitHostAccessApproved {
		service.Worktrees, err = worktree.New(worktree.Config{GitExecutable: config.GitExecutable, Root: filepath.Join(dir, "managed-workers"), ApprovedHostAccess: config.GitHostAccessApproved})
		if err != nil {
			return nil, err
		}
	}
	a.Targets = append([]hub.Target(nil), service.Targets...)
	a.Service = service
	service.Shells = a.Shells
	return a, nil
}

// Close never closes a fact store underneath still-running owned execution. A
// deadline leaves the application intact for a later explicit Close retry.
func (a *Application) Close(ctx context.Context) error {
	a.closeMu.Lock()
	defer a.closeMu.Unlock()
	if a.closed {
		return a.closeErr
	}
	if a.Service != nil && a.Service.Gateway != nil {
		if err := a.Service.Gateway.Close(ctx); err != nil {
			return err
		}
	}
	if a.Service != nil && a.Service.Automations != nil {
		if err := a.Service.Automations.Close(ctx); err != nil {
			return err
		}
	}
	if a.Service != nil && a.Service.Teams != nil {
		if err := a.Service.Teams.Close(ctx); err != nil {
			return err
		}
	}
	if a.Runtime != nil {
		if err := a.Runtime.Close(ctx); err != nil {
			return err
		}
	}
	var shellCloseErr error
	if a.Shells != nil {
		shellCloseErr = a.Shells.Close(ctx)
		if !a.Shells.CleanupComplete() {
			return errors.Join(shellCloseErr, errors.New("session shell physical cleanup incomplete; fact stores retained"))
		}
	}
	if a.Service != nil && a.Service.Worktrees != nil {
		if err := a.Service.Worktrees.Close(ctx); err != nil {
			return err
		}
		if !a.Service.Worktrees.CleanupComplete() {
			return errors.New("managed Git physical cleanup incomplete; stores retained")
		}
	}
	for _, adapter := range a.external {
		if err := adapter.Close(ctx); err != nil {
			return err
		}
	}
	var err = shellCloseErr
	// Failed early observation/cleanup can outlive the execution's terminal
	// status. The application still owns those exact resources, and never
	// closes native facts underneath pending Plugin I/O workers.
	a.pendingMu.Lock()
	var pending []*ownedRunner
	for runner := range a.pending {
		pending = append(pending, runner)
	}
	a.pendingMu.Unlock()
	unfinished := false
	for _, runner := range pending {
		err = errors.Join(err, runner.Close(ctx))
		unfinished = unfinished || !runner.CleanupComplete()
	}
	if unfinished {
		return errors.Join(err, errors.New("native runner resource cleanup incomplete; stores remain open for Close retry"))
	}
	if a.Approvals != nil {
		err = errors.Join(err, a.Approvals.Close())
	}
	if a.Questions != nil {
		a.Questions.Close()
	}
	a.memoryMu.Lock()
	if a.memory != nil {
		err = errors.Join(err, a.memory.Close())
	}
	a.memoryMu.Unlock()
	if a.Repository != nil {
		err = errors.Join(err, a.Repository.Close())
	}
	if a.Store != nil {
		err = errors.Join(err, a.Store.Close())
	}
	a.closed = true
	a.closeErr = err
	return err
}
