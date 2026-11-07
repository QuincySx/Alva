// INPUT: context, errors, sync, time, engine and the durable product Store
// OUTPUT: Runtime registry, execution identity, serial attempts with coalesced stream deltas, cancellation/join, permission routing and authorized fallback
// POS: [L7 Hub product] Product supervisor without SDK, concrete adapters or native fact storage dependencies
package hub

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"sync"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
)

type activeRun struct {
	control      sync.Mutex
	done         chan error
	exited       chan struct{}
	cancel       context.CancelFunc
	engine       engine.Runtime
	engineID     string
	executionID  string
	nativeID     string
	attemptID    string
	interactions map[string]*pendingEngineInteraction
}

var ErrSessionNotRunning = errors.New("Hub session is not running")
var ErrSessionBusy = errors.New("Hub session already running")

type Runtime struct {
	store       ExecutionStore
	waits       WaitStore
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
	return r.cancel(ctx, sessionID, "")
}

// CancelExecution cancels only the active owner with the observed execution ID.
func (r *Runtime) CancelExecution(ctx context.Context, sessionID, expectedExecutionID string) error {
	if expectedExecutionID == "" {
		return errors.New("Hub execution identity required")
	}
	return r.cancel(ctx, sessionID, expectedExecutionID)
}

// CancelAndWait captures the current physical owner, including uncommitted
// admission, then cancels and joins that owner only. A timeout retains the
// active owner; a replacement execution is never cancelled or awaited.
func (r *Runtime) CancelAndWait(ctx context.Context, sessionID string) error {
	return r.cancelAndWait(ctx, sessionID, "")
}

// CancelExecutionAndWait joins only the physical owner named by an earlier
// observation. An absent or replaced identity cannot cancel another owner.
func (r *Runtime) CancelExecutionAndWait(ctx context.Context, sessionID, expectedExecutionID string) error {
	if expectedExecutionID == "" {
		return errors.New("Hub execution identity required")
	}
	return r.cancelAndWait(ctx, sessionID, expectedExecutionID)
}

func (r *Runtime) cancelAndWait(ctx context.Context, sessionID, expectedExecutionID string) error {
	r.mu.Lock()
	run := r.active[sessionID]
	if run == nil || expectedExecutionID != "" && run.executionID != expectedExecutionID {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	r.mu.Unlock()
	err := r.cancelRun(ctx, sessionID, expectedExecutionID, run)
	if expectedExecutionID == "" && errors.Is(err, ErrSessionNotRunning) {
		err = nil // The captured owner exited before cancellation acquired it.
	}
	select {
	case <-run.exited:
		return err
	case <-ctx.Done():
		return errors.Join(err, ctx.Err())
	}
}

func (r *Runtime) cancel(ctx context.Context, sessionID, expectedExecutionID string) error {
	r.mu.Lock()
	run, ok := r.active[sessionID]
	if !ok {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	r.mu.Unlock()
	return r.cancelRun(ctx, sessionID, expectedExecutionID, run)
}

func (r *Runtime) cancelRun(ctx context.Context, sessionID, expectedExecutionID string, run *activeRun) error {
	run.control.Lock()
	defer run.control.Unlock()
	r.mu.Lock()
	if expectedExecutionID != "" && (r.active[sessionID] != run || run.executionID != expectedExecutionID) {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	if r.active[sessionID] != run {
		r.mu.Unlock()
		return ErrSessionNotRunning
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

// PermissionBinding identifies the actual admitted attempt and native callback
// route. It grants no permission and is valid only while every field matches.
type PermissionBinding struct {
	ExecutionID     string
	AttemptID       string
	EngineID        string
	NativeSessionID string
}

func permissionBinding(run *activeRun) PermissionBinding {
	return PermissionBinding{ExecutionID: run.executionID, AttemptID: run.attemptID, EngineID: run.engineID, NativeSessionID: run.nativeID}
}

// ActivePermission observes a ready callback route without discovering or
// starting an owner. Admission without an actual native identity is not ready.
func (r *Runtime) ActivePermission(sessionID string) (PermissionBinding, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	run := r.active[sessionID]
	if run == nil || run.engine == nil || run.executionID == "" || run.attemptID == "" || run.engineID == "" || run.nativeID == "" {
		return PermissionBinding{}, false
	}
	return permissionBinding(run), true
}

func (r *Runtime) RespondPermission(ctx context.Context, sessionID, requestID string, decision engine.PermissionDecision) error {
	binding, ready := r.ActivePermission(sessionID)
	if !ready {
		return ErrSessionNotRunning
	}
	return r.RespondPermissionBound(ctx, sessionID, binding, requestID, decision)
}

// RespondPermissionBound dispatches only to the previously observed physical
// owner and attempt. Fallback may keep ExecutionID but changes this binding.
func (r *Runtime) RespondPermissionBound(ctx context.Context, sessionID string, expected PermissionBinding, requestID string, decision engine.PermissionDecision) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if expected.ExecutionID == "" || expected.AttemptID == "" || expected.EngineID == "" || expected.NativeSessionID == "" {
		return errors.New("Hub permission binding required")
	}
	r.mu.Lock()
	run := r.active[sessionID]
	r.mu.Unlock()
	if run == nil {
		return ErrSessionNotRunning
	}
	run.control.Lock()
	defer run.control.Unlock()
	r.mu.Lock()
	if r.active[sessionID] != run || run.engine == nil || permissionBinding(run) != expected {
		r.mu.Unlock()
		return ErrSessionNotRunning
	}
	target, id := run.engine, run.nativeID
	pending := run.interactions[interactionKey(engine.PermissionRequest, requestID)]
	if pending != nil && pending.answered {
		r.mu.Unlock()
		return engine.ErrSessionNotFound
	}
	r.mu.Unlock()
	if !target.Capabilities().PermissionCallback {
		return engine.ErrUnsupported
	}
	if err := target.RespondPermission(ctx, id, requestID, decision); err != nil {
		return err
	}
	r.markInteractionAnswered(sessionID, run, expected, engine.PermissionRequest, requestID, pending)
	return nil
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

// StartOnBranchOwned returns the immutable physical execution identity from
// this admission, even if it finishes before the caller receives the result.
func (r *Runtime) StartOnBranchOwned(owner context.Context, sessionID, branchID, prompt string) (string, string, <-chan error, error) {
	return r.startOwned(owner, sessionID, func(ctx context.Context) (Claim, error) {
		return r.store.BeginTurnOnBranch(ctx, sessionID, branchID, prompt)
	})
}

func (r *Runtime) start(owner context.Context, sessionID string, admit func(context.Context) (Claim, error)) (string, <-chan error, error) {
	turn, _, done, err := r.startOwned(owner, sessionID, admit)
	return turn, done, err
}

func (r *Runtime) startOwned(owner context.Context, sessionID string, admit func(context.Context) (Claim, error)) (string, string, <-chan error, error) {
	ctx, cancel := context.WithCancel(owner)
	r.mu.Lock()
	if r.closed {
		r.mu.Unlock()
		cancel()
		return "", "", nil, errors.New("Hub runtime is closed")
	}
	if _, busy := r.active[sessionID]; busy {
		r.mu.Unlock()
		cancel()
		return "", "", nil, ErrSessionBusy
	}
	if len(r.active) >= 64 {
		r.mu.Unlock()
		cancel()
		return "", "", nil, errors.New("Hub execution budget exhausted (64 admitted runs)")
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
		return "", "", nil, err
	}
	r.mu.Lock()
	active.executionID = claim.Attempt.ID
	r.mu.Unlock()
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
	return claim.Turn.ID, claim.Attempt.ID, active.done, nil
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
		if outcome == engine.Succeeded || outcome == engine.Waiting || outcome == engine.Cancelled || ctx.Err() != nil || claim.WaitRevision != 0 {
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
			if runtime == nil || !supports(runtime.Capabilities(), target.RequiredCapabilities) || !supportsComposerInputs(runtime.Capabilities(), claim.Turn) {
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
func supportsComposerInputs(c engine.Capabilities, turn Turn) bool {
	return (len(turn.InputFiles) == 0 || c.FileInputs) && (len(turn.InputSkills) == 0 || c.SkillInputs)
}
func supports(c engine.Capabilities, required []string) bool {
	for _, name := range required {
		switch name {
		case "durable_waits":
			if !c.DurableWaits {
				return false
			}
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
		case "user_input_callback":
			if !c.UserInputCallback {
				return false
			}
		case "file_inputs":
			if !c.FileInputs {
				return false
			}
		case "skill_inputs":
			if !c.SkillInputs {
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
	descriptor := r.descriptors[claim.Attempt.Target.EngineID]
	active.engine = target
	active.engineID = claim.Attempt.Target.EngineID
	active.nativeID = claim.Branch.NativeSessionID
	active.attemptID = claim.Attempt.ID
	active.interactions = make(map[string]*pendingEngineInteraction)
	r.mu.Unlock()
	contract := engine.StreamContract{}
	outcome := engine.Failed
	failure := engine.FailureExecution
	sideEffects := false
	var fatalMessage string
	open := make(map[string]string)
	var persistenceErr error
	commit := func(event engine.Event) error {
		if event.Kind == engine.Completed && event.Outcome == engine.Waiting && (r.waits == nil || target == nil || !target.Capabilities().DurableWaits) {
			return fmt.Errorf("%w: durable waiting was not configured for this engine/owner", engine.ErrUnsupported)
		}
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
		if event.Kind == engine.PermissionRequest || event.Kind == engine.UserInputRequested {
			r.mu.Lock()
			_, duplicate := active.interactions[interactionKey(event.Kind, event.ID)]
			full := len(active.interactions) >= 64
			r.mu.Unlock()
			if duplicate || full {
				return errors.New("duplicate or excessive pending engine interactions")
			}
		}
		activityRisk := event.Kind == engine.ExecutionActivity && event.Activity != nil && event.Activity.Effect != engine.EffectRead && event.Activity.Effect != engine.EffectNone
		possibleEffects := sideEffects || activityRisk || (event.Kind == engine.SessionStarted && target != nil && !target.Capabilities().ToolControl) || (event.Kind == engine.ToolStart && event.Effect != engine.EffectRead)
		if err := r.store.Record(context.WithoutCancel(ctx), claim.Attempt.ID, event, possibleEffects); err != nil {
			persistenceErr = err
			return err
		}
		contract = next
		r.mu.Lock()
		switch event.Kind {
		case engine.PermissionRequest, engine.UserInputRequested:
			active.interactions[interactionKey(event.Kind, event.ID)] = &pendingEngineInteraction{event: cloneInteractionEvent(event)}
		case engine.InteractionResolved:
			kind := engine.PermissionRequest
			if event.Name == "user_input" {
				kind = engine.UserInputRequested
			}
			delete(active.interactions, interactionKey(kind, event.ID))
		case engine.Completed:
			clear(active.interactions)
		}
		r.mu.Unlock()
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
	// Streaming deltas are coalesced under eventMu before their durable commit.
	// Each raw delta is still validated immediately, so the first error stays
	// latched at the callback that caused it; relative event order is unchanged.
	window, tick, limit := streamCoalesceWindow, streamCoalesceTick, streamCoalesceBytes
	var pending engine.Event
	buffered := false
	var bufferedAt time.Time
	flush := func() {
		if !buffered {
			return
		}
		event := pending
		pending, buffered = engine.Event{}, false
		if eventErr == nil {
			eventErr = commit(event)
		}
	}
	observe := func(event engine.Event) error {
		eventMu.Lock()
		defer eventMu.Unlock()
		if !accepting {
			return errors.New("engine callback after Execute returned")
		}
		if eventErr != nil {
			return eventErr
		}
		if !coalescibleDelta(event) {
			flush()
			if eventErr == nil {
				eventErr = commit(event)
			}
			return eventErr
		}
		probe := contract.Clone()
		if buffered {
			// The buffered delta was validated against this same state.
			_ = probe.Accept(pending)
		}
		if err := probe.Accept(event); err != nil {
			// Deltas accepted before the rejected one stay durable, as they
			// would have been without coalescing.
			flush()
			if eventErr == nil {
				eventErr = err
			}
			return eventErr
		}
		if buffered && pending.Kind == event.Kind && pending.ID == event.ID && pending.SessionID == event.SessionID && len(pending.Text)+len(event.Text) <= limit {
			pending.Text += event.Text
			if time.Since(bufferedAt) >= window {
				flush()
			}
			return eventErr
		}
		flush()
		if eventErr != nil {
			return eventErr
		}
		pending, buffered, bufferedAt = event, true, time.Now()
		if len(event.Text) >= limit {
			flush()
		}
		return eventErr
	}
	var executeErr error
	if claim.WaitRevision != 0 && target != nil && !target.Capabilities().DurableWaits {
		executeErr = engine.ErrUnsupported
	} else if target == nil {
		executeErr = errors.New("requested engine is not registered")
		failure = engine.FailureStartup
	} else if inputErr := engine.ValidateInputFiles(claim.Turn.InputFiles); inputErr != nil {
		executeErr = inputErr
	} else if inputErr := engine.ValidateInputSkills(claim.Turn.InputSkills); inputErr != nil {
		executeErr = inputErr
	} else if !supportsComposerInputs(target.Capabilities(), claim.Turn) {
		executeErr = fmt.Errorf("%w: engine cannot accept the admitted files or Skills", engine.ErrUnsupported)
		failure = engine.FailureStartup
	} else if optionErr := claim.Attempt.ModelOptions.Validate(); optionErr != nil {
		executeErr = optionErr
		failure = engine.FailureStartup
	} else if (!claim.Attempt.ModelOptions.Generation.Empty() && !descriptor.Config.SupportsGeneration) || (claim.Attempt.ModelOptions.SystemPrompt != "" && !descriptor.Config.SupportsSystemPrompt) || (claim.Attempt.ModelOptions.MaxTurns != 0 && !descriptor.Config.SupportsMaxTurns) {
		executeErr = fmt.Errorf("%w: engine does not support the admitted model options", engine.ErrUnsupported)
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
				request := engine.Request{OwnerID: claim.Session.ID, Prompt: handoff + claim.Turn.Prompt, ResumeSession: resume, Workspace: claim.Session.Workspace, Model: claim.Attempt.Target.ModelID, Provider: claim.Attempt.Target.ProviderID, AccountSource: claim.Attempt.Target.AccountSource, BillingRoute: claim.Attempt.Target.BillingRoute, SystemPrompt: claim.Attempt.ModelOptions.SystemPrompt, MaxTurns: claim.Attempt.ModelOptions.MaxTurns, Generation: claim.Attempt.ModelOptions.Generation.Clone()}
				request.InputFiles = engine.CloneInputFiles(claim.Turn.InputFiles)
				request.InputSkills = engine.CloneInputSkills(claim.Turn.InputSkills)
				if claim.WaitRevision != 0 {
					request.Prompt, request.WaitRevision = "", claim.WaitRevision
				}
				if target.Capabilities().ToolControl {
					request.RetainWorkspace = retain
				}
				// A paused model still publishes received text within one window.
				stopTicker, tickerDone := make(chan struct{}), make(chan struct{})
				go func() {
					defer close(tickerDone)
					ticker := time.NewTicker(tick)
					defer ticker.Stop()
					for {
						select {
						case <-stopTicker:
							return
						case <-ticker.C:
							eventMu.Lock()
							if accepting && eventErr == nil && buffered && time.Since(bufferedAt) >= window {
								flush()
							}
							eventMu.Unlock()
						}
					}
				}()
				executeErr = target.Execute(ctx, request, observe)
				close(stopTicker)
				<-tickerDone
				seal()
			}
		}
	}
	eventMu.Lock()
	flush()
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
	if outcome != engine.Succeeded && outcome != engine.Waiting && executeErr == nil {
		executeErr = errors.New(fatalMessage)
		if fatalMessage == "" {
			executeErr = errors.New("engine attempt failed")
		}
	}
	return outcome, sideEffects, failure, executeErr
}

// Streaming deltas are merged into one durable row per window or byte budget.
// Tests may shorten the window; product code never changes these at runtime.
var (
	streamCoalesceWindow = 250 * time.Millisecond
	streamCoalesceTick   = 100 * time.Millisecond
	streamCoalesceBytes  = 16 << 10
)

// coalescibleDelta admits only plain text deltas. Any other field would be
// lost by concatenation, so such an event is committed on its own.
func coalescibleDelta(event engine.Event) bool {
	if event.Kind != engine.MessageDelta && event.Kind != engine.ReasoningDelta {
		return false
	}
	return event.Name == "" && len(event.Input) == 0 && event.Reference == nil && event.Effect == "" && !event.IsError && !event.Recoverable && event.Failure == "" && event.Outcome == "" && event.Usage == nil && event.Activity == nil && event.Wait == nil && event.Question == nil
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

// SessionActive observes physical execution ownership, including final cleanup.
func (r *Runtime) SessionActive(id string) bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	return r.active[id] != nil
}

// ActiveExecution snapshots physical execution ownership, including final cleanup.
// Its ID is the initial admitted Attempt ID, retained across fallback. While
// admission is uncommitted, active is true and executionID is empty.
func (r *Runtime) ActiveExecution(sessionID string) (executionID string, active bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	run := r.active[sessionID]
	if run == nil {
		return "", false
	}
	return run.executionID, true
}
