// INPUT: Explicit repository imports, shared tab policy, existing home-draft admission and private SQLite transactions.
// OUTPUT: v8 repository/tab persistence, scoped CAS, new-identity-only cross-scope scan, atomic tab+draft+HubSession first-Send binding.
// POS: [L7 SQLite product provider] View metadata never executes, stops sessions, removes Git worktrees or invents samples.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
)

const maxConversationTabSetData = 4 << 20

func developerWorkspacesSchema(ctx context.Context, tx *sql.Tx) error {
	for _, statement := range []string{
		`CREATE TABLE hub_developer_repositories(id TEXT PRIMARY KEY CHECK(length(id)>0),path TEXT NOT NULL UNIQUE CHECK(length(path)>0),common_dir TEXT NOT NULL UNIQUE CHECK(length(common_dir)>0),created INTEGER NOT NULL CHECK(created>0),data BLOB NOT NULL)`,
		`CREATE TABLE hub_conversation_tab_sets(repository TEXT NOT NULL REFERENCES hub_developer_repositories(id),path TEXT NOT NULL UNIQUE,revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL,PRIMARY KEY(repository,path))`,
	} {
		if _, err := tx.ExecContext(ctx, statement); err != nil {
			return err
		}
	}
	return nil
}

func validateDeveloperWorkspacesSchema(ctx context.Context, tx *sql.Tx) error {
	rows, err := tx.QueryContext(ctx, `SELECT id,path,common_dir,data FROM hub_developer_repositories`)
	if err != nil {
		return err
	}
	for rows.Next() {
		var id, path, commonDir string
		var data []byte
		if err = rows.Scan(&id, &path, &commonDir, &data); err != nil {
			rows.Close()
			return err
		}
		value, decodeErr := decodeDeveloperRepository(data)
		if decodeErr != nil || value.ID != id || value.Path != path || value.CommonDir != commonDir {
			rows.Close()
			return errors.New("developer repository schema contains corrupt identity")
		}
	}
	err = errors.Join(err, rows.Err(), rows.Close())
	if err != nil {
		return err
	}
	rows, err = tx.QueryContext(ctx, `SELECT repository,path,revision,data FROM hub_conversation_tab_sets`)
	if err != nil {
		return err
	}
	defer rows.Close()
	tabOwners, draftOwners := make(map[string]hub.WorktreeScope), make(map[string]hub.WorktreeScope)
	for rows.Next() {
		var repository, path string
		var revision uint64
		var data []byte
		if err = rows.Scan(&repository, &path, &revision, &data); err != nil {
			return err
		}
		var exists int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_developer_repositories WHERE id=?`, repository).Scan(&exists); err != nil || exists != 1 {
			return errors.New("conversation tab set references a missing repository")
		}
		scope := hub.WorktreeScope{RepositoryID: repository, Path: path}
		value, decodeErr := decodeConversationTabSet(data, scope, revision)
		if decodeErr != nil {
			return decodeErr
		}
		for _, tab := range value.Tabs {
			if _, exists := tabOwners[tab.ID]; exists {
				return errors.New("conversation tab identity is owned by multiple worktrees")
			}
			tabOwners[tab.ID] = scope
			if tab.DraftID != "" {
				if _, exists := draftOwners[tab.DraftID]; exists {
					return errors.New("conversation home draft identity is owned by multiple worktrees")
				}
				draftOwners[tab.DraftID] = scope
			}
		}
	}
	return rows.Err()
}

func decodeDeveloperRepository(data []byte) (hub.DeveloperRepository, error) {
	if len(data) > 128<<10 {
		return hub.DeveloperRepository{}, errors.New("developer repository record exceeds metadata budget")
	}
	var value hub.DeveloperRepository
	if json.Unmarshal(data, &value) != nil || value.Validate() != nil {
		return hub.DeveloperRepository{}, errors.New("developer repository record is corrupt")
	}
	return value, nil
}

func (s *Store) ListDeveloperRepositories(ctx context.Context) (items []hub.DeveloperRepository, err error) {
	defer func() { err = normalizeStoreError(err) }()
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM hub_developer_repositories ORDER BY created DESC,id`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var data []byte
		if err = rows.Scan(&data); err != nil {
			return nil, err
		}
		value, decodeErr := decodeDeveloperRepository(data)
		if decodeErr != nil {
			return nil, decodeErr
		}
		items = append(items, value)
	}
	return items, rows.Err()
}

func (s *Store) ImportDeveloperRepository(ctx context.Context, proposed hub.DeveloperRepository) (value hub.DeveloperRepository, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = proposed.Validate(); err != nil {
		return hub.DeveloperRepository{}, err
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		rows, e := tx.QueryContext(ctx, `SELECT data FROM hub_developer_repositories WHERE path=? OR common_dir=? ORDER BY created,id LIMIT 2`, proposed.Path, proposed.CommonDir)
		if e != nil {
			return e
		}
		var existing []hub.DeveloperRepository
		for rows.Next() {
			var data []byte
			if e = rows.Scan(&data); e != nil {
				rows.Close()
				return e
			}
			item, decodeErr := decodeDeveloperRepository(data)
			if decodeErr != nil {
				rows.Close()
				return decodeErr
			}
			existing = append(existing, item)
		}
		if e = errors.Join(rows.Err(), rows.Close()); e != nil {
			return e
		}
		if len(existing) > 1 {
			return hub.ErrDeveloperRepositoryConflict
		}
		if len(existing) == 1 {
			if existing[0].Path != proposed.Path || existing[0].CommonDir != proposed.CommonDir {
				return hub.ErrDeveloperRepositoryConflict
			}
			value = existing[0]
			return nil
		}
		data, e := body(proposed)
		if e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, `INSERT INTO hub_developer_repositories(id,path,common_dir,created,data) VALUES(?,?,?,?,?)`, proposed.ID, proposed.Path, proposed.CommonDir, proposed.CreatedAt, data); e != nil {
			return e
		}
		value = proposed
		return nil
	})
	if err != nil {
		return hub.DeveloperRepository{}, err
	}
	return value, nil
}

func decodeConversationTabSet(data []byte, scope hub.WorktreeScope, revision uint64) (hub.ConversationTabSet, error) {
	if len(data) > maxConversationTabSetData {
		return hub.ConversationTabSet{}, errors.New("conversation tab set exceeds encoded metadata budget")
	}
	var value hub.ConversationTabSet
	if json.Unmarshal(data, &value) != nil || value.Scope != scope || value.Revision != revision || revision == 0 || value.Validate() != nil {
		return hub.ConversationTabSet{}, errors.New("conversation tab set record is corrupt")
	}
	return hub.CloneConversationTabSet(value), nil
}

func encodeConversationTabSet(value hub.ConversationTabSet) ([]byte, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	if len(data) > maxConversationTabSetData {
		return nil, errors.New("conversation tab set exceeds encoded metadata budget")
	}
	return data, nil
}

func conversationTabs(ctx context.Context, q querier, scope hub.WorktreeScope) (hub.ConversationTabSet, error) {
	if err := scope.Validate(); err != nil {
		return hub.ConversationTabSet{}, err
	}
	var repository int
	if err := q.QueryRowContext(ctx, `SELECT count(*) FROM hub_developer_repositories WHERE id=?`, scope.RepositoryID).Scan(&repository); err != nil {
		return hub.ConversationTabSet{}, err
	}
	if repository != 1 {
		return hub.ConversationTabSet{}, hub.ErrNotFound
	}
	var storedRepository, path string
	var encodedRevision uint64
	var data []byte
	err := q.QueryRowContext(ctx, `SELECT repository,path,revision,data FROM hub_conversation_tab_sets WHERE path=?`, scope.Path).Scan(&storedRepository, &path, &encodedRevision, &data)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.ConversationTabSet{Scope: scope}, nil
	}
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	if storedRepository != scope.RepositoryID || path != scope.Path {
		return hub.ConversationTabSet{}, hub.ErrConversationTabsConflict
	}
	return decodeConversationTabSet(data, scope, encodedRevision)
}

func writeConversationTabs(ctx context.Context, tx *sql.Tx, value hub.ConversationTabSet) error {
	data, err := encodeConversationTabSet(value)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_conversation_tab_sets(repository,path,revision,data) VALUES(?,?,?,?) ON CONFLICT(repository,path) DO UPDATE SET revision=excluded.revision,data=excluded.data`, value.Scope.RepositoryID, value.Scope.Path, value.Revision, data)
	return err
}

func (s *Store) ConversationTabs(ctx context.Context, scope hub.WorktreeScope) (value hub.ConversationTabSet, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		value, err = conversationTabs(ctx, tx, scope)
		return err
	})
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	return hub.CloneConversationTabSet(value), nil
}

func (s *Store) SaveConversationTabs(ctx context.Context, next hub.ConversationTabSet, expected uint64) (value hub.ConversationTabSet, err error) {
	defer func() { err = normalizeStoreError(err) }()
	next = hub.CloneConversationTabSet(next)
	if err = next.Validate(); err != nil {
		return hub.ConversationTabSet{}, err
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := conversationTabs(ctx, tx, next.Scope)
		if e != nil {
			return e
		}
		if current.Revision != expected || expected >= 1<<63-1 {
			return hub.ErrConversationTabsConflict
		}
		existing := make(map[string]hub.ConversationTab, len(current.Tabs))
		for _, tab := range current.Tabs {
			existing[tab.ID] = tab
		}
		nextIDs := make(map[string]bool, len(next.Tabs))
		for _, tab := range next.Tabs {
			nextIDs[tab.ID] = true
		}
		for _, tab := range current.Tabs {
			if !nextIDs[tab.ID] {
				return errors.New("persisted conversation tabs cannot be removed; close them with Archived")
			}
		}
		// Cross-scope scan only guards identities this scope does not already own;
		// selection, archive and reorder reuse them, so a new tab ID or DraftID is
		// the only reason to decode every other worktree scope.
		claimsNew := false
		for _, tab := range next.Tabs {
			bound, found := existing[tab.ID]
			if !found || tab.DraftID != "" && tab.DraftID != bound.DraftID {
				claimsNew = true
				break
			}
		}
		if claimsNew {
			if e = validateConversationTabOwnership(ctx, tx, next); e != nil {
				return e
			}
		}
		for _, tab := range next.Tabs {
			bound, found := existing[tab.ID]
			if !found {
				if tab.SessionID != "" {
					return errors.New("conversation tab session identity is assigned only by first Send")
				}
				continue
			}
			if tab.SessionID != bound.SessionID || tab.DraftID != bound.DraftID {
				return errors.New("conversation tab session and draft identity are immutable outside first Send")
			}
			if tab.SessionID == "" {
				continue
			}
			session, e := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, tab.SessionID)
			if e != nil {
				return e
			}
			if session.ID != tab.SessionID || session.Workspace != next.Scope.Path {
				return errors.New("conversation tab session belongs to another workspace")
			}
		}
		value = hub.CloneConversationTabSet(next)
		value.Revision = expected + 1
		return writeConversationTabs(ctx, tx, value)
	})
	if err != nil {
		return hub.ConversationTabSet{}, err
	}
	return hub.CloneConversationTabSet(value), nil
}

func validateConversationTabOwnership(ctx context.Context, tx *sql.Tx, next hub.ConversationTabSet) error {
	rows, err := tx.QueryContext(ctx, `SELECT repository,path,revision,data FROM hub_conversation_tab_sets WHERE NOT (repository=? AND path=?)`, next.Scope.RepositoryID, next.Scope.Path)
	if err != nil {
		return err
	}
	defer rows.Close()
	ids, drafts := make(map[string]bool, len(next.Tabs)), make(map[string]bool, len(next.Tabs))
	for _, tab := range next.Tabs {
		ids[tab.ID] = true
		if tab.DraftID != "" {
			drafts[tab.DraftID] = true
		}
	}
	for rows.Next() {
		var repository, path string
		var revision uint64
		var data []byte
		if err = rows.Scan(&repository, &path, &revision, &data); err != nil {
			return err
		}
		scope := hub.WorktreeScope{RepositoryID: repository, Path: path}
		value, decodeErr := decodeConversationTabSet(data, scope, revision)
		if decodeErr != nil {
			return decodeErr
		}
		for _, tab := range value.Tabs {
			if ids[tab.ID] || tab.DraftID != "" && drafts[tab.DraftID] {
				return errors.New("conversation tab or home draft identity is already owned by another worktree")
			}
		}
	}
	return rows.Err()
}

func (s *Store) AdmitConversationTab(ctx context.Context, captured hub.ComposerDraft, title string, scope hub.WorktreeScope, tabID string, expected uint64, target hub.Target) (value hub.ConversationTabAdmission, err error) {
	defer func() { err = normalizeStoreError(err) }()
	captured = hub.CloneComposerDraft(captured)
	if err = captured.Validate(); err != nil || captured.Scope.Kind != "home" || captured.Revision == 0 || captured.Revision >= 1<<63-1 ||
		scope.Validate() != nil || tabID == "" || len(tabID) > hub.ConversationMaxTabID || captured.Scope.ID != hub.ConversationTabDraftID(tabID) ||
		strings.TrimSpace(title) == "" || len(title) > 256 || !utf8.ValidString(title) || strings.ContainsRune(title, 0) ||
		expected >= 1<<63-1 || target.Validate() != nil {
		return hub.ConversationTabAdmission{}, errors.New("conversation tab admission requires its captured home draft, scope, tab and target")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		tabs, e := conversationTabs(ctx, tx, scope)
		if e != nil {
			return e
		}
		index := -1
		for i, tab := range tabs.Tabs {
			if tab.ID == tabID {
				index = i
				break
			}
		}
		if index < 0 || tabs.Tabs[index].Archived || tabs.Tabs[index].DraftID != captured.Scope.ID {
			return hub.ErrConversationTabsConflict
		}
		tab := tabs.Tabs[index]
		current, e := composerDraft(ctx, tx, captured.Scope)
		if e != nil {
			return e
		}
		if current.PendingSessionID != "" {
			session, e := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, current.PendingSessionID)
			if e != nil {
				return e
			}
			branch, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=?`, session.ActiveBranchID)
			if e != nil {
				return e
			}
			if session.ID != current.PendingSessionID || session.Workspace != scope.Path ||
				branch.SessionID != session.ID || branch.ID != session.ActiveBranchID || hub.RouteKey(branch.Target) != hub.RouteKey(target) {
				return errors.New("pending conversation tab session workspace or target changed")
			}
			check := hub.CloneComposerDraft(current)
			check.Revision, check.PendingSessionID = captured.Revision, captured.PendingSessionID
			retryOriginal := captured.PendingSessionID == "" && current.Revision == captured.Revision+1 && tabs.Revision == expected+1
			retryCommitted := captured.PendingSessionID == session.ID && current.Revision == captured.Revision && tabs.Revision == expected
			if !reflect.DeepEqual(check, captured) || !retryOriginal && !retryCommitted || tab.SessionID != session.ID {
				return hub.ErrConversationTabsConflict
			}
			value = hub.ConversationTabAdmission{Session: session, Draft: hub.CloneComposerDraft(current), Tabs: hub.CloneConversationTabSet(tabs)}
			return nil
		}
		if tab.SessionID != "" {
			return errors.New("conversation tab is already bound to a session without its pending draft")
		}
		if tabs.Revision != expected || current.Revision != captured.Revision || !reflect.DeepEqual(current, captured) {
			return hub.ErrConversationTabsConflict
		}
		session, e := createSessionTx(ctx, tx, title, scope.Path, target)
		if e != nil {
			return e
		}
		draft := hub.CloneComposerDraft(current)
		draft.Revision++
		draft.PendingSessionID = session.ID
		if e = writeComposerDraft(ctx, tx, draft); e != nil {
			return e
		}
		if e = bindPendingComposerFiles(ctx, tx, draft); e != nil {
			return e
		}
		updated := hub.CloneConversationTabSet(tabs)
		updated.Tabs[index].SessionID = session.ID
		updated.Revision++
		if e = updated.Validate(); e != nil {
			return e
		}
		if e = writeConversationTabs(ctx, tx, updated); e != nil {
			return e
		}
		value = hub.ConversationTabAdmission{Session: session, Draft: hub.CloneComposerDraft(draft), Tabs: hub.CloneConversationTabSet(updated)}
		return nil
	})
	if err != nil {
		return hub.ConversationTabAdmission{}, err
	}
	return value, nil
}

var (
	_ hub.DeveloperRepositoryStore      = (*Store)(nil)
	_ hub.ConversationTabStore          = (*Store)(nil)
	_ hub.ConversationTabAdmissionStore = (*Store)(nil)
)
