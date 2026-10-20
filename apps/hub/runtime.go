// INPUT: context, errors, sync, engine and the durable product Store
// OUTPUT: Runtime registry, serial attempts, cancellation, permission routing and authorized fallback
// POS: [L7 Hub product] Product supervisor without SDK, concrete adapters or native fact storage dependencies
package hub

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"sync"

	"github.com/QuincySx/Alva/integration/engine"
)

type activeRun struct {
	control   sync.Mutex
	done      chan error
	exited    chan struct{}
	cancel    context.CancelFunc
	engine    engine.Runtime
	nativeID  string
	attemptID string
}

var ErrSessionNotRunning = errors.New("Hub session is not running")
var ErrSessionBusy = errors.New("Hub session already running")

type Runtime struct {
	store       ExecutionStore
	mu          sync.Mutex
	engines     map[string]engine.Runtime
	descriptors map[string]engine.Descriptor
	active      map[string]*activeRun
	closed      bool
	workspaces  *workspaceRegistry
	actions     map[*workspaceAction]struct{}
}

func New(store ExecutionStore) *Runtime {
	return &Runtime{store: store, engines: make(map[string]engine.Runtime), descriptors: make(map[string]engine.Descriptor), active: make(map[string]*activeRun), workspaces: newWorkspaceRegistry()}
}

// HasActive reports execution ownership, not whether any UI is observing it.
func (r *Runtime) HasActive() bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return len(r.active) != 0 || len(r.actions) != 0
}
func (r *Runtime) Register(id string, runtime engine.Runtime) error {
	return r.RegisterDescriptor(id, engine.DescribeRuntime(runtime))
}
func (r *Runtime) RegisterDescriptor(id string, descriptor engine.Descriptor) error {
	if err := descriptor.Validate(); err != nil {
		return err
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return errors.New("Hub engine registry is closed")
	}
	if id == "" || len(id) > 128 {
		return errors.New("engine identity and runtime required")
	}
	if _, exists := r.engines[id]; exists {
		return errors.New("engine replacement denied while native bindings may exist")
	}
	r.engines[id] = descriptor.Runtime
	r.descriptors[id] = descriptor.Clone()
	return nil
}

// Descriptor returns a detached declaration, never discovers host readiness.
func (r *Runtime) Descriptor(id string) (engine.Descriptor, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	d, ok := r.descriptors[id]
	return d.Clone(), ok
}
func (r *Runtime) EngineDescriptions() []engine.Description {
	r.mu.Lock()
	defer r.mu.Unlock()
	ids := make([]string, 0, len(r.descriptors))
	for id := range r.descriptors {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	result := make([]engine.Description, 0, len(ids))
	for _, id := range ids {
		result = append(result, r.descriptors[id].Description(id))
	}
	return result
}
func (r *Runtime) Cancel(ctx context.Context, sessionID string) error {
	r.mu.Lock()
	run, ok := r.active[sessionID]
	if !ok {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	r.mu.Unlock()
	run.control.Lock()
	defer run.control.Unlock()
	r.mu.Lock()
	if r.active[sessionID] != run {
		r.mu.Unlock()
		return errors.New("Hub cancellation target is no longer active")
	}
	run.cancel()
	target, id := run.engine, run.nativeID
	r.mu.Unlock()
	if target != nil && id != "" && target.Capabilities().Cancel {
		err := target.Cancel(ctx, id)
		if err != nil && !errors.Is(err, engine.ErrSessionNotFound) {
			return err
		}
	}
	return nil
}
func (r *Runtime) RespondPermission(ctx context.Context, sessionID, requestID string, decision engine.PermissionDecision) error {
	r.mu.Lock()
	run := r.active[sessionID]
	if run == nil {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	r.mu.Unlock()
	run.control.Lock()
	defer run.control.Unlock()
	r.mu.Lock()
	if r.active[sessionID] != run || run.engine == nil || run.nativeID == "" {
		r.mu.Unlock()
		return errors.New("Hub permission target is no longer active")
	}
	target, id := run.engine, run.nativeID
	r.mu.Unlock()
	if !target.Capabilities().PermissionCallback {
		return engine.ErrUnsupported
	}
	return target.RespondPermission(ctx, id, requestID, decision)
}

// Run is owned by the product execution service. UI observers consume committed
// Store.History; their detachment does not own this context or cancel execution.
func (r *Runtime) Run(owner context.Context, sessionID, prompt string) error {
	_, done, err := r.Start(owner, sessionID, prompt)
	if err != nil {
		return err
	}
	return <-done
}

// Start commits admission synchronously, then owns execution independently from
// UI subscriptions. The returned channel is a one-result ownership receipt.
func (r *Runtime) Start(owner context.Context, sessionID, prompt string) (string, <-chan error, error) {
	return r.start(owner, sessionID, func(ctx context.Context) (Claim, error) { return r.store.BeginTurn(ctx, sessionID, prompt) })
}

// StartOnBranch atomically binds reference-only input to its inspected route.
func (r *Runtime) StartOnBranch(owner context.Context, sessionID, branchID, prompt string) (string, <-chan error, error) {
	return r.start(owner, sessionID, func(ctx context.Context) (Claim, error) {
		return r.store.BeginTurnOnBranch(ctx, sessionID, branchID, prompt)
	})
}
func (r *Runtime) start(owner context.Context, sessionID string, admit func(context.Context) (Claim, error)) (string, <-chan error, error) {
	ctx, cancel := context.WithCancel(owner)
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		cancel()
		return "", nil, errors.New("Hub runtime is closed")
	}
	if _, busy := r.active[sessionID]; busy {
		r.mu.Unlock()
		cancel()
		return "", nil, ErrSessionBusy
	}
	if len(r.active) >= 64 {
		r.mu.Unlock()
		cancel()
		return "", nil, errors.New("Hub execution budget exhausted (64 admitted runs)")
	}
	active := &activeRun{cancel: cancel, done: make(chan error, 1), exited: make(chan struct{})}
	r.active[sessionID] = active
	r.mu.Unlock()
	claim, err := admit(ctx)
	if err != nil {
		cancel()
		r.mu.Lock()
		delete(r.active, sessionID)
		r.mu.Unlock()
		active.done <- err
		close(active.done)
		close(active.exited)
		return "", nil, err
	}
	go func() {
		err := r.executeClaim(ctx, active, claim)
		cancel()
		active.control.Lock()
		r.mu.Lock()
		delete(r.active, sessionID)
		r.mu.Unlock()
		active.control.Unlock()
		active.done <- err
		close(active.done)
		close(active.exited)
	}()
	return claim.Turn.ID, active.done, nil
}
func (r *Runtime) Close(ctx context.Context) error {
	r.mu.Lock()
	r.closed = true
	runs := make([]*activeRun, 0, len(r.active))
	for _, run := range r.active {
		run.cancel()
		runs = append(runs, run)
	}
	actions := make([]*workspaceAction, 0, len(r.actions))
	for action := range r.actions {
		action.cancel()
		actions = append(actions, action)
	}
	r.mu.Unlock()
	for _, run := range runs {
		select {
		case <-run.exited:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	for _, action := range actions {
		select {
		case <-action.exited:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return nil
}
func (r *Runtime) executeClaim(ctx context.Context, active *activeRun, claim Claim) error {
	sessionID := claim.Session.ID
	tried := make(map[string]bool)
	for attemptNo := 0; attemptNo < 8; attemptNo++ {
		tried[RouteKey(claim.Attempt.Target)] = true
		outcome, sideEffects, failure, attemptErr := r.attempt(ctx, active, &claim)
		if outcome == engine.Succeeded || outcome == engine.Cancelled || ctx.Err() != nil {
			return errors.Join(attemptErr, r.store.FinishTurn(context.WithoutCancel(ctx), claim.Turn.ID, outcome))
		}
		if claim.Turn.AutomationID != "" || sideEffects || !retryable(failure) {
			return errors.Join(attemptErr, r.store.FinishTurn(context.WithoutCancel(ctx), claim.Turn.ID, outcome))
		}
		detail, detailErr := r.store.Detail(ctx, sessionID)
		if detailErr != nil {
			return errors.Join(attemptErr, detailErr, r.store.FinishTurn(context.WithoutCancel(ctx), claim.Turn.ID, engine.Failed))
		}
		var next *Claim
		for _, target := range detail.Fallback {
			if !target.AutomaticAuthorized || tried[RouteKey(target)] {
				continue
			}
			r.mu.Lock()
			runtime := r.engines[target.EngineID]
			r.mu.Unlock()
			if runtime == nil || !supports(runtime.Capabilities(), target.RequiredCapabilities) {
				continue
			}
			candidate, candidateErr := r.store.BeginFallback(ctx, claim.Attempt.ID, target)
			if candidateErr != nil {
				continue
			}
			next = &candidate
			break
		}
		if next == nil {
			return errors.Join(attemptErr, r.store.FinishTurn(context.WithoutCancel(ctx), claim.Turn.ID, outcome))
		}
		claim = *next
	}
	return errors.Join(errors.New("fallback attempt budget exhausted"), r.store.FinishTurn(context.WithoutCancel(ctx), claim.Turn.ID, engine.Failed))
}
func retryable(kind engine.FailureKind) bool {
	return kind == engine.FailureStartup || kind == engine.FailureRateLimit
}
func supports(c engine.Capabilities, required []string) bool {
	for _, name := range required {
		switch name {
		case "streaming":
			if !c.Streaming {
				return false
			}
		case "resume":
			if !c.Resume {
				return false
			}
		case "cancel":
			if !c.Cancel {
				return false
			}
		case "permission_callback":
			if !c.PermissionCallback {
				return false
			}
		case "tool_control":
			if !c.ToolControl {
				return false
			}
		default:
			return false
		}
	}
	return true
}
func (r *Runtime) attempt(ctx context.Context, active *activeRun, claim *Claim) (engine.Outcome, bool, engine.FailureKind, error) {
	r.mu.Lock()
	target := r.engines[claim.Attempt.Target.EngineID]
	active.engine = target
	active.nativeID = claim.Branch.NativeSessionID
	active.attemptID = claim.Attempt.ID
	r.mu.Unlock()
	contract := engine.StreamContract{}
	outcome := engine.Failed
	failure := engine.FailureExecution
	sideEffects := false
	var fatalMessage string
	open := make(map[string]string)
	var persistenceErr error
	commit := func(event engine.Event) error {
		if event.Kind == engine.Completed || (event.Kind == engine.Error && !event.Recoverable) {
			for id, name := range open {
				cleanup := engine.Event{Kind: engine.ToolEnd, ID: id, Name: name, IsError: true, Text: "Engine ended before a paired tool result; inspect its native journal."}
				next := contract.Clone()
				if err := next.Accept(cleanup); err != nil {
					return err
				}
				if err := r.store.Record(context.WithoutCancel(ctx), claim.Attempt.ID, cleanup, true); err != nil {
					persistenceErr = err
					return err
				}
				contract = next
				delete(open, id)
			}
		}
		next := contract.Clone()
		if err := next.Accept(event); err != nil {
			return err
		}
		activityRisk := event.Kind == engine.ExecutionActivity && event.Activity != nil && event.Activity.Effect != engine.EffectRead && event.Activity.Effect != engine.EffectNone
		possibleEffects := sideEffects || activityRisk || (event.Kind == engine.SessionStarted && target != nil && !target.Capabilities().ToolControl) || (event.Kind == engine.ToolStart && event.Effect != engine.EffectRead)
		if err := r.store.Record(context.WithoutCancel(ctx), claim.Attempt.ID, event, possibleEffects); err != nil {
			persistenceErr = err
			return err
		}
		contract = next
		if event.Kind == engine.ToolStart {
			open[event.ID] = event.Name
		}
		if event.Kind == engine.ToolEnd {
			delete(open, event.ID)
		}
		if event.Kind == engine.SessionStarted || event.Kind == engine.ExecutionActivity && event.SessionID != "" {
			r.mu.Lock()
			active.nativeID = event.SessionID
			r.mu.Unlock()
			if event.Kind == engine.SessionStarted && target != nil && !target.Capabilities().ToolControl {
				sideEffects = true
			}
		}
		if event.Kind == engine.ToolStart && event.Effect != engine.EffectRead {
			sideEffects = true
		}
		if activityRisk {
			sideEffects = true
		}
		if event.Kind == engine.Error && !event.Recoverable {
			failure = event.Failure
			fatalMessage = event.Text
		}
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return nil
	}
	// A faulty adapter cannot erase a rejected callback by returning nil or
	// sending Completed later. Serialize callbacks and close admission before
	// inspecting the stream state; late callbacks cannot touch product facts.
	var eventMu sync.Mutex
	var eventErr error
	accepting := true
	observe := func(event engine.Event) error {
		eventMu.Lock()
		defer eventMu.Unlock()
		if !accepting {
			return errors.New("engine callback after Execute returned")
		}
		if eventErr == nil {
			eventErr = commit(event)
		}
		return eventErr
	}
	var executeErr error
	if target == nil {
		executeErr = errors.New("requested engine is not registered")
		failure = engine.FailureStartup
	} else if !supports(target.Capabilities(), claim.Attempt.Target.RequiredCapabilities) {
		executeErr = fmt.Errorf("%w: required engine capabilities", engine.ErrUnsupported)
		failure = engine.FailureStartup
	} else if claim.Branch.NativeSessionID != "" && !target.Capabilities().Resume {
		// Never silently launch a fresh native conversation under a retained
		// branch. That would lose its context and overwrite native authority.
		executeErr = fmt.Errorf("%w: this engine cannot resume the retained native branch", engine.ErrUnsupported)
	} else {
		release, admissionErr := r.workspaces.acquire(ctx, claim.Session.Workspace, !target.Capabilities().ToolControl)
		if admissionErr != nil {
			executeErr = admissionErr
		} else {
			seal, retain := retainableWorkspace(release)
			defer seal()
			handoff, handoffErr := EncodeHandoff(claim.Handoff)
			if handoffErr != nil {
				executeErr = handoffErr
			} else {
				resume := claim.Branch.NativeSessionID
				if !target.Capabilities().Resume {
					resume = ""
				}
				request := engine.Request{OwnerID: claim.Session.ID, Prompt: handoff + claim.Turn.Prompt, ResumeSession: resume, Workspace: claim.Session.Workspace, Model: claim.Attempt.Target.ModelID, Provider: claim.Attempt.Target.ProviderID, AccountSource: claim.Attempt.Target.AccountSource, BillingRoute: claim.Attempt.Target.BillingRoute}
				if target.Capabilities().ToolControl {
					request.RetainWorkspace = retain
				}
				executeErr = target.Execute(ctx, request, observe)
				seal()
			}
		}
	}
	eventMu.Lock()
	accepting = false
	observedErr := eventErr
	eventMu.Unlock()
	if observedErr != nil {
		return engine.Failed, true, engine.FailureProtocol, errors.Join(executeErr, observedErr)
	}
	if persistenceErr != nil {
		// A failed durable observation never becomes success or safe-to-retry.
		// FinishTurn will mark an uncommitted terminal attempt failed.
		return engine.Failed, true, engine.FailureExecution, errors.Join(executeErr, persistenceErr)
	}
	if contract.Terminal && executeErr != nil {
		// The event stream cannot authorize success after its actual owner
		// reports a cleanup/transport failure. Keep the original terminal
		// observation, fail the product Turn, and forbid automatic replay.
		return engine.Failed, true, engine.FailureExecution, executeErr
	}
	if !contract.Terminal {
		if executeErr == nil {
			executeErr = errors.New("engine ended without Completed")
			failure = engine.FailureProtocol
		}
		if ctx.Err() != nil && cancellationOnly(executeErr) {
			outcome = engine.Cancelled
			failure = engine.FailureCancelled
		} else {
			outcome = engine.Failed
		}
		if !contract.Fatal {
			if err := commit(engine.Event{Kind: engine.Error, Text: boundedError(executeErr), Failure: failure}); err != nil {
				return outcome, true, failure, errors.Join(executeErr, err)
			}
		}
		if err := commit(engine.Event{Kind: engine.Completed, Outcome: outcome}); err != nil {
			return outcome, true, failure, errors.Join(executeErr, err)
		}
	}
	if outcome != engine.Succeeded && executeErr == nil {
		executeErr = errors.New(fatalMessage)
		if fatalMessage == "" {
			executeErr = errors.New("engine attempt failed")
		}
	}
	return outcome, sideEffects, failure, executeErr
}
func cancellationOnly(err error) bool {
	if err == nil {
		return false
	}
	if err == context.Canceled || err == context.DeadlineExceeded {
		return true
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !cancellationOnly(child) {
				return false
			}
		}
		return true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return cancellationOnly(wrapped.Unwrap())
	}
	return false
}
func boundedError(err error) string {
	if err == nil {
		return "engine failed"
	}
	text := err.Error()
	if len(text) > 4096 {
		return "engine error exceeds display budget"
	}
	return text
}
