// INPUT: Private product session metadata and bounded literal query/cursor
// OUTPUT: PageSessions with keyset pagination over all durable ordinary sessions, plus rename, pin and soft-delete management
// POS: [L7 Hub product] Home history discovery never restores runtimes or loads native conversation bodies; list reads use the dedicated read connection
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// notDeletedFilter hides a soft-deleted session from every list and search.
const notDeletedFilter = `coalesce(json_extract(data,'$.deleted_at'),0)=0`

// liveSessionQuery reads one session that is not soft-deleted. Every product
// read and write of a session goes through it, so a deleted session answers
// NotFound everywhere (settings, plan mode, fallback, shell, admission, ...)
// and only rawSessionQuery (DetailIncludingDeleted) can still see it.
const liveSessionQuery = `SELECT data FROM hub_sessions WHERE id=? AND coalesce(json_extract(data,'$.deleted_at'),0)=0`
const rawSessionQuery = `SELECT data FROM hub_sessions WHERE id=?`

const ordinarySessionFilter = notDeletedFilter + ` AND NOT EXISTS(SELECT 1 FROM hub_workers WHERE hub_workers.session=hub_sessions.id) AND NOT EXISTS(SELECT 1 FROM hub_companion_conversations WHERE hub_companion_conversations.session=hub_sessions.id)`

func (s *Store) PageSessions(ctx context.Context, query string, cursor hub.SessionCursor, limit int) (providerResult0 hub.SessionPage, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var page hub.SessionPage
	if len(query) > 256 || !utf8.ValidString(query) || limit < 1 || limit > 128 || cursor.Updated < 0 || len(cursor.ID) > 128 {
		return page, errors.New("bounded session query/cursor/limit required")
	}
	rows, err := s.list.QueryContext(ctx, `SELECT data FROM hub_sessions WHERE `+ordinarySessionFilter+` AND (?='' OR instr(lower(json_extract(data,'$.title')),lower(?))>0 OR instr(lower(json_extract(data,'$.workspace')),lower(?))>0) AND (?=0 OR updated<? OR (updated=? AND id<?)) ORDER BY updated DESC,id DESC LIMIT ?`, query, query, query, cursor.Updated, cursor.Updated, cursor.Updated, cursor.ID, limit+1)
	if err != nil {
		return page, err
	}
	defer rows.Close()
	return readSessionPage(rows, limit)
}

func readSessionPage(rows *sql.Rows, limit int) (hub.SessionPage, error) {
	var page hub.SessionPage
	for rows.Next() {
		var data []byte
		if err := rows.Scan(&data); err != nil {
			return page, err
		}
		if len(page.Sessions) == limit {
			page.HasMore = true
			break
		}
		var item hub.Session
		if len(data) > 128<<10 || json.Unmarshal(data, &item) != nil {
			return page, errors.New("product session metadata is corrupt")
		}
		page.Sessions = append(page.Sessions, item)
		page.Next = hub.SessionCursor{Updated: item.UpdatedAt, ID: item.ID}
	}
	return page, rows.Err()
}

// PinnedSessions lists the pinned ordinary sessions, newest activity first.
func (s *Store) PinnedSessions(ctx context.Context) (providerResult0 []hub.Session, providerResult1 error) {
	defer func() { providerResult1 = normalizeStoreError(providerResult1) }()
	rows, err := s.list.QueryContext(ctx, `SELECT data FROM hub_sessions WHERE `+ordinarySessionFilter+` AND json_extract(data,'$.pinned')=1 ORDER BY updated DESC,id DESC LIMIT ?`, hub.MaxPinnedSessions)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	page, err := readSessionPage(rows, hub.MaxPinnedSessions)
	return page.Sessions, err
}

// RenameSession replaces the title without touching UpdatedAt or Status.
func (s *Store) RenameSession(ctx context.Context, id, title string) (result error) {
	defer func() { result = normalizeStoreError(result) }()
	if title != strings.TrimSpace(title) || title == "" || len(title) > 256 || !utf8.ValidString(title) || hub.TitleHasControl(title) {
		return errors.New("session title must be one trimmed UTF-8 line of 1..256 bytes without control characters")
	}
	return s.manageSession(ctx, id, func(_ context.Context, _ *sql.Tx, session *hub.Session) error {
		session.Title = title
		return nil
	})
}

// PinSession pins or unpins one session; the pinned group is bounded.
func (s *Store) PinSession(ctx context.Context, id string, pinned bool) (result error) {
	defer func() { result = normalizeStoreError(result) }()
	return s.manageSession(ctx, id, func(ctx context.Context, tx *sql.Tx, session *hub.Session) error {
		if pinned && !session.Pinned {
			var count int
			if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_sessions WHERE `+ordinarySessionFilter+` AND json_extract(data,'$.pinned')=1`).Scan(&count); err != nil {
				return err
			}
			if count >= hub.MaxPinnedSessions {
				return hub.ErrPinnedBudget
			}
		}
		session.Pinned = pinned
		return nil
	})
}

// DeleteSession soft-deletes an idle session: it disappears from lists,
// search and navigation, while every fact and native record stays on disk.
func (s *Store) DeleteSession(ctx context.Context, id string) (result error) {
	defer func() { result = normalizeStoreError(result) }()
	return s.manageSession(ctx, id, func(ctx context.Context, tx *sql.Tx, session *hub.Session) error {
		if hub.BlocksInput(session.Status) {
			return hub.ErrSessionRunning
		}
		if err := retireBackgroundWork(ctx, tx, id); err != nil {
			return err
		}
		session.DeletedAt = now()
		session.Pinned = false
		return nil
	})
}

// manageSession applies one metadata change inside the transaction that read
// the session, so a concurrent Turn admission and a delete cannot interleave.
// Worker and companion sessions are not user-managed conversations.
func (s *Store) manageSession(ctx context.Context, id string, change func(context.Context, *sql.Tx, *hub.Session) error) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if errors.Is(err, sql.ErrNoRows) || err == nil && session.DeletedAt != 0 {
			return hub.ErrNotFound
		}
		if err != nil {
			return err
		}
		var managed int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_sessions WHERE id=? AND `+ordinarySessionFilter, id).Scan(&managed); err != nil {
			return err
		}
		if managed != 1 {
			return hub.ErrNotFound
		}
		if err = change(ctx, tx, &session); err != nil {
			return err
		}
		return writeSession(ctx, tx, session)
	})
}

// LatestTurn reads the newest Turn on the read connection.
func (s *Store) LatestTurn(ctx context.Context, id string) (result hub.Turn, err error) {
	defer func() { err = normalizeStoreError(err) }()
	session, err := read[hub.Session](ctx, s.list, liveSessionQuery, id)
	if err != nil {
		return result, err
	}
	predicate, args, err := versionPredicate(ctx, s.list, session, "seq", true)
	if err != nil {
		return result, err
	}
	filter := ""
	if predicate != "" {
		filter = " AND " + predicate
	}
	result, err = read[hub.Turn](ctx, s.list, `SELECT data FROM hub_turns WHERE session=?`+filter+` ORDER BY seq DESC LIMIT 1`, append([]any{id}, args...)...)
	if errors.Is(err, sql.ErrNoRows) {
		return result, hub.ErrNotFound
	}
	return result, err
}

// retireBackgroundWork runs inside the delete transaction. A led Team with a
// running or waiting Worker (or a running Worker task) refuses the delete;
// every enabled automation of the session is paused with the reason, so a
// deleted conversation never keeps firing in the background.
func retireBackgroundWork(ctx context.Context, tx *sql.Tx, id string) error {
	var busy int
	if err := tx.QueryRowContext(ctx, `SELECT (SELECT count(*) FROM hub_workers w JOIN hub_teams t ON t.id=w.team JOIN hub_sessions s ON s.id=w.session WHERE t.lead=? AND s.status IN (?,?)) + (SELECT count(*) FROM hub_worker_tasks k JOIN hub_teams t ON t.id=k.team WHERE t.lead=? AND k.status=?)`,
		id, hub.Running, engine.Waiting, id, hub.Running).Scan(&busy); err != nil {
		return err
	}
	if busy != 0 {
		return hub.ErrSessionHasWorkers
	}
	rows, err := tx.QueryContext(ctx, `SELECT id FROM hub_automations WHERE session=? AND enabled=1 LIMIT 101`, id)
	if err != nil {
		return err
	}
	var ids []string
	for rows.Next() {
		var automationID string
		if err = rows.Scan(&automationID); err != nil {
			break
		}
		ids = append(ids, automationID)
	}
	if err = errors.Join(err, rows.Err(), rows.Close()); err != nil {
		return err
	}
	for _, automationID := range ids {
		a, err := automation(ctx, tx, id, automationID)
		if err != nil {
			return err
		}
		a.Enabled, a.NextRunAt, a.LastError = false, 0, hub.AutomationDeletedReason
		a.Revision++
		if err = writeAutomation(ctx, tx, a); err != nil {
			return err
		}
	}
	return nil
}
