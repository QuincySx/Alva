// INPUT: testing, actual private SQLite and deterministic EngineRuntime fixtures
// OUTPUT: durable product routing, reversible branches, safe fallback, recovery and scope regressions
// POS: [L7 Hub product] Product state-machine tests separate from native Agent execution facts
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func target(id string) hub.Target {
	return hub.Target{EngineID: id, ModelID: "fixture", ProviderID: "local", AccountSource: "test-only", BillingRoute: "no-billing"}
}
func newStore(t *testing.T) *Store {
	t.Helper()
	store, err := Open(context.Background(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	return store
}
func complete(t *testing.T, s *Store, claim hub.Claim, native string) {
	t.Helper()
	ctx := context.Background()
	if err := s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: native}, false); err != nil {
		t.Fatal(err)
	}
	if err := s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err := s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}
func TestReversibleBranchesKeepNativeIdentityAndDeltaHandoff(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	first, err := s.BeginTurn(ctx, session.ID, "first")
	if err != nil {
		t.Fatal(err)
	}
	complete(t, s, first, "native-a")
	branchB, err := s.Switch(ctx, session.ID, target("b"), "try another engine")
	if err != nil {
		t.Fatal(err)
	}
	second, err := s.BeginTurn(ctx, session.ID, "second")
	if err != nil {
		t.Fatal(err)
	}
	if second.Handoff == nil || len(second.Handoff.Entries) != 1 {
		t.Fatal("missing handoff index")
	}
	complete(t, s, second, "native-b")
	returned, err := s.Switch(ctx, session.ID, target("a"), "return")
	if err != nil {
		t.Fatal(err)
	}
	if returned.ID != first.Branch.ID || returned.NativeSessionID != "native-a" || returned.Watermark != 1 {
		t.Fatal("old branch was overwritten", returned)
	}
	third, err := s.BeginTurn(ctx, session.ID, "third")
	if err != nil {
		t.Fatal(err)
	}
	if third.Handoff == nil || len(third.Handoff.Entries) != 1 || third.Handoff.Entries[0].TurnID != second.Turn.ID {
		t.Fatal("delta handoff duplicated or lost turns", third.Handoff)
	}
	if _, err = s.Switch(ctx, session.ID, target("b"), "during run"); err == nil {
		t.Fatal("switch rewrote running claim")
	}
	if branchB.ID == returned.ID {
		t.Fatal("engine branches collapsed")
	}
}

type runtimeFunc struct {
	run     func(context.Context, engine.Request, func(engine.Event) error) error
	control bool
}

type failingStore struct {
	hub.Store
	terminal bool
	detail   bool
}

func (s failingStore) Record(ctx context.Context, id string, event engine.Event, effects bool) error {
	if s.terminal && event.Kind == engine.Completed {
		return errors.New("fixture terminal commit unavailable")
	}
	return s.Store.Record(ctx, id, event, effects)
}
func (s failingStore) Detail(ctx context.Context, id string) (hub.Detail, error) {
	if s.detail {
		return hub.Detail{}, errors.New("fixture fallback detail unavailable")
	}
	return s.Store.Detail(ctx, id)
}
func TestRuntimeCommitOrFallbackReadFailureNeverLeavesTurnRunning(t *testing.T) {
	for _, mode := range []string{"terminal commit", "fallback detail"} {
		t.Run(mode, func(t *testing.T) {
			store := newStore(t)
			ctx := context.Background()
			session, err := store.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			runtime := hub.New(failingStore{Store: store, terminal: mode == "terminal commit", detail: mode == "fallback detail"})
			_ = runtime.Register("a", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "fixture-native"}); err != nil {
					return err
				}
				outcome := engine.Succeeded
				if mode == "fallback detail" {
					outcome = engine.Failed
					if err := emit(engine.Event{Kind: engine.Error, Text: "startup failed", Failure: engine.FailureStartup}); err != nil {
						return err
					}
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: outcome})
			}})
			if err = runtime.Run(ctx, session.ID, "test"); err == nil {
				t.Fatal("lost the real storage failure")
			}
			detail, err := store.Detail(ctx, session.ID)
			if err != nil {
				t.Fatal(err)
			}
			if detail.Session.Status != engine.Failed || runtime.HasActive() {
				t.Fatal("failed persistence/read left busy or falsely successful", detail.Session.Status)
			}
		})
	}
}

func TestRuntimeReturnedFailureAfterCompletionCannotLeaveSuccessfulTurn(t *testing.T) {
	ctx := context.Background()
	store := newStore(t)
	s, err := store.CreateSession(ctx, "terminal owner failure", t.TempDir(), target("fixture"))
	if err != nil {
		t.Fatal(err)
	}
	failure := errors.New("engine cleanup failed after early Completed")
	r := hub.New(store)
	if err = r.Register("fixture", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "native"}); err != nil {
			return err
		}
		if err := emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}); err != nil {
			return err
		}
		return failure
	}}); err != nil {
		t.Fatal(err)
	}
	turnID, done, err := r.Start(ctx, s.ID, "actual engine")
	if err != nil {
		t.Fatal(err)
	}
	if err = <-done; !errors.Is(err, failure) {
		t.Fatal("owner return error lost", err)
	}
	detail, err := store.Detail(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	turn, err := read[hub.Turn](ctx, store.db, `SELECT data FROM hub_turns WHERE id=?`, turnID)
	if err != nil {
		t.Fatal(err)
	}
	if detail.Session.Status != engine.Failed || turn.Status != engine.Failed {
		t.Fatal("execution owner failure falsely persisted success", detail.Session.Status)
	}
}
func TestLargeBranchGapUsesBoundedExplicitHandoffInsteadOfLockingAdmission(t *testing.T) {
	store := newStore(t)
	ctx := context.Background()
	session, err := store.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	first, err := store.BeginTurn(ctx, session.ID, "original")
	if err != nil {
		t.Fatal(err)
	}
	complete(t, store, first, "native-a")
	if _, err = store.Switch(ctx, session.ID, target("b"), "test"); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 35; i++ {
		claim, err := store.BeginTurn(ctx, session.ID, "missed turn")
		if err != nil {
			t.Fatal(err)
		}
		complete(t, store, claim, "native-b")
	}
	if _, err = store.Switch(ctx, session.ID, target("a"), "return"); err != nil {
		t.Fatal(err)
	}
	claim, err := store.BeginTurn(ctx, session.ID, "continue")
	if err != nil {
		t.Fatal(err)
	}
	if claim.Handoff == nil || len(claim.Handoff.Entries) != 32 || !strings.Contains(claim.Handoff.Processing, "Older unseen Turns omitted") {
		t.Fatal("large gap was neither bounded nor explicit")
	}
}
func TestHubCancellationDoesNotHideIndependentEngineFailure(t *testing.T) {
	store := newStore(t)
	ctx := context.Background()
	product, err := store.CreateSession(ctx, "cancel", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	started := make(chan struct{})
	runtime := hub.New(store)
	failure := errors.New("independent fixture storage failure")
	_ = runtime.Register("a", runtimeFunc{control: true, run: func(ctx context.Context, _ engine.Request, _ func(engine.Event) error) error {
		close(started)
		<-ctx.Done()
		return errors.Join(ctx.Err(), failure)
	}})
	_, done, err := runtime.Start(ctx, product.ID, "wait")
	if err != nil {
		t.Fatal(err)
	}
	<-started
	if err = runtime.Cancel(ctx, product.ID); err != nil {
		t.Fatal(err)
	}
	if err = <-done; !errors.Is(err, failure) {
		t.Fatal("lost independent failure", err)
	}
	detail, err := store.Detail(ctx, product.ID)
	if err != nil || detail.Session.Status != engine.Failed {
		t.Fatal("joined non-cancel failure mislabeled cancellation", err, detail.Session.Status)
	}
}

func (r runtimeFunc) Execute(ctx context.Context, req engine.Request, emit func(engine.Event) error) error {
	return r.run(ctx, req, emit)
}
func (runtimeFunc) Cancel(context.Context, string) error { return nil }
func (runtimeFunc) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (r runtimeFunc) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Cancel: true, Resume: true, ToolControl: r.control}
}
func TestFallbackSameTurnRequiresExactApprovalAndNoSideEffects(t *testing.T) {
	for _, sideEffects := range []bool{false, true} {
		name := "safe"
		if sideEffects {
			name = "side effects"
		}
		t.Run(name, func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			session, err := s.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			approved := target("b")
			approved.AutomaticAuthorized = true
			if err = s.SetFallback(ctx, session.ID, []hub.Target{approved}); err != nil {
				t.Fatal(err)
			}
			runtime := hub.New(s)
			calls := 0
			_ = runtime.Register("a", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "native-a"}); err != nil {
					return err
				}
				if sideEffects {
					if err := emit(engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "write", Effect: engine.EffectWrite}); err != nil {
						return err
					}
					if err := emit(engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "write"}); err != nil {
						return err
					}
				}
				if err := emit(engine.Event{Kind: engine.Error, Text: "startup fixture", Failure: engine.FailureStartup}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
			}})
			_ = runtime.Register("b", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				calls++
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "native-b"}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
			}})
			err = runtime.Run(ctx, session.ID, "work")
			if sideEffects {
				if err == nil || calls != 0 {
					t.Fatal("unsafe automatic fallback", calls, err)
				}
			} else if err != nil || calls != 1 {
				t.Fatal("authorized safe fallback failed", calls, err)
			}
			var turns, attempts int
			_ = s.db.QueryRow(`SELECT count(*) FROM hub_turns`).Scan(&turns)
			_ = s.db.QueryRow(`SELECT count(*) FROM hub_attempts`).Scan(&attempts)
			want := 2
			if sideEffects {
				want = 1
			}
			if turns != 1 || attempts != want {
				t.Fatal("fallback created a new Turn or wrong attempt count", turns, attempts)
			}
		})
	}
}
func TestRecoveryAndExclusiveDatabaseOwnership(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(ctx, "recover", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.BeginTurn(ctx, session.ID, "pending"); err != nil {
		t.Fatal(err)
	}
	if other, err := Open(ctx, dir); err == nil {
		other.Close()
		t.Fatal("two owners opened product recovery")
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	detail, err := s.Detail(ctx, session.ID)
	if err != nil || detail.Session.Status != engine.Interrupted {
		t.Fatal("crash state was replayed or not repaired", detail, err)
	}
}
func TestForeignReferenceAndStaleAttemptCannotWrite(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "scope", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "work")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: "native-a"}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.ToolEnd, ID: "bad", Reference: &engine.Reference{SessionID: "foreign", RecordID: "record"}}, false); err == nil {
		t.Fatal("foreign native facts linked")
	}
	if _, err = s.History(ctx, "foreign-hub", 0, 128); !errors.Is(err, sql.ErrNoRows) {
		t.Fatal("foreign product scope accepted", err)
	}
	complete(t, s, claim, "native-a")
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.MessageDelta, ID: "late", Text: "late"}, false); err == nil {
		t.Fatal("late attempt rewrote history")
	}
}

func TestRuntimeLatchesRejectedEngineEvents(t *testing.T) {
	ctx := context.Background()
	store := newStore(t)
	session, err := store.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	runtime := hub.New(store)
	defer runtime.Close(ctx)
	_ = runtime.Register("a", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "fixture-native"}); err != nil {
			return err
		}
		first := emit(engine.Event{Kind: engine.ToolEnd, ID: "unpaired"})
		if first == nil {
			t.Error("unpaired event accepted")
		}
		// A faulty adapter ignores the sink failure and invents successful closure.
		if next := emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}); !errors.Is(next, first) || next == nil {
			t.Error("first event rejection was not retained")
		}
		return nil
	}})
	if err := runtime.Run(ctx, session.ID, "fixture"); err == nil {
		t.Fatal("rejected engine event was erased by success")
	}
	detail, err := store.Detail(ctx, session.ID)
	if err != nil || detail.Session.Status != engine.Failed {
		t.Fatalf("rejected event became success: %+v %v", detail.Session, err)
	}
}

func TestOpenRejectsUnrecognizedUnversionedDatabase(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	db, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = db.ExecContext(ctx, "CREATE TABLE unrelated_data(value TEXT); INSERT INTO unrelated_data VALUES('keep')"); err != nil {
		t.Fatal(err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	store, err := Open(ctx, dir)
	if store != nil {
		store.Close()
	}
	if err == nil {
		t.Fatal("unrecognized existing database was adopted")
	}
	db, err = private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var value string
	if err = db.QueryRowContext(ctx, "SELECT value FROM unrelated_data").Scan(&value); err != nil || value != "keep" {
		t.Fatalf("original changed: %q %v", value, err)
	}
	var count int
	if err = db.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_master WHERE name LIKE 'hub_%'").Scan(&count); err != nil || count != 0 {
		t.Fatalf("Hub schema added to unrelated DB: %d %v", count, err)
	}
}

func TestRuntimeSerializesEngineCallbacksAndRejectsLateEvents(t *testing.T) {
	ctx := context.Background()
	store := newStore(t)
	session, err := store.CreateSession(ctx, "fixture", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	runtime := hub.New(store)
	defer runtime.Close(ctx)
	var late func(engine.Event) error
	_ = runtime.Register("a", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		late = emit
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "fixture-native"}); err != nil {
			return err
		}
		var group sync.WaitGroup
		for i := 0; i < 32; i++ {
			group.Add(1)
			go func(i int) {
				defer group.Done()
				if err := emit(engine.Event{Kind: engine.MessageDelta, ID: "message-" + strconv.Itoa(i), Text: "fixture"}); err != nil {
					t.Error(err)
				}
			}(i)
		}
		group.Wait()
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	}})
	if err := runtime.Run(ctx, session.ID, "fixture"); err != nil {
		t.Fatal(err)
	}
	before, err := store.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	if err := late(engine.Event{Kind: engine.MessageDelta, ID: "late", Text: "fixture"}); err == nil {
		t.Fatal("late event admitted")
	}
	after, err := store.History(ctx, session.ID, 0, 128)
	if err != nil || len(after) != len(before) {
		t.Fatalf("late event changed durable history: %d/%d %v", len(before), len(after), err)
	}
}
