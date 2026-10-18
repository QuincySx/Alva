// INPUT: Private product session metadata and bounded literal query/cursor
// OUTPUT: PageSessions with keyset pagination over all durable sessions
// POS: [L7 Hub product] Home history discovery never restores runtimes or loads native conversation bodies
package sqlite

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/apps/hub"
	"unicode/utf8"
)

func (s *Store) PageSessions(ctx context.Context, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	var page hub.SessionPage
	if len(query) > 256 || !utf8.ValidString(query) || limit < 1 || limit > 128 || cursor.Updated < 0 || len(cursor.ID) > 128 {
		return page, errors.New("bounded session query/cursor/limit required")
	}
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM hub_sessions WHERE NOT EXISTS(SELECT 1 FROM hub_workers WHERE hub_workers.session=hub_sessions.id) AND (?='' OR instr(lower(json_extract(data,'$.title')),lower(?))>0 OR instr(lower(json_extract(data,'$.workspace')),lower(?))>0) AND (?=0 OR updated<? OR (updated=? AND id<?)) ORDER BY updated DESC,id DESC LIMIT ?`, query, query, query, cursor.Updated, cursor.Updated, cursor.Updated, cursor.ID, limit+1)
	if err != nil {
		return page, err
	}
	defer rows.Close()
	for rows.Next() {
		var data []byte
		if err = rows.Scan(&data); err != nil {
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
