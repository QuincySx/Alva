// INPUT: Explicit engine/native execution identity and committed product branch routing
// OUTPUT: NativeOwner narrow metadata resolver for actual adapter permission dispatch
// POS: [L7 Hub product] Routing metadata only, not native record access or a UI-selected authorization grant
package sqlite

import (
	"context"
	"database/sql"
	"errors"
)

func (s *Store) NativeOwner(ctx context.Context, engineID, nativeID string) (string, error) {
	if engineID == "" || nativeID == "" || len(engineID) > 256 || len(nativeID) > 256 {
		return "", errors.New("bounded actual engine/native identity required")
	}
	rows, err := s.db.QueryContext(ctx, `SELECT session FROM hub_branches WHERE json_extract(data,'$.target.engine_id')=? AND json_extract(data,'$.native_session_id')=? LIMIT 2`, engineID, nativeID)
	if err != nil {
		return "", err
	}
	defer rows.Close()
	var owner string
	count := 0
	for rows.Next() {
		count++
		if err = rows.Scan(&owner); err != nil {
			return "", err
		}
	}
	if err = rows.Err(); err != nil {
		return "", err
	}
	if count == 0 {
		return "", sql.ErrNoRows
	}
	if count != 1 {
		return "", errors.New("native engine identity has ambiguous product branch routing")
	}
	return owner, nil
}
