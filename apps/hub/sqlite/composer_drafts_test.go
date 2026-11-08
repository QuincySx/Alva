// INPUT: Temporary real SQLite, schema-five product facts and scoped manual draft revisions.
// OUTPUT: Durable restore/CAS, exact consumption tokens and atomic v5-to-v6 migration regressions.
// POS: [L7 SQLite tests] Proves drafts survive restart without executing or replacing existing facts.
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

func TestComposerDraftsRestoreTextReferencesAndRetainedRevision(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "product")
	s, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	session, err := s.CreateSession(t.Context(), "draft owner", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	scope := hub.ComposerDraftScope{Kind: "session", ID: session.ID}
	home := hub.ComposerDraftScope{Kind: "home", ID: "logical-new-task"}
	baseline, err := s.ComposerDraft(t.Context(), scope)
	if err != nil || baseline.Revision != 0 {
		t.Fatal(baseline, err)
	}
	next := hub.ComposerDraft{Scope: scope, Text: "manual input", References: []hub.ComposerDraftReference{{Kind: "workspace", ID: "frozen-original", SHA256: strings.Repeat("a", 64)}}, PreparedCommands: json.RawMessage(`[  { "preserved": "<source>" }  ]`)}
	saved, err := s.SaveComposerDraft(t.Context(), next, 0)
	if err != nil || saved.Revision != 1 {
		t.Fatal(saved, err)
	}
	if _, err = s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: home, Text: "home stays separate"}, 0); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	got, err := s.ComposerDraft(t.Context(), scope)
	if err != nil || !reflect.DeepEqual(got, saved) {
		t.Fatal("restart did not restore exact manual draft", got, err)
	}
	detail, err := s.Detail(t.Context(), session.ID)
	var turns int
	if err != nil || detail.Session.Status == hub.Running {
		t.Fatal("draft restoration executed input", err)
	}
	if err = s.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_turns WHERE session=?`, session.ID).Scan(&turns); err != nil || turns != 0 {
		t.Fatal("draft persistence admitted a Turn", turns, err)
	}
	// Identical text after an intervening edit is a distinct captured version.
	got.Text = "temporary edit"
	got, err = s.SaveComposerDraft(t.Context(), got, got.Revision)
	if err != nil {
		t.Fatal(err)
	}
	got.Text = saved.Text
	got, err = s.SaveComposerDraft(t.Context(), got, got.Revision)
	if err != nil || got.Revision != 3 {
		t.Fatal(got, err)
	}
	if _, err = s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope}, saved.Revision); !errors.Is(err, hub.ErrComposerDraftConflict) {
		t.Fatal("old acknowledgement erased edited-back text", err)
	}
	empty, err := s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope}, got.Revision)
	if err != nil || empty.Revision != 4 || empty.Text != "" {
		t.Fatal(empty, err)
	}
	if _, err = s.SaveComposerDraft(t.Context(), next, 0); !errors.Is(err, hub.ErrComposerDraftConflict) {
		t.Fatal("discard reset revision and admitted a stale window", err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	if restoredEmpty, err := s.ComposerDraft(t.Context(), scope); err != nil || !reflect.DeepEqual(restoredEmpty, empty) {
		t.Fatal("restart lost the consumed revision token", restoredEmpty, err)
	}
	other, err := s.ComposerDraft(t.Context(), home)
	if err != nil || other.Text != "home stays separate" {
		t.Fatal("session consumption touched the home draft", other, err)
	}
	if _, err = s.ComposerDraft(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: "missing"}); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("missing session was replaced with a draft", err)
	}
}

func TestComposerDraftsCompetingWindowsCommitExactlyOne(t *testing.T) {
	s := newStore(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "shared-logical-scope"}
	start := make(chan struct{})
	results := make(chan error, 2)
	var joined sync.WaitGroup
	for _, text := range []string{"first window", "second window"} {
		joined.Add(1)
		go func() {
			defer joined.Done()
			<-start
			_, err := s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope, Text: text}, 0)
			results <- err
		}()
	}
	close(start)
	joined.Wait()
	close(results)
	success, conflict := 0, 0
	for err := range results {
		if err == nil {
			success++
		} else if errors.Is(err, hub.ErrComposerDraftConflict) {
			conflict++
		} else {
			t.Fatal(err)
		}
	}
	if success != 1 || conflict != 1 {
		t.Fatal("shared-scope CAS lost an edit", success, conflict)
	}
}

func TestComposerDraftsFailureAndCorruptionDoNotResetInput(t *testing.T) {
	s := newStore(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "faults"}
	draft := hub.ComposerDraft{Scope: scope, Text: strings.Repeat("<", 64<<10)}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := s.SaveComposerDraft(ctx, draft, 0); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled save committed", err)
	}
	if _, err := s.db.ExecContext(t.Context(), `CREATE TRIGGER reject_composer_draft BEFORE INSERT ON hub_composer_drafts BEGIN SELECT RAISE(ABORT,'draft fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	if value, err := s.SaveComposerDraft(t.Context(), draft, 0); err == nil || value.Revision != 0 {
		t.Fatal("failed insert returned committed input", value, err)
	}
	if value, err := s.ComposerDraft(t.Context(), scope); err != nil || value.Revision != 0 {
		t.Fatal("failed insert left partial input", value, err)
	}
	if _, err := s.db.ExecContext(t.Context(), `DROP TRIGGER reject_composer_draft`); err != nil {
		t.Fatal(err)
	}
	saved, err := s.SaveComposerDraft(t.Context(), draft, 0)
	if err != nil || saved.Text != draft.Text {
		t.Fatal("JSON escaping shrank the declared text budget", err)
	}
	if _, err = s.db.ExecContext(t.Context(), `CREATE TRIGGER reject_composer_update BEFORE UPDATE ON hub_composer_drafts BEGIN SELECT RAISE(ABORT,'draft fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	if value, err := s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope}, saved.Revision); err == nil || value.Revision != 0 {
		t.Fatal("failed consumption reported success", value, err)
	}
	if value, err := s.ComposerDraft(t.Context(), scope); err != nil || !reflect.DeepEqual(value, saved) {
		t.Fatal("failed consumption changed the draft", err)
	}
	if _, err = s.db.ExecContext(t.Context(), `DROP TRIGGER reject_composer_update`); err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(t.Context(), `UPDATE hub_composer_drafts SET revision=revision+1 WHERE scope_kind=? AND scope_id=?`, scope.Kind, scope.ID); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ComposerDraft(t.Context(), scope); err == nil {
		t.Fatal("corrupt revision was silently replaced")
	}
}

func composerVersionFiveDatabase(t *testing.T, directory string) (*sql.DB, hub.ModelConnection, hub.Goal) {
	t.Helper()
	db, connection, goal := workflowVersionFourDatabase(t, directory)
	tx, err := db.BeginTx(t.Context(), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer tx.Rollback()
	if err = companionSchema(t.Context(), tx); err != nil {
		t.Fatal(err)
	}
	if err = workspaceViewsSchema(t.Context(), tx); err != nil {
		t.Fatal(err)
	}
	if _, err = tx.ExecContext(t.Context(), `PRAGMA user_version=5`); err != nil {
		t.Fatal(err)
	}
	if err = tx.Commit(); err != nil {
		t.Fatal(err)
	}
	return db, connection, goal
}

func TestComposerSchemaSixMigratesRealVersionFiveWithoutReplacingFacts(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "v5")
	db, connection, goal := composerVersionFiveDatabase(t, directory)
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	var version int
	if err = s.db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil || version != 9 {
		t.Fatal(version, err)
	}
	loaded, err := s.Goal(t.Context(), goal.ID)
	if err != nil || loaded != goal {
		t.Fatal("migration replaced existing goal", err)
	}
	secrets, err := s.ConnectionSecrets(t.Context(), connection.ID, connection.Revision)
	if err != nil || secrets.APIKey != connectionFixture().APIKey {
		t.Fatal("migration replaced existing connection", err)
	}
	draft, err := s.ComposerDraft(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "new-task"})
	if err != nil || draft.Revision != 0 || draft.Text != "" {
		t.Fatal("migration invented input", draft, err)
	}
}

func TestComposerSchemaSixFailureRollsBackDraftsBeforeRecovery(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "v5-conflict")
	db, _, goal := composerVersionFiveDatabase(t, directory)
	legacy := &Store{db: db}
	session, err := legacy.CreateSession(t.Context(), "retain running fact", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = legacy.BeginTurn(t.Context(), session.ID, "explicit fixture"); err != nil {
		t.Fatal(err)
	}
	if _, err = db.ExecContext(t.Context(), `CREATE TABLE hub_composer_files(foreign_fact TEXT)`); err != nil {
		t.Fatal(err)
	}
	if err = db.Close(); err != nil {
		t.Fatal(err)
	}
	if bad, err := Open(t.Context(), directory); err == nil {
		bad.Close()
		t.Fatal("conflicting authority was replaced")
	}
	db, err = private.Open(t.Context(), directory, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, draftTables, goals int
	var status string
	for _, query := range []struct {
		sql string
		out any
	}{
		{`PRAGMA user_version`, &version},
		{`SELECT count(*) FROM sqlite_master WHERE type='table' AND name='hub_composer_drafts'`, &draftTables},
		{`SELECT count(*) FROM hub_goals`, &goals},
	} {
		if err = db.QueryRowContext(t.Context(), query.sql).Scan(query.out); err != nil {
			t.Fatal(err)
		}
	}
	if err = db.QueryRowContext(t.Context(), `SELECT status FROM hub_sessions WHERE id=?`, session.ID).Scan(&status); err != nil {
		t.Fatal(err)
	}
	if version != 5 || draftTables != 0 || goals != 1 || status != string(hub.Running) {
		t.Fatal("failed migration changed prior authority or ran recovery", version, draftTables, goals, status, goal.ID)
	}
}

func TestComposerCurrentSchemaCorruptionFailsBeforeRecovery(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "v6-corrupt")
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(t.Context(), "preserve running state", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.BeginTurn(t.Context(), session.ID, "fixture input"); err != nil {
		t.Fatal(err)
	}
	scope := hub.ComposerDraftScope{Kind: "session", ID: session.ID}
	if _, err = s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope, Text: "retain corrupt evidence"}, 0); err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(t.Context(), `UPDATE hub_composer_drafts SET data='{}'`); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if bad, err := Open(t.Context(), directory); err == nil {
		bad.Close()
		t.Fatal("corrupt committed draft was silently replaced")
	}
	db, err := private.Open(t.Context(), directory, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var status, data string
	if err = db.QueryRowContext(t.Context(), `SELECT status FROM hub_sessions WHERE id=?`, session.ID).Scan(&status); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(t.Context(), `SELECT data FROM hub_composer_drafts WHERE scope_kind=? AND scope_id=?`, scope.Kind, scope.ID).Scan(&data); err != nil {
		t.Fatal(err)
	}
	if status != string(hub.Running) || data != "{}" {
		t.Fatal("failed validation changed execution or reset draft evidence", status, data)
	}
}

func TestComposerFirstSessionAdmissionIsAtomicAndSharedAcrossWindows(t *testing.T) {
	s := newStore(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}
	captured, err := s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope, Text: "manual first Send"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	workspace := t.TempDir()
	type result struct {
		session hub.Session
		draft   hub.ComposerDraft
		err     error
	}
	results := make(chan result, 2)
	var join sync.WaitGroup
	for range 2 {
		join.Add(1)
		go func() {
			defer join.Done()
			session, draft, err := s.CreateComposerSession(t.Context(), captured, "first Send", workspace, target("alva"))
			results <- result{session, draft, err}
		}()
	}
	join.Wait()
	close(results)
	var id string
	for result := range results {
		if result.err != nil || result.session.ID == "" || result.draft.PendingSessionID != result.session.ID || result.draft.Revision != captured.Revision+1 {
			t.Fatal("atomic first-Send identity receipt", result)
		}
		if id != "" && id != result.session.ID {
			t.Fatal("synchronized windows created duplicate sessions")
		}
		id = result.session.ID
	}
	sessions, err := s.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 1 {
		t.Fatal("first identity admission created duplicate facts", len(sessions), err)
	}
	current, err := s.ComposerDraft(t.Context(), scope)
	if err != nil || current.PendingSessionID != id || current.Text != captured.Text {
		t.Fatal("identity binding did not retain manual input", current, err)
	}
	if reused, _, err := s.CreateComposerSession(t.Context(), current, "same captured owner", workspace, target("alva")); err != nil || reused.ID != id {
		t.Fatal("matching pending owner could not reuse its actual creation", reused, err)
	}
	if _, _, err := s.CreateComposerSession(t.Context(), current, "foreign workspace", t.TempDir(), target("alva")); err == nil {
		t.Fatal("pending identity was reused in a different requested workspace")
	}
	changedTarget := target("alva")
	changedTarget.ModelID = "another-model"
	if _, _, err := s.CreateComposerSession(t.Context(), current, "foreign target", workspace, changedTarget); err == nil {
		t.Fatal("pending identity was reused with a different requested target")
	}
	if _, _, err = s.CreateComposerSession(t.Context(), hub.ComposerDraft{Scope: scope, Revision: captured.Revision, Text: "foreign changed input"}, "changed", workspace, target("alva")); !errors.Is(err, hub.ErrComposerDraftConflict) {
		t.Fatal("changed stale draft reused another captured admission", err)
	}
	for _, forged := range []hub.ComposerDraft{
		{Scope: scope, Revision: current.Revision, Text: current.Text},
		{Scope: scope, Revision: current.Revision, Text: current.Text, PendingSessionID: "foreign-session"},
	} {
		if _, _, err = s.CreateComposerSession(t.Context(), forged, "forged pointer", workspace, target("alva")); !errors.Is(err, hub.ErrComposerDraftConflict) {
			t.Fatal("altered pending identity was accepted", forged, err)
		}
	}
	forged := hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: "forged-other-home"}, Text: current.Text, PendingSessionID: id}
	if _, err = s.SaveComposerDraft(t.Context(), forged, 0); err == nil {
		t.Fatal("ordinary home Save introduced a session identity")
	}
	forged = hub.CloneComposerDraft(current)
	forged.PendingSessionID = "foreign-session"
	if _, err = s.SaveComposerDraft(t.Context(), forged, current.Revision); err == nil {
		t.Fatal("ordinary home Save changed its atomic session identity")
	}
	if _, err = s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope}, current.Revision); err != nil {
		t.Fatal("explicit relocation/consumption could not clear its own pending identity", err)
	}
}

func TestComposerFirstSessionBindingFailureRollsBackSessionAndBranch(t *testing.T) {
	s := newStore(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "first-fault"}
	captured, err := s.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope, Text: "keep input"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(t.Context(), `CREATE TRIGGER reject_composer_binding BEFORE UPDATE ON hub_composer_drafts BEGIN SELECT RAISE(ABORT,'binding fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	if session, draft, err := s.CreateComposerSession(t.Context(), captured, "failed first Send", t.TempDir(), target("alva")); err == nil || session.ID != "" || draft.Revision != 0 {
		t.Fatal("failed binding returned a durable identity", session, draft, err)
	}
	var sessions, branches int
	if err = s.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_sessions`).Scan(&sessions); err != nil {
		t.Fatal(err)
	}
	if err = s.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_branches`).Scan(&branches); err != nil {
		t.Fatal(err)
	}
	current, err := s.ComposerDraft(t.Context(), scope)
	if err != nil || sessions != 0 || branches != 0 || !reflect.DeepEqual(current, captured) {
		t.Fatal("binding failure left partial product authority", sessions, branches, current, err)
	}
}
