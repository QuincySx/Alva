// INPUT: Shared immutable Goal policy, explicit scope and selected SQLite transactions.
// OUTPUT: Durable goal IDs, atomic idempotent creation receipts and bounded scope-specific cold reads.
// POS: [L7 SQLite adapter] Saves desired outcomes independently of execution, Team and native records.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func validateGoalsSchema(ctx context.Context, tx *sql.Tx) error {
	rows, err := tx.QueryContext(ctx, `SELECT id,request,created,scope_kind,workspace,session,data FROM hub_goals LIMIT 0`)
	if err != nil {
		return err
	}
	return errors.Join(rows.Err(), rows.Close())
}

func goal(ctx context.Context, q querier, id string) (hub.Goal, error) {
	g, err := read[hub.Goal](ctx, q, `SELECT data FROM hub_goals WHERE id=?`, id)
	if err != nil {
		return hub.Goal{}, err
	}
	var request, kind, workspace, session string
	var created int64
	if err = q.QueryRowContext(ctx, `SELECT request,scope_kind,workspace,session,created FROM hub_goals WHERE id=?`, id).Scan(&request, &kind, &workspace, &session, &created); err != nil {
		return hub.Goal{}, err
	}
	if g.ID != id || g.RequestID != request || string(g.Scope.Kind) != kind || g.Scope.Workspace != workspace || g.Scope.SessionID != session || g.CreatedAt != created || g.Validate() != nil {
		return hub.Goal{}, errors.New("product goal is corrupt")
	}
	return g, nil
}

func validateGoalScope(ctx context.Context, q querier, scope hub.GoalScope) error {
	if err := scope.Validate(); err != nil {
		return err
	}
	if scope.Kind == hub.GoalSessionScope {
		session, err := read[hub.Session](ctx, q, `SELECT data FROM hub_sessions WHERE id=?`, scope.SessionID)
		if err != nil {
			return err
		}
		if session.ID != scope.SessionID || session.Workspace != scope.Workspace {
			return hub.ErrGoalScope
		}
	}
	return nil
}

func (s *Store) CreateGoal(ctx context.Context, request hub.GoalCreate) (receipt hub.GoalReceipt, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = request.Validate(); err != nil {
		return hub.GoalReceipt{}, err
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if err := validateGoalScope(ctx, tx, request.Scope); err != nil {
			return err
		}
		var existing string
		readErr := tx.QueryRowContext(ctx, `SELECT id FROM hub_goals WHERE request=?`, request.RequestID).Scan(&existing)
		if readErr == nil {
			g, err := goal(ctx, tx, existing)
			if err != nil {
				return err
			}
			if g.Creation() != request {
				return hub.ErrGoalConflict
			}
			receipt = hub.GoalReceipt{Goal: g, Replayed: true}
			return nil
		}
		if !errors.Is(readErr, sql.ErrNoRows) {
			return readErr
		}
		id, err := hub.NewID("goal-")
		if err != nil {
			return err
		}
		g := hub.Goal{ID: id, Revision: 1, RequestID: request.RequestID, Name: request.Name, CompletionCriteria: request.CompletionCriteria, Scope: request.Scope, CreatedAt: now()}
		data, err := body(g)
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_goals(id,request,created,scope_kind,workspace,session,data) VALUES(?,?,?,?,?,?,?)`, g.ID, g.RequestID, g.CreatedAt, g.Scope.Kind, g.Scope.Workspace, g.Scope.SessionID, data); err != nil {
			return err
		}
		receipt = hub.GoalReceipt{Goal: g}
		return nil
	})
	if err != nil {
		return hub.GoalReceipt{}, err
	}
	return receipt, nil
}

func (s *Store) Goal(ctx context.Context, id string) (value hub.Goal, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { value, err = goal(ctx, tx, id); return err })
	if err != nil {
		return hub.Goal{}, err
	}
	return value, nil
}

func (s *Store) ListGoals(ctx context.Context, scope hub.GoalScope, limit int) (values []hub.Goal, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if limit < 1 || limit > 100 {
		return nil, errors.New("goal page size must be 1..100")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if err := validateGoalScope(ctx, tx, scope); err != nil {
			return err
		}
		rows, err := tx.QueryContext(ctx, `SELECT id FROM hub_goals WHERE scope_kind=? AND workspace=? AND session=? ORDER BY created DESC,id DESC LIMIT ?`, scope.Kind, scope.Workspace, scope.SessionID, limit)
		if err != nil {
			return err
		}
		var ids []string
		for rows.Next() {
			var id string
			if err = rows.Scan(&id); err != nil {
				rows.Close()
				return err
			}
			ids = append(ids, id)
		}
		if err = errors.Join(rows.Err(), rows.Close()); err != nil {
			return err
		}
		values = make([]hub.Goal, 0, len(ids))
		for _, id := range ids {
			g, err := goal(ctx, tx, id)
			if err != nil {
				return err
			}
			if g.Scope != scope {
				return hub.ErrGoalScope
			}
			values = append(values, g)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return values, nil
}

var _ hub.GoalStore = (*Store)(nil)
