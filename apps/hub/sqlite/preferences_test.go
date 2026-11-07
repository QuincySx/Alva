// INPUT: Real private SQLite, shared Hub preference policy and version-one product fixtures.
// OUTPUT: Durable preferences, atomic optimistic updates and non-destructive schema migration regressions.
// POS: [L7 SQLite adapter] Verifies persistence and failures without duplicating preference business rules.
package sqlite

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func TestPreferencesDefaultsAndDurableAtomicUpdate(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if store != nil {
			_ = store.Close()
		}
	})
	initial, err := store.Preferences(ctx)
	if err != nil || initial != hub.DefaultPreferences() {
		t.Fatalf("fresh preferences: %+v %v", initial, err)
	}
	theme, density, fontSize := "dark", "compact", 16
	saved, err := store.UpdatePreferences(ctx, hub.PreferenceUpdate{ExpectedRevision: initial.Revision, Theme: &theme, Density: &density, FontSize: &fontSize})
	if err != nil {
		t.Fatal(err)
	}
	if saved.Revision != initial.Revision+1 || saved.Theme != theme || saved.Density != density || saved.FontSize != fontSize {
		t.Fatalf("incomplete committed update: %+v", saved)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	reopened, err := store.Preferences(ctx)
	if err != nil || reopened != saved {
		t.Fatalf("preferences changed on reopen: %+v %v", reopened, err)
	}
	var version, rows int
	if err = store.db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(ctx, `SELECT count(*) FROM hub_preferences`).Scan(&rows); err != nil || version != 8 || rows != 1 {
		t.Fatalf("schema=%d singleton rows=%d: %v", version, rows, err)
	}
}

func TestPreferencesSidebarSparseCASReopenAndExplicitReset(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	store, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if store != nil {
			_ = store.Close()
		}
	})
	initial, err := store.Preferences(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	// Existing current-schema preference records omit the newly optional
	// layout fields. Reading them must preserve their appearance and revision.
	if _, err = store.db.ExecContext(t.Context(), `UPDATE hub_preferences SET data=json_remove(data,'$.sidebar_width','$.sidebar_collapsed')`); err != nil {
		t.Fatal(err)
	}
	if observed, err := store.Preferences(t.Context()); err != nil || observed != initial {
		t.Fatal("absent layout fields changed an existing record", observed, err)
	}
	theme, size, motion := "dark", 16, true
	appearance, err := store.UpdatePreferences(t.Context(), hub.PreferenceUpdate{ExpectedRevision: initial.Revision, Theme: &theme, FontSize: &size, ReducedMotion: &motion})
	if err != nil {
		t.Fatal(err)
	}
	width, collapsed := 364, true
	update := hub.PreferenceUpdate{ExpectedRevision: initial.Revision, SidebarWidth: &width, SidebarCollapsed: &collapsed}
	if value, err := store.UpdatePreferences(t.Context(), update); !errors.Is(err, hub.ErrPreferenceConflict) || value != (hub.Preferences{}) {
		t.Fatal("stale layout replaced concurrent appearance", value, err)
	}
	update.ExpectedRevision = appearance.Revision
	saved, err := store.UpdatePreferences(t.Context(), update)
	if err != nil || saved.Theme != theme || saved.FontSize != size || !saved.ReducedMotion || saved.SidebarWidth != width || !saved.SidebarCollapsed {
		t.Fatal("sparse retry did not preserve independent appearance", saved, err)
	}
	invalidWidth := hub.MaxSidebarWidth + 1
	if value, err := store.UpdatePreferences(t.Context(), hub.PreferenceUpdate{ExpectedRevision: saved.Revision, Theme: &theme, SidebarWidth: &invalidWidth}); !errors.Is(err, hub.ErrInvalidPreferences) || value != (hub.Preferences{}) {
		t.Fatal("invalid layout partially committed", value, err)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	actual, err := store.Preferences(t.Context())
	if err != nil || actual != saved {
		t.Fatal("reopening lost sidebar or appearance", actual, saved, err)
	}
	width, collapsed = 0, false
	reset, err := store.UpdatePreferences(t.Context(), hub.PreferenceUpdate{ExpectedRevision: actual.Revision, SidebarWidth: &width, SidebarCollapsed: &collapsed})
	if err != nil || reset.SidebarWidth != 0 || reset.SidebarCollapsed || reset.Theme != saved.Theme || reset.FontSize != saved.FontSize || reset.ReducedMotion != saved.ReducedMotion {
		t.Fatal("reset erased other preferences or ignored explicit zero/false", reset, err)
	}
}

func TestPreferencesCompetingUpdatesCommitExactlyOne(t *testing.T) {
	ctx := context.Background()
	store := newStore(t)
	initial, err := store.Preferences(ctx)
	if err != nil {
		t.Fatal(err)
	}
	type result struct {
		value hub.Preferences
		err   error
	}
	results := make(chan result, 2)
	start := make(chan struct{})
	var ready sync.WaitGroup
	ready.Add(2)
	for _, theme := range []string{"light", "dark"} {
		go func() {
			ready.Done()
			<-start
			value, err := store.UpdatePreferences(ctx, hub.PreferenceUpdate{ExpectedRevision: initial.Revision, Theme: &theme})
			results <- result{value, err}
		}()
	}
	ready.Wait()
	close(start)
	var winner hub.Preferences
	successes, conflicts := 0, 0
	for range 2 {
		result := <-results
		switch {
		case result.err == nil:
			successes++
			winner = result.value
		case errors.Is(result.err, hub.ErrPreferenceConflict):
			conflicts++
			if result.value != (hub.Preferences{}) {
				t.Fatal("conflict returned an uncommitted value", result.value)
			}
		default:
			t.Fatal(result.err)
		}
	}
	actual, err := store.Preferences(ctx)
	if err != nil || successes != 1 || conflicts != 1 || actual != winner || actual.Revision != initial.Revision+1 {
		t.Fatalf("successes=%d conflicts=%d persisted=%+v winner=%+v: %v", successes, conflicts, actual, winner, err)
	}
}

func TestPreferencesInvalidUpdatesDoNotPartiallyCommit(t *testing.T) {
	ctx := context.Background()
	store := newStore(t)
	initial, err := store.Preferences(ctx)
	if err != nil {
		t.Fatal(err)
	}
	theme, invalidDensity, invalidSize := "dark", "dense", 15
	for _, update := range []hub.PreferenceUpdate{
		{ExpectedRevision: initial.Revision, Theme: &theme, Density: &invalidDensity},
		{ExpectedRevision: initial.Revision, Theme: &theme, FontSize: &invalidSize},
		{ExpectedRevision: initial.Revision},
		{Theme: &theme},
	} {
		value, err := store.UpdatePreferences(ctx, update)
		if !errors.Is(err, hub.ErrInvalidPreferences) || value != (hub.Preferences{}) {
			t.Fatalf("invalid update returned %+v: %v", value, err)
		}
		actual, err := store.Preferences(ctx)
		if err != nil || actual != initial {
			t.Fatalf("invalid update partially committed: %+v %v", actual, err)
		}
	}
}

func TestPreferencesCancellationLeavesDurableStateUnchanged(t *testing.T) {
	ctx := context.Background()
	store := newStore(t)
	initial, err := store.Preferences(ctx)
	if err != nil {
		t.Fatal(err)
	}
	// Hold the sole configured connection so cancellation must also unwind a
	// pending transaction admission without a write or detached callback.
	held, err := store.db.BeginTx(ctx, nil)
	if err != nil {
		t.Fatal(err)
	}
	defer held.Rollback()
	canceled, cancel := context.WithCancel(ctx)
	defer cancel()
	done := make(chan error, 1)
	theme := "dark"
	go func() {
		_, err := store.UpdatePreferences(canceled, hub.PreferenceUpdate{ExpectedRevision: initial.Revision, Theme: &theme})
		done <- err
	}()
	cancel()
	select {
	case err = <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal("cancellation lost", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("canceled preference update did not finish")
	}
	if err = held.Rollback(); err != nil {
		t.Fatal(err)
	}
	if value, err := store.Preferences(canceled); !errors.Is(err, context.Canceled) || value != (hub.Preferences{}) {
		t.Fatalf("canceled read returned %+v: %v", value, err)
	}
	actual, err := store.Preferences(ctx)
	if err != nil || actual != initial {
		t.Fatalf("canceled update changed state: %+v %v", actual, err)
	}
}

func TestPreferencesVersionOneUpgradePreservesProductAndNativeFacts(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	db := preferencesVersionOneDatabase(t, dir)
	old := &Store{db: db}
	session, err := old.CreateSession(ctx, "existing session", t.TempDir(), target("fixture"))
	if err != nil {
		t.Fatal(err)
	}
	claim := preferencesVersionOneClaim(t, old, session, "existing prompt")
	complete(t, old, claim, "existing-native-session")
	detail, err := old.Detail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	history, err := old.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	native, err := private.Open(ctx, dir, "session.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = native.ExecContext(ctx, `CREATE TABLE native_facts(data BLOB); INSERT INTO native_facts VALUES('original native body')`); err != nil {
		native.Close()
		t.Fatal(err)
	}
	if err = native.Close(); err != nil {
		t.Fatal(err)
	}
	nativePath := filepath.Join(dir, "session.sqlite")
	beforeNative, err := os.ReadFile(nativePath)
	if err != nil {
		t.Fatal(err)
	}
	store, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	preferences, err := store.Preferences(ctx)
	if err != nil || preferences != hub.DefaultPreferences() {
		t.Fatalf("upgrade preferences: %+v %v", preferences, err)
	}
	upgradedDetail, err := store.Detail(ctx, session.ID)
	if err != nil || !reflect.DeepEqual(upgradedDetail, detail) {
		t.Fatalf("product/native identity changed: %+v %v", upgradedDetail, err)
	}
	upgradedHistory, err := store.History(ctx, session.ID, 0, 128)
	if err != nil || !reflect.DeepEqual(upgradedHistory, history) {
		t.Fatalf("product history changed: %+v %v", upgradedHistory, err)
	}
	afterNative, err := os.ReadFile(nativePath)
	if err != nil || !bytes.Equal(beforeNative, afterNative) {
		t.Fatalf("upgrade modified the native facts database: %v", err)
	}
}

func TestPreferencesMigrationFailureRollsBackSchemaAndVersion(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	db := preferencesVersionOneDatabase(t, dir)
	// The collision forces migration failure after the earlier schema steps.
	// An absent old index proves those steps are rolled back with the version.
	if _, err := db.ExecContext(ctx, `DROP INDEX hub_events_turn; CREATE TABLE hub_preferences(data TEXT); INSERT INTO hub_preferences VALUES('retain collision')`); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	store, err := Open(ctx, dir)
	if store != nil {
		store.Close()
	}
	if err == nil {
		t.Fatal("colliding migration succeeded")
	}
	db, err = private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, addedIndex int
	var original string
	if err = db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE name='hub_events_turn'`).Scan(&addedIndex); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(ctx, `SELECT data FROM hub_preferences`).Scan(&original); err != nil || version != 1 || addedIndex != 0 || original != "retain collision" {
		t.Fatalf("migration partially committed: version=%d index=%d original=%q %v", version, addedIndex, original, err)
	}
}

func TestPreferencesUnknownSchemaIsNotModified(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	db, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = db.ExecContext(ctx, `CREATE TABLE future_facts(data TEXT); INSERT INTO future_facts VALUES('retain future'); PRAGMA user_version=9`); err != nil {
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
		t.Fatal("future schema was adopted")
	}
	db, err = private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, added int
	var original string
	if err = db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE name LIKE 'hub_%'`).Scan(&added); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(ctx, `SELECT data FROM future_facts`).Scan(&original); err != nil || version != 9 || added != 0 || original != "retain future" {
		t.Fatalf("future schema changed: version=%d added=%d original=%q %v", version, added, original, err)
	}
}

func TestPreferencesCorruptionAndMissingStateNeverReset(t *testing.T) {
	for _, fixture := range []struct {
		name string
		sql  string
	}{
		{"missing row", `DELETE FROM hub_preferences`},
		{"missing table", `DROP TABLE hub_preferences`},
		{"malformed record", `UPDATE hub_preferences SET data='{'`},
		{"unsupported value", `UPDATE hub_preferences SET data=json_set(data,'$.theme','unknown')`},
		{"mismatched revision", `UPDATE hub_preferences SET revision=2`},
	} {
		t.Run(fixture.name, func(t *testing.T) {
			ctx := context.Background()
			dir := filepath.Join(t.TempDir(), "private")
			store, err := Open(ctx, dir)
			if err != nil {
				t.Fatal(err)
			}
			if _, err = store.db.ExecContext(ctx, fixture.sql); err != nil {
				store.Close()
				t.Fatal(err)
			}
			if value, err := store.Preferences(ctx); err == nil || errors.Is(err, hub.ErrInvalidPreferences) || value != (hub.Preferences{}) {
				t.Fatalf("invalid durable read returned %+v: %v", value, err)
			}
			theme := "dark"
			if value, err := store.UpdatePreferences(ctx, hub.PreferenceUpdate{ExpectedRevision: 1, Theme: &theme}); err == nil || errors.Is(err, hub.ErrInvalidPreferences) || value != (hub.Preferences{}) {
				t.Fatalf("invalid state was overwritten: %+v %v", value, err)
			}
			if err = store.Close(); err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(dir, "product.sqlite")
			before, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			store, err = Open(ctx, dir)
			if store != nil {
				store.Close()
			}
			if err == nil {
				t.Fatal("invalid version-two state was accepted on reopen")
			}
			after, err := os.ReadFile(path)
			if err != nil || !bytes.Equal(before, after) {
				t.Fatalf("failed open modified corrupt database: %v", err)
			}
		})
	}
}

func preferencesVersionOneDatabase(t *testing.T, dir string) *sql.DB {
	t.Helper()
	db, err := private.Open(context.Background(), dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Close() })
	// Frozen pre-preferences schema: do not create a v2 database then lower its
	// version, which would conceal migration dependencies on the new table.
	const versionOne = `
CREATE TABLE hub_sessions(id TEXT PRIMARY KEY,status TEXT NOT NULL,updated INTEGER NOT NULL,data BLOB NOT NULL);
CREATE TABLE hub_branches(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),route TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(session,route));
CREATE INDEX hub_native_route ON hub_branches(json_extract(data,'$.target.engine_id'),json_extract(data,'$.native_session_id'));
CREATE TABLE hub_turns(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),seq INTEGER NOT NULL,status TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(session,seq));
CREATE TABLE hub_attempts(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),turn TEXT NOT NULL REFERENCES hub_turns(id),status TEXT NOT NULL,data BLOB NOT NULL);
CREATE TABLE hub_events(seq INTEGER PRIMARY KEY AUTOINCREMENT,session TEXT NOT NULL REFERENCES hub_sessions(id),turn TEXT REFERENCES hub_turns(id),attempt TEXT REFERENCES hub_attempts(id),data BLOB NOT NULL);
CREATE INDEX hub_events_session ON hub_events(session,seq);
CREATE INDEX hub_events_turn ON hub_events(turn,seq);
CREATE TABLE hub_fallback(session TEXT PRIMARY KEY REFERENCES hub_sessions(id),data BLOB NOT NULL);
CREATE TABLE hub_handoffs(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),data BLOB NOT NULL);
CREATE TABLE hub_settings(session TEXT PRIMARY KEY REFERENCES hub_sessions(id),data BLOB NOT NULL);
CREATE TABLE hub_teams(id TEXT PRIMARY KEY,lead TEXT NOT NULL REFERENCES hub_sessions(id),status TEXT NOT NULL,data BLOB NOT NULL);
CREATE UNIQUE INDEX hub_team_active_lead ON hub_teams(lead) WHERE status='active';
CREATE TABLE hub_workers(id TEXT PRIMARY KEY,team TEXT NOT NULL REFERENCES hub_teams(id),session TEXT NOT NULL UNIQUE REFERENCES hub_sessions(id),state TEXT NOT NULL,name TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(team,name));
CREATE TABLE hub_worker_tasks(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,team TEXT NOT NULL REFERENCES hub_teams(id),worker TEXT NOT NULL REFERENCES hub_workers(id),status TEXT NOT NULL,turn TEXT UNIQUE REFERENCES hub_turns(id),data BLOB NOT NULL);
CREATE INDEX hub_worker_queue ON hub_worker_tasks(worker,status,seq);
CREATE TABLE hub_automations(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),enabled INTEGER NOT NULL,next INTEGER NOT NULL,running TEXT NOT NULL,data BLOB NOT NULL);
CREATE INDEX hub_automation_due ON hub_automations(enabled,running,next);
PRAGMA user_version=1;`
	if _, err = db.ExecContext(context.Background(), versionOne); err != nil {
		t.Fatal(err)
	}
	return db
}

// Frozen v1 facts must not invoke today's admission recipe, which requires
// newer settings tables. This exercises migration of an actual old record.
func preferencesVersionOneClaim(t *testing.T, store *Store, session hub.Session, prompt string) hub.Claim {
	t.Helper()
	detail, err := store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	var branch hub.Branch
	for _, current := range detail.Branches {
		if current.ID == session.ActiveBranchID {
			branch = current
		}
	}
	turnID, err := hub.NewID("turn-")
	if err != nil {
		t.Fatal(err)
	}
	attemptID, err := hub.NewID("attempt-")
	if err != nil {
		t.Fatal(err)
	}
	stamp := now()
	turn := hub.Turn{ID: turnID, SessionID: session.ID, Sequence: 1, Prompt: prompt, Status: hub.Running, ActiveAttemptID: attemptID, CreatedAt: stamp}
	attempt := hub.Attempt{ID: attemptID, TurnID: turnID, SessionID: session.ID, BranchID: branch.ID, Target: branch.Target, Status: hub.Running, CreatedAt: stamp}
	session.Status, session.UpdatedAt = hub.Running, stamp
	err = store.transaction(t.Context(), func(tx *sql.Tx) error {
		turnData, err := body(turn)
		if err != nil {
			return err
		}
		attemptData, err := body(attempt)
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(t.Context(), `INSERT INTO hub_turns(id,session,seq,status,data) VALUES(?,?,?,?,?)`, turn.ID, session.ID, turn.Sequence, turn.Status, turnData); err != nil {
			return err
		}
		if _, err = tx.ExecContext(t.Context(), `INSERT INTO hub_attempts(id,session,turn,status,data) VALUES(?,?,?,?,?)`, attempt.ID, session.ID, turn.ID, attempt.Status, attemptData); err != nil {
			return err
		}
		return writeSession(t.Context(), tx, session)
	})
	if err != nil {
		t.Fatal(err)
	}
	return hub.Claim{Session: session, Branch: branch, Turn: turn, Attempt: attempt}
}
