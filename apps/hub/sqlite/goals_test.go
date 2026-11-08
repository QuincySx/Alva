// INPUT: Real temporary SQLite, immutable Goal requests and frozen schema-three fixtures.
// OUTPUT: Durable scope-bound keyset pagination, idempotent creation and non-destructive migration/failure regressions.
// POS: [L7 SQLite adapter tests] Proves committed Goal facts independently of execution or model accounts.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func TestGoalsPagesReachOlderThanListLimitAndPinScopeAcrossReopen(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "paged-goals")
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Close() })
	request := goalRequest(t)
	want := make(map[string]bool, 125)
	for index := range 125 {
		request.RequestID = fmt.Sprintf("goal-request-%03d", index)
		receipt, err := s.CreateGoal(t.Context(), request)
		if err != nil {
			t.Fatal(err)
		}
		want[receipt.Goal.ID] = true
	}
	first, err := s.PageGoals(t.Context(), request.Scope, hub.GoalCursor{}, 20)
	if err != nil || len(first.Entries) != 20 || !first.HasMore || first.Next.Scope != request.Scope {
		t.Fatal("first scoped page", first, err)
	}
	for _, goal := range first.Entries {
		delete(want, goal.ID)
	}
	// A newer insert must not shift or duplicate the opened descending pages.
	request.RequestID = "new-after-first-page"
	if _, err = s.CreateGoal(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	cursor := first.Next
	for {
		page, err := s.PageGoals(t.Context(), request.Scope, cursor, 20)
		if err != nil || len(page.Entries) > 20 {
			t.Fatal("continued page", page, err)
		}
		for _, goal := range page.Entries {
			if !want[goal.ID] {
				t.Fatal("page repeated or admitted a newer Goal", goal.ID)
			}
			delete(want, goal.ID)
		}
		if !page.HasMore {
			break
		}
		cursor = page.Next
	}
	if len(want) != 0 {
		t.Fatal("pagination hid older saved Goals", len(want))
	}
	foreign := request.Scope
	foreign.Workspace = t.TempDir()
	if _, err = s.PageGoals(t.Context(), foreign, first.Next, 20); err == nil {
		t.Fatal("cursor crossed project scope")
	}
	bad := first.Next
	bad.Created = 0
	if _, err = s.PageGoals(t.Context(), request.Scope, bad, 20); err == nil {
		t.Fatal("partial cursor accepted")
	}
}

func goalRequest(t *testing.T) hub.GoalCreate {
	t.Helper()
	return hub.GoalCreate{RequestID: "explicit-goal-request", Name: "Ship scoped goal", CompletionCriteria: "The named regression passes and the original source is retained.", Scope: hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: t.TempDir()}}
}

func TestGoalsDurableIdempotentReceiptAndCompetingCreates(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	request := goalRequest(t)
	type result struct {
		receipt hub.GoalReceipt
		err     error
	}
	results := make(chan result, 8)
	var ready sync.WaitGroup
	ready.Add(8)
	start := make(chan struct{})
	for range 8 {
		go func() {
			ready.Done()
			<-start
			receipt, err := s.CreateGoal(t.Context(), request)
			results <- result{receipt, err}
		}()
	}
	ready.Wait()
	close(start)
	var saved hub.Goal
	created := 0
	for range 8 {
		result := <-results
		if result.err != nil {
			t.Fatal(result.err)
		}
		if !result.receipt.Replayed {
			created++
		}
		if saved.ID != "" && saved != result.receipt.Goal {
			t.Fatal("same request changed stable fact")
		}
		saved = result.receipt.Goal
	}
	if created != 1 || saved.Revision != 1 || saved.ID == "" {
		t.Fatal("creation receipt", created, saved)
	}
	changed := request
	changed.Name = "different request body"
	if receipt, err := s.CreateGoal(t.Context(), changed); !errors.Is(err, hub.ErrGoalConflict) || receipt != (hub.GoalReceipt{}) {
		t.Fatal("changed idempotency request admitted", receipt, err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	got, err := s.Goal(t.Context(), saved.ID)
	if err != nil || got != saved {
		t.Fatal("reopen lost Goal", got, err)
	}
	receipt, err := s.CreateGoal(t.Context(), request)
	if err != nil || !receipt.Replayed || receipt.Goal != saved {
		t.Fatal("reopened receipt changed", receipt, err)
	}
	list, err := s.ListGoals(t.Context(), request.Scope, 100)
	if err != nil || len(list) != 1 || list[0] != saved {
		t.Fatal("scope list", list, err)
	}
}

func TestGoalsScopeValidationCancellationAndCommitFailure(t *testing.T) {
	s := newStore(t)
	request := goalRequest(t)
	a, err := s.CreateSession(t.Context(), "scope owner", request.Scope.Workspace, target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	request.Scope = hub.GoalScope{Kind: hub.GoalSessionScope, SessionID: a.ID, Workspace: t.TempDir()}
	if _, err = s.CreateGoal(t.Context(), request); !errors.Is(err, hub.ErrGoalScope) {
		t.Fatal("foreign workspace accepted", err)
	}
	request.Scope.Workspace = a.Workspace
	canceled, cancel := context.WithCancel(t.Context())
	cancel()
	if receipt, err := s.CreateGoal(canceled, request); !errors.Is(err, context.Canceled) || receipt != (hub.GoalReceipt{}) {
		t.Fatal("canceled goal committed", receipt, err)
	}
	if _, err = s.db.ExecContext(t.Context(), `CREATE TRIGGER reject_goal BEFORE INSERT ON hub_goals BEGIN SELECT RAISE(ABORT,'goal fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	if receipt, err := s.CreateGoal(t.Context(), request); err == nil || receipt != (hub.GoalReceipt{}) {
		t.Fatal("failed transaction returned receipt", receipt, err)
	}
	list, err := s.ListGoals(t.Context(), request.Scope, 100)
	if err != nil || len(list) != 0 {
		t.Fatal("failed/canceled creation left facts", list, err)
	}
	if _, err = s.db.ExecContext(t.Context(), `DROP TRIGGER reject_goal`); err != nil {
		t.Fatal(err)
	}
	if _, err = s.CreateGoal(t.Context(), request); err != nil {
		t.Fatal(err)
	}
	request.Scope.SessionID = "missing-session"
	if _, err = s.CreateGoal(t.Context(), request); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("missing scope accepted", err)
	}
}

func goalsVersionThreeDatabase(t *testing.T, dir string) (*sql.DB, hub.ModelConnection) {
	t.Helper()
	db := preferencesVersionOneDatabase(t, dir)
	for _, statement := range []string{
		`CREATE TABLE hub_preferences(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL)`,
		`CREATE TABLE hub_connections(id TEXT PRIMARY KEY,revision INTEGER NOT NULL CHECK(revision>0),created INTEGER NOT NULL,data BLOB NOT NULL)`,
		`CREATE TABLE hub_connection_credentials(connection TEXT PRIMARY KEY REFERENCES hub_connections(id),data BLOB NOT NULL)`,
		`PRAGMA user_version=3`,
	} {
		if _, err := db.ExecContext(t.Context(), statement); err != nil {
			t.Fatal(err)
		}
	}
	data, _ := json.Marshal(hub.DefaultPreferences())
	if _, err := db.ExecContext(t.Context(), `INSERT INTO hub_preferences VALUES(1,1,?)`, data); err != nil {
		t.Fatal(err)
	}
	legacy := &Store{db: db}
	connection, err := legacy.SaveConnection(t.Context(), connectionFixture())
	if err != nil {
		t.Fatal(err)
	}
	return db, connection
}

func TestGoalsVersionThreeMigrationPreservesConnections(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "v3")
	db, connection := goalsVersionThreeDatabase(t, dir)
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	s, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	var version int
	if err = s.db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil || version != 9 {
		t.Fatal("schema", version, err)
	}
	connections, err := s.Connections(t.Context())
	if err != nil || len(connections) != 1 || connections[0].ID != connection.ID || connections[0].Revision != connection.Revision {
		t.Fatal("migration changed connection", connections, err)
	}
	secret, err := s.ConnectionSecrets(t.Context(), connection.ID, connection.Revision)
	if err != nil || secret.APIKey != connectionFixture().APIKey {
		t.Fatal("migration changed credential", err)
	}
	if _, err = s.CreateGoal(t.Context(), goalRequest(t)); err != nil {
		t.Fatal(err)
	}
}

func TestGoalsMigrationFailureRollsBackVersionAndPreservesConnection(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "v3-failure")
	db, connection := goalsVersionThreeDatabase(t, dir)
	for _, statement := range []string{`CREATE TABLE collision(data TEXT)`, `INSERT INTO collision VALUES('retain v3 evidence')`, `CREATE INDEX hub_goal_scope ON collision(data)`} {
		if _, err := db.ExecContext(t.Context(), statement); err != nil {
			t.Fatal(err)
		}
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	if opened, err := Open(t.Context(), dir); err == nil {
		opened.Close()
		t.Fatal("colliding Goal index accepted")
	}
	db, err := private.Open(t.Context(), dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, tables int
	var evidence string
	if err = db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(t.Context(), `SELECT count(*) FROM sqlite_master WHERE name='hub_goals'`).Scan(&tables); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(t.Context(), `SELECT data FROM collision`).Scan(&evidence); err != nil || version != 3 || tables != 0 || evidence != "retain v3 evidence" {
		t.Fatal("partial migration", version, tables, evidence, err)
	}
	legacy := &Store{db: db}
	secret, err := legacy.ConnectionSecrets(t.Context(), connection.ID, connection.Revision)
	if err != nil || secret.APIKey != connectionFixture().APIKey {
		t.Fatal("failed migration changed credential", err)
	}
}
