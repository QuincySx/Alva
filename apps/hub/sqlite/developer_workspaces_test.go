// INPUT: Real private SQLite, canonical temporary workspaces and existing home-draft/session policies.
// OUTPUT: Import/tab CAS/reopen/budget and atomic first-Send retry/rollback regressions without execution.
// POS: [L7 SQLite adapter tests] Proves durable developer metadata never invents samples or starts a model.
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

const developerTime = int64(1_760_000_000_000)

func developerRepositoryFixture(t *testing.T, store *Store) hub.DeveloperRepository {
	return developerRepositoryFixtureNamed(t, store, "repository-fixture")
}

func developerRepositoryFixtureNamed(t *testing.T, store *Store, id string) hub.DeveloperRepository {
	t.Helper()
	root := t.TempDir()
	common := filepath.Join(root, ".git")
	if err := os.Mkdir(common, 0o700); err != nil {
		t.Fatal(err)
	}
	value, err := store.ImportDeveloperRepository(t.Context(), hub.DeveloperRepository{
		ID: id, Name: filepath.Base(root), Path: root, CommonDir: common, CreatedAt: developerTime,
	})
	if err != nil {
		t.Fatal(err)
	}
	return value
}

func TestConversationTabsIdentityCannotCrossRepositoryOrWorktreeScopes(t *testing.T) {
	store := newStore(t)
	firstRepository := developerRepositoryFixture(t, store)
	secondRepository := developerRepositoryFixtureNamed(t, store, "repository-second")
	firstScope := hub.WorktreeScope{RepositoryID: firstRepository.ID, Path: firstRepository.Path}
	secondWorktree := hub.WorktreeScope{RepositoryID: firstRepository.ID, Path: filepath.Clean(t.TempDir())}
	secondRepositoryScope := hub.WorktreeScope{RepositoryID: secondRepository.ID, Path: secondRepository.Path}

	tab := draftConversationTab("global-tab-one", "Repository owner", developerTime)
	saved, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: firstScope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
	if err != nil {
		t.Fatal(err)
	}
	for _, foreign := range []hub.WorktreeScope{secondWorktree, secondRepositoryScope} {
		if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: foreign, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0); err == nil {
			t.Fatal("same tab/home draft identity was claimed by another worktree", foreign)
		}
		empty, readErr := store.ConversationTabs(t.Context(), foreign)
		if readErr != nil || empty.Revision != 0 || len(empty.Tabs) != 0 {
			t.Fatal("failed cross-scope claim left partial tab metadata", empty, readErr)
		}
	}
	unchanged, err := store.ConversationTabs(t.Context(), firstScope)
	if err != nil || unchanged.Revision != saved.Revision || len(unchanged.Tabs) != 1 || unchanged.Tabs[0].ID != tab.ID {
		t.Fatal("failed cross-scope claim changed the owner", unchanged, err)
	}
	if saved, err = store.SaveConversationTabs(t.Context(), unchanged, unchanged.Revision); err != nil || saved.Revision != unchanged.Revision+1 {
		t.Fatal("existing identity in its owning scope was not reusable", saved, err)
	}
}

func TestConversationTabsArchiveKeepsIdentityAndPersistedTabsCannotBeRemoved(t *testing.T) {
	store := newStore(t)
	repository := developerRepositoryFixture(t, store)
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	otherScope := hub.WorktreeScope{RepositoryID: repository.ID, Path: filepath.Clean(t.TempDir())}
	first := draftConversationTab("archive-owner", "Archive owner", developerTime)
	second := draftConversationTab("archive-peer", "Archive peer", 0)
	saved, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: scope, Tabs: []hub.ConversationTab{first, second}, ActiveTabID: first.ID,
	}, 0)
	if err != nil {
		t.Fatal(err)
	}
	first.Archived, first.SelectedAt = true, 0
	second.SelectedAt = developerTime + 1
	saved, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: scope, Tabs: []hub.ConversationTab{first, second}, ActiveTabID: second.ID,
	}, saved.Revision)
	if err != nil || !saved.Tabs[0].Archived {
		t.Fatal("archive did not remain durable metadata", saved, err)
	}
	if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: otherScope, Tabs: []hub.ConversationTab{first}, ActiveTabID: first.ID,
	}, 0); err == nil {
		t.Fatal("archived tab identity was released to another worktree")
	}
	if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: scope, Tabs: []hub.ConversationTab{second}, ActiveTabID: second.ID,
	}, saved.Revision); err == nil {
		t.Fatal("persisted tab identity was deleted instead of archived")
	}
	restored, err := store.ConversationTabs(t.Context(), scope)
	if err != nil || restored.Revision != saved.Revision || len(restored.Tabs) != 2 || !restored.Tabs[0].Archived {
		t.Fatal("rejected delete changed archived tab ownership", restored, err)
	}
}

func draftConversationTab(id, title string, selected int64) hub.ConversationTab {
	return hub.ConversationTab{
		ID: id, DraftID: hub.ConversationTabDraftID(id), Title: title,
		CreatedAt: developerTime, SelectedAt: selected,
	}
}

func TestDeveloperRepositoryImportIsExplicitAndPathCommonRootIdempotent(t *testing.T) {
	store := newStore(t)
	repository := developerRepositoryFixture(t, store)
	again, err := store.ImportDeveloperRepository(t.Context(), hub.DeveloperRepository{
		ID: "repository-other", Name: "other", Path: repository.Path, CommonDir: repository.CommonDir, CreatedAt: developerTime + 1,
	})
	if err != nil || again != repository {
		t.Fatal("same path/common root did not resolve to the existing import", again, err)
	}
	items, err := store.ListDeveloperRepositories(t.Context())
	if err != nil || len(items) != 1 || items[0] != repository {
		t.Fatal("import listing invented or lost a repository", items, err)
	}
	for _, mismatch := range []hub.DeveloperRepository{
		{ID: "repository-changed", Name: "changed", Path: repository.Path, CommonDir: filepath.Join(t.TempDir(), ".git"), CreatedAt: developerTime},
		{ID: "repository-moved", Name: "moved", Path: filepath.Join(repository.Path, "moved"), CommonDir: repository.CommonDir, CreatedAt: developerTime},
	} {
		if _, err = store.ImportDeveloperRepository(t.Context(), mismatch); !errors.Is(err, hub.ErrDeveloperRepositoryConflict) {
			t.Fatal("changed common-root identity was silently adopted", err)
		}
	}
}

func TestConversationTabsCASOrderArchiveSelectionAndReopen(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	store, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	repository := developerRepositoryFixture(t, store)
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	session, err := store.CreateSession(t.Context(), "live session", scope.Path, target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	first := draftConversationTab("tab-one", "First", developerTime)
	second := draftConversationTab("tab-two", "Second", 0)
	saved, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: scope, Tabs: []hub.ConversationTab{first, second}, ActiveTabID: first.ID,
	}, 0)
	if err != nil || saved.Revision != 1 {
		t.Fatal(err)
	}
	second.SelectedAt = developerTime + 1
	saved, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: scope, Tabs: []hub.ConversationTab{second, first}, ActiveTabID: second.ID,
	}, saved.Revision)
	if err != nil || saved.Revision != 2 || saved.Tabs[0].ID != second.ID {
		t.Fatal("explicit order/selection was not saved", saved, err)
	}
	second.Archived, first.SelectedAt = true, developerTime+2
	saved, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{
		Scope: scope, Tabs: []hub.ConversationTab{second, first}, ActiveTabID: first.ID,
	}, saved.Revision)
	if err != nil || saved.Revision != 3 || saved.ActiveTabID != first.ID || !saved.Tabs[0].Archived {
		t.Fatal("archive did not remain explicit view metadata", saved, err)
	}
	if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope}, saved.Revision-1); !errors.Is(err, hub.ErrConversationTabsConflict) {
		t.Fatal("stale tab revision was accepted", err)
	}
	if _, err = store.db.ExecContext(t.Context(), `UPDATE hub_sessions SET updated=9999999999999 WHERE id=?`, session.ID); err != nil {
		t.Fatal(err)
	}
	afterUpdate, err := store.ConversationTabs(t.Context(), scope)
	if err != nil || afterUpdate.ActiveTabID != first.ID || afterUpdate.Tabs[0].ID != second.ID || !afterUpdate.Tabs[0].Archived {
		t.Fatal("session activity changed explicit tab metadata", afterUpdate, err)
	}
	var status string
	var turns, attempts int
	if err = store.db.QueryRowContext(t.Context(), `SELECT status FROM hub_sessions WHERE id=?`, session.ID).Scan(&status); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_turns WHERE session=?`, session.ID).Scan(&turns); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_attempts WHERE session=?`, session.ID).Scan(&attempts); err != nil {
		t.Fatal(err)
	}
	if status != string(engine.Succeeded) || turns != 0 || attempts != 0 {
		t.Fatal("archive or save started/stopped execution", status, turns, attempts)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	restored, err := reopened.ConversationTabs(t.Context(), scope)
	if err != nil || restored.Revision != 3 || restored.ActiveTabID != first.ID || restored.Tabs[0].ID != second.ID || !restored.Tabs[0].Archived {
		t.Fatal("reopen lost explicit order/selection/archive", restored, err)
	}
}

func TestConversationTabsRejectForeignSessionAndBoundedMetadataBudget(t *testing.T) {
	store := newStore(t)
	repository := developerRepositoryFixture(t, store)
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	foreign, err := store.CreateSession(t.Context(), "foreign", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	tab := draftConversationTab("foreign-tab", "Foreign", developerTime)
	tab.SessionID = foreign.ID
	if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0); err == nil {
		t.Fatal("Save introduced a session identity that belongs only to first Send")
	}
	tabs := make([]hub.ConversationTab, hub.MaxConversationTabs)
	for index := range tabs {
		tabs[index] = draftConversationTab(fmt.Sprintf("budget-%04d", index), fmt.Sprintf("Draft %d", index), 0)
	}
	tabs[0].SelectedAt = developerTime
	saved, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: tabs, ActiveTabID: tabs[0].ID}, 0)
	if err != nil || len(saved.Tabs) != hub.MaxConversationTabs {
		t.Fatal("supported 1024-tab metadata budget failed", err)
	}
	tabs = append(tabs, draftConversationTab("over-budget", "Over", 0))
	if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: tabs, ActiveTabID: tabs[0].ID}, saved.Revision); err == nil {
		t.Fatal("metadata abuse budget above 1024 was accepted")
	}
}

func TestConversationTabsSaveOnlyPreservesValidFirstSendBindings(t *testing.T) {
	store := newStore(t)
	repository := developerRepositoryFixture(t, store)
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	tab := draftConversationTab("binding-owner", "Bound by Send", developerTime)
	saved, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
	if err != nil {
		t.Fatal(err)
	}
	local, err := store.CreateSession(t.Context(), "local", scope.Path, target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	forged := saved
	forged.Tabs[0].SessionID = local.ID
	if _, err = store.SaveConversationTabs(t.Context(), forged, saved.Revision); err == nil {
		t.Fatal("Save forged a first-Send session binding")
	}
	foreign, err := store.CreateSession(t.Context(), "foreign", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	bindStoredTabs := func(sessionID string) hub.ConversationTabSet {
		t.Helper()
		current, readErr := store.ConversationTabs(t.Context(), scope)
		if readErr != nil {
			t.Fatal(readErr)
		}
		current.Tabs[0].SessionID = sessionID
		if writeErr := store.transaction(t.Context(), func(tx *sql.Tx) error { return writeConversationTabs(t.Context(), tx, current) }); writeErr != nil {
			t.Fatal(writeErr)
		}
		return current
	}
	current := bindStoredTabs(foreign.ID)
	if _, err = store.SaveConversationTabs(t.Context(), current, current.Revision); err == nil {
		t.Fatal("Save preserved a session from another workspace")
	}
	current = bindStoredTabs(local.ID)
	saved, err = store.SaveConversationTabs(t.Context(), current, current.Revision)
	if err != nil || saved.Tabs[0].SessionID != local.ID {
		t.Fatal("Save rejected an existing valid first-Send binding", saved, err)
	}
	unbound := saved
	unbound.Tabs[0].SessionID = ""
	if _, err = store.SaveConversationTabs(t.Context(), unbound, saved.Revision); err == nil {
		t.Fatal("Save unbound a first-Send session")
	}
	current = bindStoredTabs("hub-session-missing")
	if _, err = store.SaveConversationTabs(t.Context(), current, current.Revision); err == nil {
		t.Fatal("Save preserved a missing session")
	}
}

func TestConversationTabFirstSendIsAtomicIdempotentAndNeverStartsTurn(t *testing.T) {
	store := newStore(t)
	repository := developerRepositoryFixture(t, store)
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	tab := draftConversationTab("first-send", "New workspace chat", developerTime)
	tabs, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
	if err != nil {
		t.Fatal(err)
	}
	draftScope := hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}
	draft, err := store.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: draftScope, Text: "first prompt"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	first, err := store.AdmitConversationTab(t.Context(), draft, "created on Send", scope, tab.ID, tabs.Revision, target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if first.Session.Workspace != scope.Path || first.Draft.PendingSessionID != first.Session.ID || first.Draft.Revision != 2 || first.Tabs.Revision != 2 {
		t.Fatal("first Send did not atomically bind tab/draft/session", first)
	}
	retry, err := store.AdmitConversationTab(t.Context(), draft, "ignored retry title", scope, tab.ID, tabs.Revision, target("alva"))
	if err != nil || retry.Session.ID != first.Session.ID || retry.Tabs.Revision != first.Tabs.Revision {
		t.Fatal("original first-Send retry created or returned another session", retry, err)
	}
	committed, err := store.AdmitConversationTab(t.Context(), first.Draft, "ignored retry title", scope, tab.ID, first.Tabs.Revision, target("alva"))
	if err != nil || committed.Session.ID != first.Session.ID {
		t.Fatal("committed first-Send retry was not idempotent", err)
	}
	if _, err = store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope}, first.Tabs.Revision); err == nil {
		t.Fatal("session-bound tab identity was removed from its durable set")
	}
	if _, err = store.AdmitConversationTab(t.Context(), draft, "changed target", scope, tab.ID, tabs.Revision, target("other")); err == nil {
		t.Fatal("captured route mismatch was accepted")
	}
	var sessions, turns, attempts int
	if err = store.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_sessions`).Scan(&sessions); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_turns`).Scan(&turns); err != nil {
		t.Fatal(err)
	}
	if err = store.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_attempts`).Scan(&attempts); err != nil {
		t.Fatal(err)
	}
	if sessions != 1 || turns != 0 || attempts != 0 {
		t.Fatal("first-Send admission created duplicate sessions or started a Turn", sessions, turns, attempts)
	}
}

func TestConversationTabAdmissionRequiresCurrentDraftAndRollsBackEveryFactOnFailure(t *testing.T) {
	store := newStore(t)
	repository := developerRepositoryFixture(t, store)
	scope := hub.WorktreeScope{RepositoryID: repository.ID, Path: repository.Path}
	tab := draftConversationTab("rollback-send", "Rollback", developerTime)
	tabs, err := store.SaveConversationTabs(t.Context(), hub.ConversationTabSet{Scope: scope, Tabs: []hub.ConversationTab{tab}, ActiveTabID: tab.ID}, 0)
	if err != nil {
		t.Fatal(err)
	}
	draft, err := store.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: tab.DraftID}, Text: "captured"}, 0)
	if err != nil {
		t.Fatal(err)
	}
	newer, err := store.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: draft.Scope, Text: "newer edit"}, draft.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.AdmitConversationTab(t.Context(), draft, "stale", scope, tab.ID, tabs.Revision, target("alva")); err == nil {
		t.Fatal("stale captured draft was admitted", err)
	}
	if _, err = store.db.ExecContext(t.Context(), `CREATE TRIGGER reject_tab_binding BEFORE INSERT ON hub_conversation_tab_sets BEGIN SELECT RAISE(ABORT,'fixture tab write failure'); END`); err != nil {
		t.Fatal(err)
	}
	updatedTabs, err := store.ConversationTabs(t.Context(), scope)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = store.AdmitConversationTab(t.Context(), newer, "must rollback", scope, tab.ID, updatedTabs.Revision, target("alva")); err == nil {
		t.Fatal("forced tab write failure was reported as success")
	}
	var sessions int
	if err = store.db.QueryRowContext(t.Context(), `SELECT count(*) FROM hub_sessions`).Scan(&sessions); err != nil || sessions != 0 {
		t.Fatal("failed admission retained a HubSession", sessions, err)
	}
	readDraft, err := store.ComposerDraft(t.Context(), draft.Scope)
	if err != nil || readDraft.Revision != newer.Revision || readDraft.PendingSessionID != "" {
		t.Fatal("failed admission changed the durable draft", readDraft, err)
	}
	readTabs, err := store.ConversationTabs(t.Context(), scope)
	if err != nil || readTabs.Revision != tabs.Revision || readTabs.Tabs[0].SessionID != "" {
		t.Fatal("failed admission changed tab binding", readTabs, err)
	}
}

func TestDeveloperWorkspacesSchemaEightMigrationAndFailurePreserveVersionSeven(t *testing.T) {
	ctx := context.Background()
	for _, broken := range []bool{false, true} {
		directory := filepath.Join(t.TempDir(), "v7")
		store, err := Open(ctx, directory)
		if err != nil {
			t.Fatal(err)
		}
		session, err := store.CreateSession(ctx, "retain v7", t.TempDir(), target("alva"))
		if err != nil {
			t.Fatal(err)
		}
		for _, statement := range []string{`DROP TABLE hub_conversation_tab_sets`, `DROP TABLE hub_developer_repositories`, `PRAGMA user_version=7`} {
			if _, err = store.db.ExecContext(ctx, statement); err != nil {
				t.Fatal(err)
			}
		}
		if broken {
			if _, err = store.db.ExecContext(ctx, `CREATE TABLE hub_developer_repositories(foreign_fact TEXT); INSERT INTO hub_developer_repositories VALUES('retain collision')`); err != nil {
				t.Fatal(err)
			}
		}
		if err = store.Close(); err != nil {
			t.Fatal(err)
		}
		reopened, err := Open(ctx, directory)
		if broken {
			if reopened != nil {
				reopened.Close()
			}
			if err == nil {
				t.Fatal("collision was silently adopted")
			}
			db, openErr := private.Open(ctx, directory, "product.sqlite")
			if openErr != nil {
				t.Fatal(openErr)
			}
			defer db.Close()
			var version, sessions, tabSets int
			var fact string
			if openErr = db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); openErr != nil {
				t.Fatal(openErr)
			}
			if openErr = db.QueryRowContext(ctx, `SELECT count(*) FROM hub_sessions WHERE id=?`, session.ID).Scan(&sessions); openErr != nil {
				t.Fatal(openErr)
			}
			if openErr = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE type='table' AND name='hub_conversation_tab_sets'`).Scan(&tabSets); openErr != nil {
				t.Fatal(openErr)
			}
			if openErr = db.QueryRowContext(ctx, `SELECT foreign_fact FROM hub_developer_repositories`).Scan(&fact); openErr != nil || version != 7 || sessions != 1 || tabSets != 0 || fact != "retain collision" {
				t.Fatal("failed v8 migration changed version/facts", version, sessions, tabSets, fact, openErr)
			}
			continue
		}
		if err != nil {
			t.Fatal(err)
		}
		var version, sessions, repositories, tabSets int
		if err = reopened.db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
			t.Fatal(err)
		}
		if err = reopened.db.QueryRowContext(ctx, `SELECT count(*) FROM hub_sessions WHERE id=?`, session.ID).Scan(&sessions); err != nil {
			t.Fatal(err)
		}
		if err = reopened.db.QueryRowContext(ctx, `SELECT count(*) FROM hub_developer_repositories`).Scan(&repositories); err != nil {
			t.Fatal(err)
		}
		if err = reopened.db.QueryRowContext(ctx, `SELECT count(*) FROM hub_conversation_tab_sets`).Scan(&tabSets); err != nil {
			t.Fatal(err)
		}
		if version != 8 || sessions != 1 || repositories != 0 || tabSets != 0 {
			t.Fatal("v8 migration lost v7 facts or invented sample metadata", version, sessions, repositories, tabSets)
		}
		reopened.Close()
	}
}
