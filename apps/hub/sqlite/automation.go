// INPUT: Private product SQLite transactions, ordinary sessions and exact authorized route snapshots
// OUTPUT: Optimistic paused-first schedules, atomic normal Turn claims and crash-safe settlement
// POS: [L7 Hub product] Sole automation fact authority; no OS daemon, catch-up queue or model/provider invocation
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

var errAutomationScoped = errors.New("automations require an ordinary session, not a Team Lead or Worker")

func automation(ctx context.Context, q querier, owner, id string) (hub.Automation, error) {
	a, err := read[hub.Automation](ctx, q, `SELECT data FROM hub_automations WHERE id=? AND session=?`, id, owner)
	if err != nil {
		return a, err
	}
	if a.ID != id || a.SessionID != owner || a.Revision == 0 || a.AutomationDraft.Validate() != nil || a.Target.Validate() != nil || a.BranchID == "" {
		return hub.Automation{}, errors.New("automation metadata is corrupt")
	}
	return a, nil
}

func writeAutomation(ctx context.Context, tx *sql.Tx, a hub.Automation) error {
	data, err := body(a)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_automations SET enabled=?,next=?,running=?,data=? WHERE id=? AND session=?`, a.Enabled, a.NextRunAt, a.RunningTurnID, data, a.ID, a.SessionID)
	return err
}

func ordinaryAutomationSession(ctx context.Context, tx *sql.Tx, id string) (hub.Session, hub.Branch, error) {
	s, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
	if err != nil {
		return s, hub.Branch{}, err
	}
	var scoped int
	if err = tx.QueryRowContext(ctx, `SELECT (SELECT count(*) FROM hub_teams WHERE lead=?)+(SELECT count(*) FROM hub_workers WHERE session=?)`, id, id).Scan(&scoped); err != nil {
		return s, hub.Branch{}, err
	}
	if scoped != 0 {
		return s, hub.Branch{}, errAutomationScoped
	}
	b, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, s.ActiveBranchID, id)
	return s, b, err
}

func nextAutomation(at, minutes int64) (int64, error) {
	interval := minutes * 60_000
	if at <= 0 || at > math.MaxInt64-interval {
		return 0, errors.New("automation clock is outside its supported range")
	}
	return at + interval, nil
}

func (s *Store) Automation(ctx context.Context, owner, id string) (hub.Automation, error) {
	return automation(ctx, s.db, owner, id)
}

func (s *Store) ListAutomations(ctx context.Context, owner string) ([]hub.Automation, error) {
	if _, err := read[hub.Session](ctx, s.db, `SELECT data FROM hub_sessions WHERE id=?`, owner); err != nil {
		return nil, err
	}
	rows, err := s.db.QueryContext(ctx, `SELECT id FROM hub_automations WHERE session=? ORDER BY rowid DESC LIMIT 101`, owner)
	if err != nil {
		return nil, err
	}
	var ids []string
	for rows.Next() {
		var id string
		if err = rows.Scan(&id); err != nil {
			break
		}
		ids = append(ids, id)
	}
	err = errors.Join(err, rows.Err(), rows.Close())
	if err != nil {
		return nil, err
	}
	if len(ids) > 100 {
		return nil, errors.New("automation metadata exceeds its 100-config budget")
	}
	result := make([]hub.Automation, 0, len(ids))
	for _, id := range ids {
		a, err := s.Automation(ctx, owner, id)
		if err != nil {
			return nil, err
		}
		result = append(result, a)
	}
	return result, nil
}

func (s *Store) CreateAutomation(ctx context.Context, owner string, draft hub.AutomationDraft) (hub.Automation, error) {
	var a hub.Automation
	if err := draft.Validate(); err != nil {
		return a, err
	}
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var count int
		if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_automations`).Scan(&count); err != nil {
			return err
		}
		if count >= 100 {
			return errors.New("at most 100 automations are supported")
		}
		_, branch, err := ordinaryAutomationSession(ctx, tx, owner)
		if err != nil {
			return err
		}
		id, err := hub.NewID("automation-")
		if err != nil {
			return err
		}
		draft.Title = strings.TrimSpace(draft.Title)
		a = hub.Automation{ID: id, SessionID: owner, AutomationDraft: draft, Revision: 1, BranchID: branch.ID, Target: branch.Target, LastStatus: "paused"}
		data, err := body(a)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `INSERT INTO hub_automations VALUES(?,?,?,?,?,?)`, id, owner, false, 0, "", data)
		return err
	})
	return a, err
}

func (s *Store) UpdateAutomation(ctx context.Context, owner, id string, revision uint64, draft hub.AutomationDraft) (hub.Automation, error) {
	var a hub.Automation
	if err := draft.Validate(); err != nil {
		return a, err
	}
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		a, err = automation(ctx, tx, owner, id)
		if err != nil {
			return err
		}
		if a.Revision != revision {
			return hub.ErrAutomationStale
		}
		if a.RunningTurnID != "" {
			return errors.New("wait for the current automation Turn before editing")
		}
		_, branch, err := ordinaryAutomationSession(ctx, tx, owner)
		if err != nil {
			return err
		}
		draft.Title = strings.TrimSpace(draft.Title)
		a.AutomationDraft, a.BranchID, a.Target = draft, branch.ID, branch.Target
		a.Enabled, a.NextRunAt, a.LastStatus, a.LastError = false, 0, "paused", ""
		a.Revision++
		return writeAutomation(ctx, tx, a)
	})
	return a, err
}

func (s *Store) SetAutomationEnabled(ctx context.Context, owner, id string, revision uint64, enabled, accountAndBillingApproved bool) (hub.Automation, error) {
	var a hub.Automation
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		a, err = automation(ctx, tx, owner, id)
		if err != nil {
			return err
		}
		if a.Revision != revision {
			return hub.ErrAutomationStale
		}
		if enabled {
			if !accountAndBillingApproved {
				return errors.New("explicit automation account/billing approval required")
			}
			if a.RunningTurnID != "" {
				return errors.New("wait for the current automation Turn before enabling")
			}
			_, branch, err := ordinaryAutomationSession(ctx, tx, owner)
			if err != nil {
				return err
			}
			if a.BranchID != branch.ID || hub.RouteKey(a.Target) != hub.RouteKey(branch.Target) {
				return hub.ErrAutomationRouteChanged
			}
			if a.Target.ProviderID == "workspace-config" || a.Target.ProviderID == "pi-unconfigured" || a.Target.ModelID == "engine-default" || a.Target.ProviderID == "engine-default" {
				return errors.New("automation requires an explicit configured model/account route, not an external default")
			}
			a.NextRunAt, err = nextAutomation(now(), a.IntervalMinutes)
			if err != nil {
				return err
			}
		} else {
			a.NextRunAt = 0
		}
		a.Enabled = enabled
		a.Revision++
		return writeAutomation(ctx, tx, a)
	})
	return a, err
}

func (s *Store) DeleteAutomation(ctx context.Context, owner, id string, revision uint64) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		a, err := automation(ctx, tx, owner, id)
		if err != nil {
			return err
		}
		if a.Revision != revision {
			return hub.ErrAutomationStale
		}
		if a.RunningTurnID != "" {
			return errors.New("wait for the current automation Turn before deleting its configuration")
		}
		_, err = tx.ExecContext(ctx, `DELETE FROM hub_automations WHERE id=? AND session=?`, id, owner)
		return err
	})
}

func (s *Store) DueAutomations(ctx context.Context, at int64) ([]hub.AutomationTick, error) {
	rows, err := s.db.QueryContext(ctx, `SELECT id,session,json_extract(data,'$.revision') FROM hub_automations WHERE enabled=1 AND running='' AND next>0 AND next<=? ORDER BY next,rowid LIMIT 100`, at)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var ticks []hub.AutomationTick
	for rows.Next() {
		var tick hub.AutomationTick
		if err = rows.Scan(&tick.ID, &tick.SessionID, &tick.Revision); err != nil {
			return nil, err
		}
		ticks = append(ticks, tick)
	}
	return ticks, rows.Err()
}

func dueAutomation(ctx context.Context, tx *sql.Tx, tick hub.AutomationTick, at int64) (hub.Automation, int64, error) {
	a, err := automation(ctx, tx, tick.SessionID, tick.ID)
	if err != nil {
		return a, 0, err
	}
	if a.Revision != tick.Revision || !a.Enabled || a.RunningTurnID != "" || a.NextRunAt == 0 || a.NextRunAt > at {
		return a, 0, hub.ErrAutomationStale
	}
	next, err := nextAutomation(at, a.IntervalMinutes)
	return a, next, err
}

func (s *Store) ClaimAutomation(ctx context.Context, tick hub.AutomationTick, at int64) (hub.Claim, error) {
	var claim hub.Claim
	var rejected error
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		a, next, err := dueAutomation(ctx, tx, tick, at)
		if err != nil {
			return err
		}
		current, branch, scopeErr := ordinaryAutomationSession(ctx, tx, tick.SessionID)
		if scopeErr != nil || branch.ID != a.BranchID || hub.RouteKey(branch.Target) != hub.RouteKey(a.Target) {
			// A corrupt/read-failed fact is not a normal route change. Fail the
			// transaction rather than inventing a valid scope from missing data.
			if scopeErr != nil && !errors.Is(scopeErr, errAutomationScoped) {
				return scopeErr
			}
			a.Enabled, a.NextRunAt, a.LastStatus = false, 0, "route_changed"
			a.LastError = "Session route or collaboration scope changed; edit and review again"
			rejected = hub.ErrAutomationRouteChanged
		} else if current.Status == hub.Running || at-a.NextRunAt >= a.IntervalMinutes*60_000 {
			a.NextRunAt, a.LastStatus = next, "skipped_missed"
			if current.Status == hub.Running {
				a.LastStatus = "skipped_busy"
			}
			rejected = hub.ErrAutomationSkipped
		} else {
			claim, err = beginTurn(ctx, tx, a.SessionID, a.Prompt)
			if err != nil {
				return err
			}
			claim.Turn.AutomationID = a.ID
			if err = writeTurn(ctx, tx, claim.Turn); err != nil {
				return err
			}
			a.RunningTurnID, a.LastRunAt, a.NextRunAt, a.LastStatus, a.LastError = claim.Turn.ID, at, 0, "running", ""
		}
		a.Revision++
		return writeAutomation(ctx, tx, a)
	})
	return claim, errors.Join(err, rejected)
}

func (s *Store) SkipBusyAutomation(ctx context.Context, tick hub.AutomationTick, at int64) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		a, next, err := dueAutomation(ctx, tx, tick, at)
		if err != nil {
			return err
		}
		a.NextRunAt, a.LastStatus = next, "skipped_busy"
		a.Revision++
		return writeAutomation(ctx, tx, a)
	})
}

func (s *Store) PauseInvalidAutomation(ctx context.Context, tick hub.AutomationTick) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		a, err := automation(ctx, tx, tick.SessionID, tick.ID)
		if err != nil {
			return err
		}
		if a.Revision != tick.Revision || !a.Enabled || a.RunningTurnID != "" {
			return hub.ErrAutomationStale
		}
		a.Enabled, a.NextRunAt, a.LastStatus, a.LastError = false, 0, "failed", "Configured route is unavailable or execution admission failed; review and enable again"
		a.Revision++
		return writeAutomation(ctx, tx, a)
	})
}

func settleAutomation(ctx context.Context, tx *sql.Tx, turn hub.Turn) error {
	if turn.AutomationID == "" {
		return nil
	}
	a, err := automation(ctx, tx, turn.SessionID, turn.AutomationID)
	if err != nil {
		return err
	}
	if a.RunningTurnID != turn.ID {
		return errors.New("automation settlement does not match its actual running Turn")
	}
	a.RunningTurnID, a.NextRunAt = "", 0
	if turn.Status == engine.Succeeded {
		a.LastStatus, a.LastError = "completed", ""
		if a.Enabled {
			a.NextRunAt, err = nextAutomation(now(), a.IntervalMinutes)
			if err != nil {
				return err
			}
		}
	} else {
		a.Enabled, a.LastStatus, a.LastError = false, "failed", "Execution did not complete successfully; review and enable again"
		if turn.Status == engine.Cancelled || turn.Status == engine.Interrupted {
			a.LastStatus = "interrupted"
		}
	}
	a.Revision++
	return writeAutomation(ctx, tx, a)
}

func recoverAutomations(ctx context.Context, tx *sql.Tx) error {
	rows, err := tx.QueryContext(ctx, `SELECT id,session FROM hub_automations WHERE enabled=1 OR running<>'' LIMIT 101`)
	if err != nil {
		return err
	}
	var keys []hub.AutomationTick
	for rows.Next() {
		var k hub.AutomationTick
		if err = rows.Scan(&k.ID, &k.SessionID); err != nil {
			break
		}
		keys = append(keys, k)
	}
	err = errors.Join(err, rows.Err(), rows.Close())
	if err != nil {
		return err
	}
	if len(keys) > 100 {
		return errors.New("automation recovery budget exceeded")
	}
	for _, k := range keys {
		a, err := automation(ctx, tx, k.SessionID, k.ID)
		if err != nil {
			return err
		}
		if a.RunningTurnID != "" {
			a.Enabled, a.NextRunAt, a.RunningTurnID, a.LastStatus = false, 0, "", "interrupted"
			a.LastError = "Application exited before automation settlement; native history remains authoritative, no execution is replayed"
		} else {
			a.NextRunAt, err = nextAutomation(now(), a.IntervalMinutes)
			if err != nil {
				return err
			}
		}
		a.Revision++
		if err = writeAutomation(ctx, tx, a); err != nil {
			return err
		}
	}
	return nil
}

var _ hub.AutomationStore = (*Store)(nil)
