// INPUT: Saved ordinary-session workspace metadata and the bounded automation table.
// OUTPUT: Stable workspace/path pages, exact workspace session pages and a cold global schedule index.
// POS: [L7 SQLite product provider] Reuses session/automation policies; navigation never executes or scans user directories.
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *Store) PageWorkspaces(ctx context.Context, query hub.WorkspaceNavigationQuery, cursor hub.WorkspaceCursor, limit int) (page hub.WorkspacePage, result error) {
	defer func() { result = normalizeStoreError(result) }()
	if err := hub.ValidateWorkspaceNavigation(query, cursor, limit); err != nil {
		return page, err
	}
	prefix := strings.TrimRight(query.ExcludeRoot, string(filepath.Separator)) + string(filepath.Separator)
	rows, err := s.list.QueryContext(ctx, `SELECT json_extract(data,'$.workspace') AS workspace, count(*), max(updated) FROM hub_sessions WHERE `+ordinarySessionFilter+` AND json_extract(data,'$.workspace')>? AND (?='' OR instr(lower(json_extract(data,'$.workspace')),lower(?))>0) AND (?='' OR (json_extract(data,'$.workspace')<>? AND substr(json_extract(data,'$.workspace'),1,length(?))<>?)) GROUP BY json_extract(data,'$.workspace') ORDER BY workspace COLLATE BINARY LIMIT ?`, cursor.Path, query.Query, query.Query, query.ExcludeRoot, query.ExcludeRoot, prefix, prefix, limit+1)
	if err != nil {
		return page, err
	}
	defer rows.Close()
	for rows.Next() {
		var entry hub.WorkspaceEntry
		if err := rows.Scan(&entry.Path, &entry.SessionCount, &entry.LastActiveAt); err != nil {
			return page, err
		}
		if len(page.Items) == limit {
			page.HasMore = true
			break
		}
		if entry.Path == "" || hub.ValidateWorkspaceNavigation(hub.WorkspaceNavigationQuery{}, hub.WorkspaceCursor{Path: entry.Path}, 1) != nil {
			return page, errors.New("saved workspace metadata is not a clean absolute path")
		}
		entry.Name = filepath.Base(entry.Path)
		page.Items = append(page.Items, entry)
		page.Next.Path = entry.Path
	}
	return page, rows.Err()
}

func (s *Store) PageWorkspaceSessions(ctx context.Context, workspace, query string, cursor hub.SessionCursor, limit int) (page hub.SessionPage, result error) {
	defer func() { result = normalizeStoreError(result) }()
	if workspace == "" || hub.ValidateWorkspaceNavigation(hub.WorkspaceNavigationQuery{Query: query}, hub.WorkspaceCursor{Path: workspace}, limit) != nil || cursor.Updated < 0 || len(cursor.ID) > 128 || !utf8.ValidString(cursor.ID) {
		return page, errors.New("bounded exact-workspace session query required")
	}
	rows, err := s.list.QueryContext(ctx, `SELECT data FROM hub_sessions WHERE `+ordinarySessionFilter+` AND json_extract(data,'$.workspace')=? AND (?='' OR instr(lower(json_extract(data,'$.title')),lower(?))>0) AND (?=0 OR updated<? OR (updated=? AND id<?)) ORDER BY updated DESC,id DESC LIMIT ?`, workspace, query, query, cursor.Updated, cursor.Updated, cursor.Updated, cursor.ID, limit+1)
	if err != nil {
		return page, err
	}
	defer rows.Close()
	return readSessionPage(rows, limit)
}

func (s *Store) ListAutomationIndex(ctx context.Context) (items []hub.Automation, result error) {
	defer func() { result = normalizeStoreError(result) }()
	rows, err := s.db.QueryContext(ctx, `SELECT id,session FROM hub_automations ORDER BY rowid DESC LIMIT 101`)
	if err != nil {
		return nil, err
	}
	type identity struct{ id, session string }
	var keys []identity
	for rows.Next() {
		var key identity
		if err = rows.Scan(&key.id, &key.session); err != nil {
			break
		}
		keys = append(keys, key)
	}
	if err = errors.Join(err, rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	if len(keys) > 100 {
		return nil, errors.New("automation index exceeds its 100-config budget")
	}
	for _, key := range keys {
		item, err := s.Automation(ctx, key.session, key.id)
		if err != nil {
			return nil, err
		}
		items = append(items, item)
	}
	return items, nil
}
