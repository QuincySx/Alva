// INPUT: Real temporary private SQLite, explicit synthetic credentials and shared connection edits.
// OUTPUT: CRUD/reopen, request replay/recovery, atomic credential/receipt failures and v2/v6-to-v7 migration acceptance.
// POS: [L7 SQLite adapter] Exercises durable connection facts without real accounts or network requests.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func connectionFixture() hub.ConnectionUpdate {
	return hub.ConnectionUpdate{Name: "Explicit fixture", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:32123/v1/chat/completions", Auth: "api_key", APIKey: "synthetic-connection-key", Enabled: true, Models: []hub.ConnectionModel{{ID: "exact-model", Name: "Fixture model", Enabled: true}}, Headers: map[string]string{"X-Fixture": "synthetic-private-header"}, ReplaceHeaders: true}
}
func connectionUpdate(c hub.ModelConnection) hub.ConnectionUpdate {
	return hub.ConnectionUpdate{ID: c.ID, ExpectedRevision: c.Revision, Name: c.Name, Protocol: c.Protocol, Endpoint: c.Endpoint, Auth: c.Auth, Enabled: c.Enabled, Models: c.Models}
}

func TestConnectionsCRUDReopenAndRedactedMetadata(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	u := connectionFixture()
	created, err := s.SaveConnection(ctx, u)
	if err != nil {
		t.Fatal(err)
	}
	if created.ID == "" || created.Revision != 1 || !created.HasAPIKey {
		t.Fatal("missing committed identity/credential marker")
	}
	metadata, err := s.Connections(ctx)
	if err != nil || len(metadata) != 1 {
		t.Fatal("list", err)
	}
	data, _ := json.Marshal(metadata)
	if strings.Contains(string(data), u.APIKey) || strings.Contains(string(data), u.Headers["X-Fixture"]) {
		t.Fatal("list exposed private values")
	}
	var stored string
	if err = s.db.QueryRowContext(ctx, `SELECT data FROM hub_connections WHERE id=?`, created.ID).Scan(&stored); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(stored, u.APIKey) || strings.Contains(stored, u.Headers["X-Fixture"]) {
		t.Fatal("private values entered ordinary metadata")
	}
	edit := connectionUpdate(created)
	edit.Name = "Edited"
	edited, err := s.SaveConnection(ctx, edit)
	if err != nil || edited.ID != created.ID || edited.Revision != 2 {
		t.Fatal("edit", err)
	}
	secret, err := s.ConnectionSecrets(ctx, edited.ID, edited.Revision)
	if err != nil || secret.APIKey != u.APIKey || secret.Headers["X-Fixture"] != u.Headers["X-Fixture"] {
		t.Fatal("blank edit did not preserve saved credential")
	}
	if _, err = s.SaveConnection(ctx, edit); !errors.Is(err, hub.ErrConnectionConflict) {
		t.Fatal("stale edit accepted", err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err = s.Connections(ctx)
	if err != nil || len(metadata) != 1 || metadata[0].ID != created.ID || metadata[0].Name != "Edited" {
		t.Fatal("reopen lost connection", err)
	}
	if _, err = s.ConnectionSecrets(ctx, created.ID, 1); !errors.Is(err, hub.ErrConnectionConflict) {
		t.Fatal("old route resolved changed credentials", err)
	}
	edit = connectionUpdate(edited)
	edit.Auth = "none"
	edit.ClearAPIKey = true
	edit.ReplaceHeaders = true
	cleared, err := s.SaveConnection(ctx, edit)
	if err != nil || cleared.HasAPIKey || len(cleared.HeaderNames) != 0 {
		t.Fatal("explicit clear failed", err)
	}
	if err = s.DeleteConnection(ctx, created.ID, edited.Revision); !errors.Is(err, hub.ErrConnectionConflict) {
		t.Fatal("stale delete accepted", err)
	}
	if err = s.DeleteConnection(ctx, created.ID, cleared.Revision); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ConnectionSecrets(ctx, created.ID, cleared.Revision); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("deleted credential remained available", err)
	}
	metadata, err = s.Connections(ctx)
	if err != nil || len(metadata) != 0 {
		t.Fatal("deleted connection listed", err)
	}
}

func TestConnectionsConcurrentRevisionAndAtomicCredentialFailure(t *testing.T) {
	ctx := t.Context()
	s := newStore(t)
	created, err := s.SaveConnection(ctx, connectionFixture())
	if err != nil {
		t.Fatal(err)
	}
	start := make(chan struct{})
	results := make(chan error, 2)
	var ready sync.WaitGroup
	ready.Add(2)
	for _, name := range []string{"one", "two"} {
		go func() {
			ready.Done()
			<-start
			u := connectionUpdate(created)
			u.Name = name
			_, err := s.SaveConnection(ctx, u)
			results <- err
		}()
	}
	ready.Wait()
	close(start)
	wins, conflicts := 0, 0
	for range 2 {
		err := <-results
		if err == nil {
			wins++
		} else if errors.Is(err, hub.ErrConnectionConflict) {
			conflicts++
		} else {
			t.Fatal(err)
		}
	}
	if wins != 1 || conflicts != 1 {
		t.Fatal("revision admitted competing writes")
	}
	list, err := s.Connections(ctx)
	if err != nil {
		t.Fatal(err)
	}
	before := list[0]
	if _, err = s.db.ExecContext(ctx, `CREATE TRIGGER fixture_credential_failure BEFORE UPDATE ON hub_connection_credentials BEGIN SELECT RAISE(ABORT,'fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	u := connectionUpdate(before)
	u.Name = "must not commit"
	u.APIKey = "synthetic-replacement-key"
	if _, err = s.SaveConnection(ctx, u); err == nil {
		t.Fatal("injected credential failure succeeded")
	}
	list, err = s.Connections(ctx)
	if err != nil || list[0].Revision != before.Revision || list[0].Name != before.Name {
		t.Fatal("metadata partially committed", err)
	}
	secret, err := s.ConnectionSecrets(ctx, before.ID, before.Revision)
	if err != nil || secret.APIKey != connectionFixture().APIKey {
		t.Fatal("credential partially committed", err)
	}
	canceled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err = s.SaveConnection(canceled, u); !errors.Is(err, context.Canceled) {
		t.Fatal("canceled save committed", err)
	}
}

func TestConnectionsVersionTwoMigrationAndCorruptionAreNonDestructive(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "v2")
	db := preferencesVersionOneDatabase(t, dir)
	p := hub.DefaultPreferences()
	data, _ := json.Marshal(p)
	if _, err := db.ExecContext(ctx, `CREATE TABLE hub_preferences(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL)`); err != nil {
		t.Fatal(err)
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO hub_preferences(id,revision,data) VALUES(1,1,?)`, data); err != nil {
		t.Fatal(err)
	}
	if _, err := db.ExecContext(ctx, `PRAGMA user_version=2`); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	list, err := s.Connections(ctx)
	if err != nil || len(list) != 0 {
		t.Fatal("v2 did not migrate to empty explicit connection set", err)
	}
	saved, err := s.SaveConnection(ctx, connectionFixture())
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(ctx, `DELETE FROM hub_connection_credentials WHERE connection=?`, saved.ID); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if reopened, err := Open(ctx, dir); err == nil {
		_ = reopened.Close()
		t.Fatal("corrupt credential table was reset on reopen")
	}
	db, err = private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var count int
	if err = db.QueryRowContext(ctx, `SELECT count(*) FROM hub_connections WHERE id=?`, saved.ID).Scan(&count); err != nil || count != 1 {
		t.Fatal("corrupt database was modified", err)
	}
}

func TestConnectionsMigrationFailureRollsBackVersionAndNewTables(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "migration-failure")
	db := preferencesVersionOneDatabase(t, dir)
	data, _ := json.Marshal(hub.DefaultPreferences())
	for _, statement := range []string{
		`CREATE TABLE hub_preferences(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL)`,
		`CREATE TABLE hub_connection_credentials(data TEXT)`,
		`INSERT INTO hub_connection_credentials VALUES('retain collision evidence')`,
		`PRAGMA user_version=2`,
	} {
		if _, err := db.ExecContext(ctx, statement); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := db.ExecContext(ctx, `INSERT INTO hub_preferences VALUES(1,1,?)`, data); err != nil {
		t.Fatal(err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	if opened, err := Open(ctx, dir); err == nil {
		_ = opened.Close()
		t.Fatal("colliding schema was overwritten")
	}
	db, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, created int
	var original string
	if err = db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE name='hub_connections'`).Scan(&created); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(ctx, `SELECT data FROM hub_connection_credentials`).Scan(&original); err != nil || version != 2 || created != 0 || original != "retain collision evidence" {
		t.Fatal("migration partially committed or replaced original data", err)
	}
}

func TestConnectionCreationReplayResolveEditDeleteAndRestart(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "recoverable")
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	request := hub.ConnectionCreate{RequestID: "fixture-stable-create", Update: connectionFixture()}
	created, err := s.CreateConnection(t.Context(), request)
	if err != nil {
		t.Fatal(err)
	}
	again, err := s.CreateConnection(t.Context(), request)
	if err != nil || !reflect.DeepEqual(created, again) {
		t.Fatal("exact retry did not return its original receipt", err)
	}
	var metadata string
	if err = s.db.QueryRowContext(t.Context(), `SELECT data FROM hub_connection_creations WHERE request=?`, request.RequestID).Scan(&metadata); err != nil || strings.Contains(metadata, request.Update.APIKey) || strings.Contains(metadata, request.Update.Headers["X-Fixture"]) {
		t.Fatal("recovery metadata contained private credential values", err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	receipt, err := s.ConnectionCreation(t.Context(), request.RequestID)
	if err != nil || receipt.Current == nil || !reflect.DeepEqual(receipt.Created, created) || !reflect.DeepEqual(*receipt.Current, created) {
		t.Fatal("restart could not resolve the original stable identity", err)
	}
	for _, change := range []func(*hub.ConnectionUpdate){
		func(u *hub.ConnectionUpdate) { u.Name = "changed payload" },
		func(u *hub.ConnectionUpdate) { u.APIKey = "different-synthetic-key" },
		func(u *hub.ConnectionUpdate) { u.Headers["X-Fixture"] = "different-synthetic-header" },
		func(u *hub.ConnectionUpdate) { u.Models[0].Enabled = false },
	} {
		changed := hub.ConnectionCreate{RequestID: request.RequestID, Update: connectionFixture()}
		change(&changed.Update)
		if _, err = s.CreateConnection(t.Context(), changed); !errors.Is(err, hub.ErrConnectionCreationConflict) {
			t.Fatal("changed payload reused an existing creation", err)
		}
	}
	edit := connectionUpdate(created)
	edit.Name = "other window edit"
	edited, err := s.SaveConnection(t.Context(), edit)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.CreateConnection(t.Context(), request); !errors.Is(err, hub.ErrConnectionCreationConflict) {
		t.Fatal("retry accepted a revised creation", err)
	}
	receipt, err = s.ConnectionCreation(t.Context(), request.RequestID)
	if err != nil || !reflect.DeepEqual(receipt.Created, created) || receipt.Current == nil || receipt.Current.Revision != edited.Revision {
		t.Fatal("resolve replaced the immutable original or lost current revision", err)
	}
	if err = s.DeleteConnection(t.Context(), created.ID, edited.Revision); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	receipt, err = s.ConnectionCreation(t.Context(), request.RequestID)
	if err != nil || receipt.Current != nil || !reflect.DeepEqual(receipt.Created, created) {
		t.Fatal("deleted creation lost its exact recovery identity", err)
	}
	if _, err = s.CreateConnection(t.Context(), request); !errors.Is(err, hub.ErrConnectionCreationConflict) {
		t.Fatal("retry recreated a deliberately deleted connection", err)
	}
	for _, table := range []string{"hub_connections", "hub_connection_credentials"} {
		var count int
		if err = s.db.QueryRowContext(t.Context(), `SELECT count(*) FROM `+table).Scan(&count); err != nil || count != 0 {
			t.Fatal("delete retained credentials or retry recreated an entity", table, count, err)
		}
	}
}

func TestConnectionCreationLedgerAndCredentialsCommitAtomically(t *testing.T) {
	for _, statement := range []string{
		`CREATE TRIGGER reject_creation BEFORE INSERT ON hub_connection_creations BEGIN SELECT RAISE(ABORT,'creation fixture failure'); END`,
		`CREATE TRIGGER reject_creation BEFORE INSERT ON hub_connection_credentials BEGIN SELECT RAISE(ABORT,'credential fixture failure'); END`,
	} {
		s := newStore(t)
		if _, err := s.db.ExecContext(t.Context(), statement); err != nil {
			t.Fatal(err)
		}
		request := hub.ConnectionCreate{RequestID: "fixture-atomic-create", Update: connectionFixture()}
		if _, err := s.CreateConnection(t.Context(), request); err == nil {
			t.Fatal("injected partial creation claimed success")
		}
		for _, table := range []string{"hub_connections", "hub_connection_credentials", "hub_connection_creations"} {
			var count int
			if err := s.db.QueryRowContext(t.Context(), `SELECT count(*) FROM `+table).Scan(&count); err != nil || count != 0 {
				t.Fatal("creation committed only part of its entity/credential/receipt", table, count, err)
			}
		}
		if _, err := s.db.ExecContext(t.Context(), `DROP TRIGGER reject_creation`); err != nil {
			t.Fatal(err)
		}
		start, results := make(chan struct{}), make(chan hub.ModelConnection, 2)
		failures := make(chan error, 2)
		for range 2 {
			go func() {
				<-start
				value, err := s.CreateConnection(t.Context(), request)
				results <- value
				failures <- err
			}()
		}
		close(start)
		first, second := <-results, <-results
		if err := <-failures; err != nil {
			t.Fatal(err)
		}
		if err := <-failures; err != nil || !reflect.DeepEqual(first, second) || first.ID == "" {
			t.Fatal("competing exact requests did not converge on one receipt", err)
		}
	}
}

func connectionsVersionSixDatabase(t *testing.T, directory string) (*sql.DB, hub.ModelConnection) {
	t.Helper()
	db, connection, _ := composerVersionFiveDatabase(t, directory)
	tx, err := db.BeginTx(t.Context(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	if err = composerDraftsSchema(t.Context(), tx); err != nil {
		t.Fatal(err)
	}
	if err = composerFilesSchema(t.Context(), tx); err != nil {
		t.Fatal(err)
	}
	if _, err = tx.ExecContext(t.Context(), `PRAGMA user_version=6`); err != nil {
		t.Fatal(err)
	}
	if err = tx.Commit(); err != nil {
		t.Fatal(err)
	}
	return db, connection
}

func TestConnectionCreationSchemaSevenMigrationAndFailureBeforeRecovery(t *testing.T) {
	for _, broken := range []bool{false, true} {
		directory := filepath.Join(t.TempDir(), "v6")
		db, original := connectionsVersionSixDatabase(t, directory)
		legacy := &Store{db: db}
		session, err := legacy.CreateSession(t.Context(), "retain running authority", t.TempDir(), target("a"))
		if err != nil {
			t.Fatal(err)
		}
		if _, err = legacy.BeginTurn(t.Context(), session.ID, "fixture running turn"); err != nil {
			t.Fatal(err)
		}
		if broken {
			if _, err = db.ExecContext(t.Context(), `CREATE TABLE hub_connection_creations(foreign_fact TEXT); INSERT INTO hub_connection_creations VALUES('retain foreign fact')`); err != nil {
				t.Fatal(err)
			}
		}
		if err = db.Close(); err != nil {
			t.Fatal(err)
		}
		s, err := Open(t.Context(), directory)
		if broken {
			if err == nil {
				s.Close()
				t.Fatal("migration replaced a colliding authority")
			}
			db, err = private.Open(t.Context(), directory, "product.sqlite")
			if err != nil {
				t.Fatal(err)
			}
			var version int
			var status, fact string
			if err = db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil {
				t.Fatal(err)
			}
			if err = db.QueryRowContext(t.Context(), `SELECT status FROM hub_sessions WHERE id=?`, session.ID).Scan(&status); err != nil {
				t.Fatal(err)
			}
			if err = db.QueryRowContext(t.Context(), `SELECT foreign_fact FROM hub_connection_creations`).Scan(&fact); err != nil || version != 6 || status != string(hub.Running) || fact != "retain foreign fact" {
				t.Fatal("failed migration changed facts/version or ran execution recovery", version, status, fact, err)
			}
			db.Close()
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		var version int
		if err = s.db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil || version != 8 {
			t.Fatal("v6 did not atomically migrate to the current schema", version, err)
		}
		secret, err := s.ConnectionSecrets(t.Context(), original.ID, original.Revision)
		if err != nil || secret.APIKey != connectionFixture().APIKey {
			t.Fatal("migration changed existing credentials", err)
		}
		if _, err = s.ConnectionCreation(t.Context(), "never-issued-request"); !errors.Is(err, hub.ErrNotFound) {
			t.Fatal("migration fabricated a request for an old connection", err)
		}
		s.Close()
	}
}

func TestConnectionCreationCurrentSchemaRequiresIdentityConstraintsBeforeRecovery(t *testing.T) {
	for _, tc := range []struct{ name, schema string }{
		{"missing", `CREATE TABLE hub_connection_creations(request TEXT,connection TEXT,data BLOB)`},
		{"composite-request", `CREATE TABLE hub_connection_creations(request TEXT,connection TEXT NOT NULL UNIQUE,data BLOB NOT NULL,PRIMARY KEY(request,data))`},
		{"partial-connection", `CREATE TABLE hub_connection_creations(request TEXT PRIMARY KEY,connection TEXT NOT NULL,data BLOB NOT NULL); CREATE UNIQUE INDEX partial_creation_connection ON hub_connection_creations(connection) WHERE request='only-one-request'`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			directory := filepath.Join(t.TempDir(), "broken-v7")
			s, err := Open(t.Context(), directory)
			if err != nil {
				t.Fatal(err)
			}
			session, err := s.CreateSession(t.Context(), "retain running fact", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			if _, err = s.BeginTurn(t.Context(), session.ID, "fixture"); err != nil {
				t.Fatal(err)
			}
			if _, err = s.db.ExecContext(t.Context(), `DROP TABLE hub_connection_creations; `+tc.schema); err != nil {
				t.Fatal(err)
			}
			if err = s.Close(); err != nil {
				t.Fatal(err)
			}
			if bad, err := Open(t.Context(), directory); err == nil {
				bad.Close()
				t.Fatal("current schema without atomic request constraints was accepted")
			}
			db, err := private.Open(t.Context(), directory, "product.sqlite")
			if err != nil {
				t.Fatal(err)
			}
			defer db.Close()
			var status string
			if err = db.QueryRowContext(t.Context(), `SELECT status FROM hub_sessions WHERE id=?`, session.ID).Scan(&status); err != nil || status != string(hub.Running) {
				t.Fatal("invalid identity schema ran execution recovery", status, err)
			}
		})
	}
}
