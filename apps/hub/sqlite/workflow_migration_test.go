// INPUT: Frozen schema-four facts, real private SQLite and current product migrations.
// OUTPUT: Atomic v4-to-v5 preservation, rollback and validation-before-recovery regressions.
// POS: [L7 SQLite adapter tests] Never touches user databases or substitutes new defaults after failed migration.
package sqlite

import (
	"database/sql"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func workflowVersionFourDatabase(t *testing.T, directory string) (*sql.DB, hub.ModelConnection, hub.Goal) {
	t.Helper()
	db, connection := goalsVersionThreeDatabase(t, directory)
	tx, err := db.BeginTx(t.Context(), nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, statement := range []string{
		`CREATE TABLE hub_goals(id TEXT PRIMARY KEY,request TEXT NOT NULL UNIQUE,created INTEGER NOT NULL,scope_kind TEXT NOT NULL,workspace TEXT NOT NULL,session TEXT NOT NULL,data BLOB NOT NULL)`,
		`CREATE INDEX hub_goal_scope ON hub_goals(scope_kind,workspace,session,created,id)`,
	} {
		if _, err = tx.ExecContext(t.Context(), statement); err != nil {
			t.Fatal(err)
		}
	}
	if err = modelPreferencesSchema(t.Context(), tx); err != nil {
		t.Fatal(err)
	}
	if _, err = tx.ExecContext(t.Context(), `PRAGMA user_version=4`); err != nil {
		t.Fatal(err)
	}
	if err = tx.Commit(); err != nil {
		t.Fatal(err)
	}
	legacy := &Store{db: db}
	goal, err := legacy.CreateGoal(t.Context(), goalRequest(t))
	if err != nil {
		t.Fatal(err)
	}
	key := hub.ModelPreferenceKey(connection.Target(connection.Models[0]))
	if _, err = legacy.SaveModelPreference(t.Context(), hub.ModelPreference{Key: key, Favorite: true}); err != nil {
		t.Fatal(err)
	}
	return db, connection, goal.Goal
}

func TestWorkflowSchemaFivePreservesVersionFourFacts(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "product")
	db, connection, goal := workflowVersionFourDatabase(t, directory)
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	store, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	var version int
	if err = store.db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil || version != 8 {
		t.Fatal(version, err)
	}
	loaded, err := store.Goal(t.Context(), goal.ID)
	if err != nil || loaded != goal {
		t.Fatal("goal changed during migration", err)
	}
	secrets, err := store.ConnectionSecrets(t.Context(), connection.ID, connection.Revision)
	if err != nil || secrets.APIKey != connectionFixture().APIKey {
		t.Fatal("connection credential was not preserved", err)
	}
	preferences, err := store.ModelPreferences(t.Context())
	if err != nil || len(preferences.Entries) != 1 || !preferences.Entries[0].Favorite {
		t.Fatal("model preferences were replaced", err)
	}
	profiles, err := store.Companions(t.Context(), hub.LocalCompanionOwner)
	if err != nil || len(profiles) != 0 {
		t.Fatal("migration invented a companion", err)
	}
}

func TestWorkflowSchemaFiveFailureRollsBackEveryNewAuthority(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "product")
	db, _, goal := workflowVersionFourDatabase(t, directory)
	if _, err := db.ExecContext(t.Context(), `CREATE TABLE hub_workspace_views(foreign_fact TEXT)`); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	if store, err := Open(t.Context(), directory); err == nil {
		store.Close()
		t.Fatal("conflicting authority was silently replaced")
	}
	db, err := private.Open(t.Context(), directory, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, companions, goals int
	if err = db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(t.Context(), `SELECT count(*) FROM sqlite_master WHERE type='table' AND name='hub_companions'`).Scan(&companions); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_goals WHERE id=?`, goal.ID).Scan(&goals); err != nil {
		t.Fatal(err)
	}
	if version != 4 || companions != 0 || goals != 1 {
		t.Fatal("failed migration left partial schema or lost prior facts", version, companions, goals)
	}
}

func TestWorkflowSchemaValidationPrecedesExecutionRecovery(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "product")
	store, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	session, err := store.CreateSession(t.Context(), "retain running fact", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.BeginTurn(t.Context(), session.ID, "explicit fixture"); err != nil {
		t.Fatal(err)
	}
	if _, err = store.db.ExecContext(t.Context(), `DROP TABLE hub_workspace_views`); err != nil {
		t.Fatal(err)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	if bad, openErr := Open(t.Context(), directory); openErr == nil {
		bad.Close()
		t.Fatal("damaged current authority was repaired silently")
	}
	db, err := private.Open(t.Context(), directory, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var status string
	if err = db.QueryRowContext(t.Context(), `SELECT status FROM hub_sessions WHERE id=?`, session.ID).Scan(&status); err != nil {
		t.Fatal(err)
	}
	if status != string(hub.Running) {
		t.Fatal("failed open mutated execution facts before validating storage", status)
	}
}
