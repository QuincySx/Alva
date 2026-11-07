// INPUT: Actual SQLite product claims, controlled EngineRuntime owners and cancellation/permission barriers
// OUTPUT: Bound cancellation/join, permission dispatch, fallback and same-Turn resume ownership regressions
// POS: [L7 Hub product] UI actions target the actual in-memory owner and attempt through physical completion
package hub_test

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
)

type identityEngine struct {
	run     func(context.Context, engine.Request, func(engine.Event) error) error
	cancels chan string
}

func (e identityEngine) Execute(ctx context.Context, req engine.Request, emit func(engine.Event) error) error {
	return e.run(ctx, req, emit)
}
func (e identityEngine) Cancel(_ context.Context, id string) error {
	e.cancels <- id
	return nil
}
func (identityEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (identityEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true, DurableWaits: true}
}

func identityTarget(id string) hub.Target {
	return hub.Target{EngineID: id, ModelID: "fixture", ProviderID: "local", AccountSource: "test-only", BillingRoute: "no-billing"}
}
func identityStore(t *testing.T) (*sqlite.Store, hub.Session) {
	t.Helper()
	s, err := sqlite.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Close() })
	session, err := s.CreateSession(t.Context(), "execution identity", t.TempDir(), identityTarget("fixture"))
	if err != nil {
		t.Fatal(err)
	}
	return s, session
}
func ownIdentityRuntime(t *testing.T, r *hub.Runtime) {
	t.Helper()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		if err := r.Close(ctx); err != nil {
			t.Error("runtime cleanup did not finish", err)
		}
	})
}
func identityReceive[T any](t *testing.T, ch <-chan T) T {
	t.Helper()
	select {
	case value := <-ch:
		return value
	case <-time.After(3 * time.Second):
		t.Fatal("execution barrier timed out")
		var zero T
		return zero
	}
}
func identitySnapshot(t *testing.T, r *hub.Runtime, session, want string, active bool) {
	t.Helper()
	if id, ok := r.ActiveExecution(session); id != want || ok != active {
		t.Fatalf("ActiveExecution = (%q, %v), want (%q, %v)", id, ok, want, active)
	}
}

type identityCall struct {
	ctx    context.Context
	finish func()
}

func identityBarrier() (<-chan struct{}, func()) {
	done := make(chan struct{})
	return done, sync.OnceFunc(func() { close(done) })
}

func identityBlockingEngine(t *testing.T) (identityEngine, <-chan identityCall) {
	t.Helper()
	calls := make(chan identityCall, 1)
	e := identityEngine{cancels: make(chan string, 4)}
	e.run = func(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "identity-native"}); err != nil {
			return err
		}
		finish, release := identityBarrier()
		calls <- identityCall{ctx: ctx, finish: release}
		<-finish
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	}
	return e, calls
}

func TestRuntimeExecutionIdentityRejectsStaleCancelAndRetainsCleanup(t *testing.T) {
	s, session := identityStore(t)
	r := hub.New(s)
	ownIdentityRuntime(t, r)
	e, calls := identityBlockingEngine(t)
	if err := r.Register("fixture", e); err != nil {
		t.Fatal(err)
	}
	identitySnapshot(t, r, session.ID, "", false)
	if err := r.CancelExecution(t.Context(), session.ID, "missing"); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("idle execution was cancellable", err)
	}
	_, firstID, done, err := r.StartOnBranchOwned(t.Context(), session.ID, session.ActiveBranchID, "first")
	if err != nil {
		t.Fatal(err)
	}
	first := identityReceive(t, calls)
	defer first.finish()
	observedID, active := r.ActiveExecution(session.ID)
	turn, err := s.BranchTurn(t.Context(), session.ID, session.ActiveBranchID)
	if err != nil || !active || firstID == "" || firstID != turn.ActiveAttemptID || firstID != observedID {
		t.Fatal("identity did not match actual admitted attempt", firstID, active, turn, err)
	}
	first.finish()
	if err = identityReceive(t, done); err != nil {
		t.Fatal(err)
	}
	identitySnapshot(t, r, session.ID, "", false)
	_, secondID, done, err := r.StartOnBranchOwned(t.Context(), session.ID, session.ActiveBranchID, "second")
	if err != nil {
		t.Fatal(err)
	}
	second := identityReceive(t, calls)
	defer second.finish()
	observedID, active = r.ActiveExecution(session.ID)
	if !active || secondID == "" || secondID == firstID || secondID != observedID {
		t.Fatal("new owner reused execution identity", firstID, secondID, active)
	}
	if err = r.CancelExecution(t.Context(), session.ID, firstID); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("stale cancellation accepted", err)
	}
	if err = r.CancelExecutionAndWait(t.Context(), session.ID, firstID); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("stale cancel/join accepted a replacement owner", err)
	}
	if err = r.CancelExecutionAndWait(t.Context(), session.ID, ""); err == nil {
		t.Fatal("empty cancel/join identity accepted")
	}
	if err = r.CancelExecution(t.Context(), session.ID, ""); err == nil {
		t.Fatal("empty cancellation identity accepted")
	}
	if second.ctx.Err() != nil || len(e.cancels) != 0 {
		t.Fatal("rejected cancellation reached the new owner")
	}
	if err = r.CancelExecution(t.Context(), session.ID, secondID); err != nil {
		t.Fatal(err)
	}
	if second.ctx.Err() != context.Canceled || identityReceive(t, e.cancels) != "identity-native" {
		t.Fatal("current execution was not cancelled through its actual native route")
	}
	identitySnapshot(t, r, session.ID, secondID, true)
	select {
	case <-done:
		t.Fatal("cancellation claimed cleanup had completed")
	default:
	}
	short, cancel := context.WithTimeout(t.Context(), 20*time.Millisecond)
	defer cancel()
	if err = r.Close(short); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("Close did not wait for actual Execute return", err)
	}
	identitySnapshot(t, r, session.ID, secondID, true)
	second.finish()
	if err = identityReceive(t, done); !errors.Is(err, context.Canceled) {
		t.Fatal("actual cancellation result lost", err)
	}
	if err = r.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	identitySnapshot(t, r, session.ID, "", false)
}

type identityAdmissionStore struct {
	hub.ExecutionStore
	entered chan context.Context
	release <-chan struct{}
}

func (s identityAdmissionStore) BeginTurn(ctx context.Context, sessionID, prompt string) (hub.Claim, error) {
	s.entered <- ctx
	<-s.release
	return s.ExecutionStore.BeginTurn(ctx, sessionID, prompt)
}

func TestRuntimeExecutionIdentityIsEmptyDuringUncommittedAdmission(t *testing.T) {
	s, session := identityStore(t)
	release, finishAdmission := identityBarrier()
	defer finishAdmission()
	admission := identityAdmissionStore{ExecutionStore: s, entered: make(chan context.Context, 1), release: release}
	r := hub.New(admission)
	ownIdentityRuntime(t, r)
	done := make(chan error, 1)
	go func() {
		_, _, err := r.Start(t.Context(), session.ID, "blocked admission")
		done <- err
	}()
	ctx := identityReceive(t, admission.entered)
	identitySnapshot(t, r, session.ID, "", true)
	if binding, ready := r.ActivePermission(session.ID); ready || binding != (hub.PermissionBinding{}) {
		t.Fatal("uncommitted admission invented a native permission route", binding, ready)
	}
	if err := r.CancelExecution(t.Context(), session.ID, "previous-owner"); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("stale identity cancelled uncommitted admission", err)
	}
	if ctx.Err() != nil {
		t.Fatal("stale cancellation reached admission", ctx.Err())
	}
	if err := r.Cancel(t.Context(), session.ID); err != nil || ctx.Err() != context.Canceled {
		t.Fatal("session cancellation no longer reaches admission", err, ctx.Err())
	}
	identitySnapshot(t, r, session.ID, "", true)
	finishAdmission()
	if err := identityReceive(t, done); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled admission result lost", err)
	}
	identitySnapshot(t, r, session.ID, "", false)
}

func TestRuntimeCancelAndWaitRetainsUncommittedAdmissionUntilActualExit(t *testing.T) {
	s, session := identityStore(t)
	release, finishAdmission := identityBarrier()
	defer finishAdmission()
	admission := identityAdmissionStore{ExecutionStore: s, entered: make(chan context.Context, 1), release: release}
	r := hub.New(admission)
	ownIdentityRuntime(t, r)
	done := make(chan error, 1)
	go func() {
		_, _, err := r.Start(t.Context(), session.ID, "blocked admission")
		done <- err
	}()
	owner := identityReceive(t, admission.entered)
	short, cancel := context.WithTimeout(t.Context(), 20*time.Millisecond)
	defer cancel()
	if err := r.CancelAndWait(short, session.ID); !errors.Is(err, context.DeadlineExceeded) || owner.Err() != context.Canceled {
		t.Fatal("cancel/join did not retain the uncommitted physical owner", err, owner.Err())
	}
	identitySnapshot(t, r, session.ID, "", true)
	if _, _, err := r.Start(t.Context(), session.ID, "replacement before cleanup"); !errors.Is(err, hub.ErrSessionBusy) {
		t.Fatal("cancel/join released admission before its actual exit", err)
	}
	finishAdmission()
	if err := identityReceive(t, done); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled admission result lost", err)
	}
	if err := r.CancelAndWait(t.Context(), session.ID); !errors.Is(err, hub.ErrSessionNotRunning) {
		t.Fatal("idle cancel/join invented an owner", err)
	}
	identitySnapshot(t, r, session.ID, "", false)
}

func TestRuntimeExecutionIdentityIsStableAcrossFallback(t *testing.T) {
	s, session := identityStore(t)
	r := hub.New(s)
	ownIdentityRuntime(t, r)
	firstStarted := make(chan struct{})
	fallback, allowFallback := identityBarrier()
	defer allowFallback()
	if err := r.Register("fixture", identityEngine{run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		close(firstStarted)
		<-fallback
		if err := emit(engine.Event{Kind: engine.Error, Text: "startup unavailable", Failure: engine.FailureStartup}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
	}}); err != nil {
		t.Fatal(err)
	}
	e, calls := identityBlockingEngine(t)
	if err := r.Register("fallback", e); err != nil {
		t.Fatal(err)
	}
	target := identityTarget("fallback")
	target.AutomaticAuthorized = true
	if err := s.SetFallback(t.Context(), session.ID, []hub.Target{target}); err != nil {
		t.Fatal(err)
	}
	_, done, err := r.Start(t.Context(), session.ID, "fallback")
	if err != nil {
		t.Fatal(err)
	}
	identityReceive(t, firstStarted)
	id, active := r.ActiveExecution(session.ID)
	initial, err := s.BranchTurn(t.Context(), session.ID, session.ActiveBranchID)
	if err != nil || !active || id == "" || id != initial.ActiveAttemptID {
		t.Fatal("initial owner identity did not match its claim", id, initial, err)
	}
	allowFallback()
	call := identityReceive(t, calls)
	defer call.finish()
	identitySnapshot(t, r, session.ID, id, true)
	detail, err := s.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	current, err := s.BranchTurn(t.Context(), session.ID, detail.Session.ActiveBranchID)
	if err != nil || current.ActiveAttemptID == id {
		t.Fatal("fixture did not admit a distinct fallback attempt", current, err)
	}
	if err = r.CancelExecution(t.Context(), session.ID, id); err != nil || call.ctx.Err() != context.Canceled {
		t.Fatal("owner identity did not reach its fallback attempt", err, call.ctx.Err())
	}
	call.finish()
	if err = identityReceive(t, done); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestRuntimeExecutionIdentityChangesOnSameTurnWaitResume(t *testing.T) {
	s, session := identityStore(t)
	if err := s.SetDurableWaits(t.Context(), session.ID, true); err != nil {
		t.Fatal(err)
	}
	r := hub.NewWithWaits(s)
	ownIdentityRuntime(t, r)
	e, calls := identityBlockingEngine(t)
	resume := e.run
	finishWait, allowWaiting := identityBarrier()
	defer allowWaiting()
	e.run = func(ctx context.Context, req engine.Request, emit func(engine.Event) error) error {
		if req.WaitRevision != 0 {
			return resume(ctx, req, emit)
		}
		<-finishWait
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "identity-native"}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Waiting, Wait: &engine.WaitCheckpoint{Revision: 3, Requests: 1, ExpiresAt: time.Now().Add(time.Hour).UnixMilli()}})
	}
	if err := r.Register("fixture", e); err != nil {
		t.Fatal(err)
	}
	turnID, done, err := r.Start(t.Context(), session.ID, "wait")
	if err != nil {
		t.Fatal(err)
	}
	firstID, active := r.ActiveExecution(session.ID)
	if !active || firstID == "" {
		t.Fatal("waiting owner has no execution identity")
	}
	allowWaiting()
	if err = identityReceive(t, done); err != nil {
		t.Fatal(err)
	}
	resumedTurn, resumedID, done, err := r.ResumeWaitOwned(t.Context(), session.ID, session.ActiveBranchID, turnID, 4)
	if err != nil {
		t.Fatal(err)
	}
	call := identityReceive(t, calls)
	defer call.finish()
	observedID, active := r.ActiveExecution(session.ID)
	if resumedTurn != turnID || !active || resumedID == "" || resumedID == firstID || resumedID != observedID {
		t.Fatal("same-Turn continuation reused its previous execution owner", resumedTurn, turnID, resumedID, firstID)
	}
	if err = r.CancelExecution(t.Context(), session.ID, firstID); !errors.Is(err, hub.ErrSessionNotRunning) || call.ctx.Err() != nil || len(e.cancels) != 0 {
		t.Fatal("previous waiting owner cancelled its continuation", err, call.ctx.Err())
	}
	if err = r.CancelExecution(t.Context(), session.ID, resumedID); err != nil {
		t.Fatal(err)
	}
	call.finish()
	if err = identityReceive(t, done); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

type permissionIdentityEngine struct {
	identityEngine
	permissions chan string
}

func (e permissionIdentityEngine) RespondPermission(_ context.Context, native, request string, _ engine.PermissionDecision) error {
	e.permissions <- native + "/" + request
	return nil
}
func (e permissionIdentityEngine) Capabilities() engine.Capabilities {
	caps := e.identityEngine.Capabilities()
	caps.PermissionCallback = true
	return caps
}

func TestRuntimePermissionBindingRejectsSameRunFallback(t *testing.T) {
	for _, change := range []string{"engine", "native", "attempt"} {
		t.Run(change, func(t *testing.T) {
			s, session := identityStore(t)
			r := hub.New(s)
			ownIdentityRuntime(t, r)
			firstStarted, fallbackStarted := make(chan struct{}), make(chan struct{})
			fallback, allowFallback := identityBarrier()
			defer allowFallback()
			firstNative, fallbackNative := "permission-native", "permission-native"
			if change == "native" {
				fallbackNative = "fallback-native"
			}
			e := permissionIdentityEngine{permissions: make(chan string, 8)}
			e.run = func(ctx context.Context, req engine.Request, emit func(engine.Event) error) error {
				native := firstNative
				if req.Model == "fallback" {
					native = fallbackNative
				}
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: native}); err != nil {
					return err
				}
				if req.Model == "fallback" {
					close(fallbackStarted)
					<-ctx.Done()
					return ctx.Err()
				}
				close(firstStarted)
				<-fallback
				if err := emit(engine.Event{Kind: engine.Error, Text: "startup unavailable", Failure: engine.FailureStartup}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
			}
			if err := r.Register("fixture", e); err != nil {
				t.Fatal(err)
			}
			target := identityTarget("fixture")
			if change == "engine" {
				target.EngineID = "fallback"
				if err := r.Register(target.EngineID, e); err != nil {
					t.Fatal(err)
				}
			}
			target.ModelID = "fallback"
			target.AutomaticAuthorized = true
			if err := s.SetFallback(t.Context(), session.ID, []hub.Target{target}); err != nil {
				t.Fatal(err)
			}
			if _, _, err := r.Start(t.Context(), session.ID, "permission fallback fixture"); err != nil {
				t.Fatal(err)
			}
			identityReceive(t, firstStarted)
			first, ready := r.ActivePermission(session.ID)
			if !ready || first.ExecutionID == "" || first.AttemptID == "" || first.EngineID != "fixture" || first.NativeSessionID != firstNative {
				t.Fatal("first observed route did not describe its actual owner", first, ready)
			}
			allowFallback()
			identityReceive(t, fallbackStarted)
			current, ready := r.ActivePermission(session.ID)
			if !ready || current.ExecutionID != first.ExecutionID || current.AttemptID == first.AttemptID || current.EngineID != target.EngineID || current.NativeSessionID != fallbackNative {
				t.Fatal("fixture did not replace the attempt within its original physical run", first, current, ready)
			}
			if err := r.RespondPermissionBound(t.Context(), session.ID, first, "old-request", engine.PermissionDecision{Allow: true}); !errors.Is(err, hub.ErrSessionNotRunning) {
				t.Fatal("old attempt permission was accepted by its fallback", err)
			}
			for _, field := range []string{"execution", "attempt", "engine", "native"} {
				mismatched := current
				switch field {
				case "execution":
					mismatched.ExecutionID += "-old"
				case "attempt":
					mismatched.AttemptID += "-old"
				case "engine":
					mismatched.EngineID += "-old"
				case "native":
					mismatched.NativeSessionID += "-old"
				}
				if err := r.RespondPermissionBound(t.Context(), session.ID, mismatched, "mismatched-request", engine.PermissionDecision{Allow: true}); !errors.Is(err, hub.ErrSessionNotRunning) {
					t.Fatal("permission dispatch ignored its observed "+field, err)
				}
			}
			if len(e.permissions) != 0 {
				t.Fatal("a rejected binding reached an actual permission callback")
			}
			if err := r.RespondPermissionBound(t.Context(), session.ID, current, "current-request", engine.PermissionDecision{Allow: false}); err != nil {
				t.Fatal("actual current permission owner was rejected", err)
			}
			if got := identityReceive(t, e.permissions); got != fallbackNative+"/current-request" {
				t.Fatal("bound permission changed its actual native identity", got)
			}
			if err := r.RespondPermission(t.Context(), session.ID, "current-unbound-request", engine.PermissionDecision{Allow: false}); err != nil {
				t.Fatal("ordinary permission callback no longer shares the current binding", err)
			}
			if got := identityReceive(t, e.permissions); got != fallbackNative+"/current-unbound-request" {
				t.Fatal("ordinary permission callback changed its actual native identity", got)
			}
		})
	}
}
