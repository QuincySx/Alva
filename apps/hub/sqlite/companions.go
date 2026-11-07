// INPUT: Explicit CompanionStore DTOs and the existing isolated product SQLite transaction.
// OUTPUT: Durable Profile/group facts, tombstone discovery, scoped keyset pages and transactional deleted-execution rejection.
// POS: [L7 Hub storage] Companion identity is independent of Teams; all group membership/session snapshots commit atomically.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

var _ hub.CompanionStore = (*Store)(nil)
var _ hub.CompanionHistoryStore = (*Store)(nil)

func companionSchema(ctx context.Context, tx *sql.Tx) error {
	for _, q := range []string{
		`CREATE TABLE hub_companions(id TEXT PRIMARY KEY,owner TEXT NOT NULL,revision INTEGER NOT NULL,data BLOB NOT NULL)`,
		`CREATE INDEX hub_companion_owner ON hub_companions(owner,id)`,
		`CREATE TABLE hub_companion_groups(id TEXT PRIMARY KEY,owner TEXT NOT NULL,revision INTEGER NOT NULL,data BLOB NOT NULL)`,
		`CREATE INDEX hub_companion_group_owner ON hub_companion_groups(owner,id)`,
		`CREATE TABLE hub_companion_conversations(id TEXT PRIMARY KEY,owner TEXT NOT NULL,companion TEXT NOT NULL REFERENCES hub_companions(id),group_id TEXT NOT NULL,scope_revision INTEGER NOT NULL,session TEXT NOT NULL UNIQUE REFERENCES hub_sessions(id),data BLOB NOT NULL,UNIQUE(owner,companion,group_id,scope_revision))`,
		`CREATE TABLE hub_companion_rounds(id TEXT PRIMARY KEY,owner TEXT NOT NULL,group_id TEXT NOT NULL REFERENCES hub_companion_groups(id),scope_revision INTEGER NOT NULL,status TEXT NOT NULL,created INTEGER NOT NULL,data BLOB NOT NULL)`,
		`CREATE UNIQUE INDEX hub_companion_round_active ON hub_companion_rounds(group_id) WHERE status IN ('running','waiting','interrupted','unknown')`,
		`CREATE TABLE hub_companion_messages(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,owner TEXT NOT NULL,group_id TEXT NOT NULL,scope_revision INTEGER NOT NULL,round_id TEXT NOT NULL REFERENCES hub_companion_rounds(id),data BLOB NOT NULL)`,
		`CREATE INDEX hub_companion_messages_scope ON hub_companion_messages(owner,group_id,scope_revision,seq)`,
	} {
		if _, err := tx.ExecContext(ctx, q); err != nil {
			return err
		}
	}
	return nil
}
func validateCompanionSchema(ctx context.Context, tx *sql.Tx) error {
	for _, q := range []string{`SELECT id,owner,revision,data FROM hub_companions LIMIT 0`, `SELECT id,owner,revision,data FROM hub_companion_groups LIMIT 0`, `SELECT id,owner,companion,group_id,scope_revision,session,data FROM hub_companion_conversations LIMIT 0`, `SELECT id,owner,group_id,scope_revision,status,created,data FROM hub_companion_rounds LIMIT 0`, `SELECT seq,id,owner,group_id,scope_revision,round_id,data FROM hub_companion_messages LIMIT 0`} {
		rows, err := tx.QueryContext(ctx, q)
		if err != nil {
			return err
		}
		if err = rows.Close(); err != nil {
			return err
		}
	}
	return nil
}
func companion(ctx context.Context, q querier, owner, id string) (hub.CompanionProfile, error) {
	p, err := read[hub.CompanionProfile](ctx, q, `SELECT data FROM hub_companions WHERE owner=? AND id=?`, owner, id)
	if err != nil {
		return p, err
	}
	if p.ID != id || p.OwnerID != owner || p.Revision == 0 {
		return p, errors.New("companion identity is corrupt")
	}
	return p, p.Validate()
}
func companionGroup(ctx context.Context, q querier, owner, id string) (hub.CompanionGroup, error) {
	g, err := read[hub.CompanionGroup](ctx, q, `SELECT data FROM hub_companion_groups WHERE owner=? AND id=?`, owner, id)
	if err != nil {
		return g, err
	}
	if g.ID != id || g.OwnerID != owner || g.Revision == 0 || g.ScopeRevision == 0 {
		return g, errors.New("companion group identity is corrupt")
	}
	return g, g.Validate()
}
func (s *Store) Companion(ctx context.Context, owner, id string) (p hub.CompanionProfile, err error) {
	defer func() { err = normalizeStoreError(err) }()
	return companion(ctx, s.db, owner, id)
}
func (s *Store) CompanionGroup(ctx context.Context, owner, id string) (g hub.CompanionGroup, err error) {
	defer func() { err = normalizeStoreError(err) }()
	return companionGroup(ctx, s.db, owner, id)
}
func (s *Store) Companions(ctx context.Context, owner string) ([]hub.CompanionProfile, error) {
	return s.companionProfiles(ctx, owner, false)
}
func (s *Store) companionProfiles(ctx context.Context, owner string, deleted bool) (out []hub.CompanionProfile, err error) {
	defer func() { err = normalizeStoreError(err) }()
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM hub_companions WHERE owner=? ORDER BY json_extract(data,'$.created_at'),id LIMIT 257`, owner)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var b []byte
		var p hub.CompanionProfile
		if err = rows.Scan(&b); err != nil {
			return nil, err
		}
		if len(b) > 128<<10 || json.Unmarshal(b, &p) != nil || p.OwnerID != owner || p.Revision == 0 {
			return nil, errors.New("companion record is corrupt")
		}
		if err = p.Validate(); err != nil {
			return nil, err
		}
		if p.Deleted == deleted {
			out = append(out, p)
		}
	}
	if len(out) > 256 {
		return nil, errors.New("companion roster exceeds 256 records")
	}
	return out, rows.Err()
}
func (s *Store) CompanionGroups(ctx context.Context, owner string) ([]hub.CompanionGroup, error) {
	return s.companionGroups(ctx, owner, false)
}
func (s *Store) companionGroups(ctx context.Context, owner string, deleted bool) (out []hub.CompanionGroup, err error) {
	defer func() { err = normalizeStoreError(err) }()
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM hub_companion_groups WHERE owner=? ORDER BY json_extract(data,'$.created_at'),id LIMIT 257`, owner)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var b []byte
		var g hub.CompanionGroup
		if err = rows.Scan(&b); err != nil {
			return nil, err
		}
		if len(b) > 128<<10 || json.Unmarshal(b, &g) != nil || g.OwnerID != owner || g.Revision == 0 || g.ScopeRevision == 0 {
			return nil, errors.New("companion group record is corrupt")
		}
		if err = g.Validate(); err != nil {
			return nil, err
		}
		if g.Deleted == deleted {
			out = append(out, g)
		}
	}
	if len(out) > 256 {
		return nil, errors.New("group roster exceeds 256 records")
	}
	return out, rows.Err()
}
func (s *Store) SaveCompanion(ctx context.Context, p hub.CompanionProfile, expected uint64) (out hub.CompanionProfile, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = p.Validate(); err != nil {
		return out, err
	}
	if p.Deleted {
		return out, errors.New("use explicit DeleteCompanion")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		if expected == 0 {
			if p.ID != "" {
				return hub.ErrCompanionConflict
			}
			var count int
			if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_companions WHERE owner=?`, p.OwnerID).Scan(&count); err != nil {
				return err
			}
			if count >= 256 {
				return errors.New("companion roster budget exhausted")
			}
			var err error
			p.ID, err = hub.NewID("companion-")
			if err != nil {
				return err
			}
			p.CreatedAt = now()
		} else {
			old, err := companion(ctx, tx, p.OwnerID, p.ID)
			if err != nil {
				return err
			}
			if old.Revision != expected || old.Deleted {
				return hub.ErrCompanionConflict
			}
			p.CreatedAt = old.CreatedAt
			if old.Workspace != p.Workspace {
				var count int
				if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_companion_conversations WHERE companion=? AND group_id=''`, p.ID).Scan(&count); err != nil {
					return err
				}
				if count > 0 {
					return errors.New("private workspace is fixed after opening a conversation")
				}
			}
		}
		p.Revision = expected + 1
		p.UpdatedAt = now()
		b, err := body(p)
		if err != nil {
			return err
		}
		if expected == 0 {
			_, err = tx.ExecContext(ctx, `INSERT INTO hub_companions(id,owner,revision,data) VALUES(?,?,?,?)`, p.ID, p.OwnerID, p.Revision, b)
		} else {
			_, err = tx.ExecContext(ctx, `UPDATE hub_companions SET revision=?,data=? WHERE owner=? AND id=? AND revision=?`, p.Revision, b, p.OwnerID, p.ID, expected)
		}
		out = p
		return err
	})
	return out, err
}
func (s *Store) DeleteCompanion(ctx context.Context, owner, id string, expected uint64) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		p, err := companion(ctx, tx, owner, id)
		if err != nil {
			return err
		}
		if p.Revision != expected || p.Deleted {
			return hub.ErrCompanionConflict
		}
		p.Revision++
		p.Enabled = false
		p.Deleted = true
		p.UpdatedAt = now()
		b, err := body(p)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `UPDATE hub_companions SET revision=?,data=? WHERE owner=? AND id=?`, p.Revision, b, owner, id)
		return err
	})
}
func (s *Store) SaveCompanionGroup(ctx context.Context, g hub.CompanionGroup, expected uint64) (out hub.CompanionGroup, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = g.Validate(); err != nil {
		return out, err
	}
	if g.Deleted {
		return out, errors.New("use explicit DeleteCompanionGroup")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		for _, id := range g.Members {
			p, err := companion(ctx, tx, g.OwnerID, id)
			if err != nil {
				return err
			}
			if p.Deleted {
				return hub.ErrCompanionDisabled
			}
		}
		if expected == 0 {
			if g.ID != "" {
				return hub.ErrCompanionConflict
			}
			var count int
			if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_companion_groups WHERE owner=?`, g.OwnerID).Scan(&count); err != nil {
				return err
			}
			if count >= 256 {
				return errors.New("group roster budget exhausted")
			}
			var err error
			g.ID, err = hub.NewID("group-")
			if err != nil {
				return err
			}
			g.CreatedAt = now()
			g.ScopeRevision = 1
		} else {
			old, err := companionGroup(ctx, tx, g.OwnerID, g.ID)
			if err != nil {
				return err
			}
			if old.Revision != expected || old.Deleted {
				return hub.ErrCompanionConflict
			}
			var active int
			if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_companion_rounds WHERE group_id=? AND status IN ('running','waiting','interrupted','unknown')`, g.ID).Scan(&active); err != nil {
				return err
			}
			if active != 0 {
				return errors.New("finish or stop the group round before editing its membership or policy")
			}
			g.CreatedAt = old.CreatedAt
			g.ScopeRevision = old.ScopeRevision
			if old.Workspace != g.Workspace || !reflect.DeepEqual(old.Members, g.Members) {
				g.ScopeRevision++
			}
		}
		g.Revision = expected + 1
		g.UpdatedAt = now()
		b, err := body(g)
		if err != nil {
			return err
		}
		if expected == 0 {
			_, err = tx.ExecContext(ctx, `INSERT INTO hub_companion_groups(id,owner,revision,data) VALUES(?,?,?,?)`, g.ID, g.OwnerID, g.Revision, b)
		} else {
			_, err = tx.ExecContext(ctx, `UPDATE hub_companion_groups SET revision=?,data=? WHERE owner=? AND id=? AND revision=?`, g.Revision, b, g.OwnerID, g.ID, expected)
		}
		out = g
		return err
	})
	return out, err
}
func (s *Store) DeleteCompanionGroup(ctx context.Context, owner, id string, expected uint64) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		g, err := companionGroup(ctx, tx, owner, id)
		if err != nil {
			return err
		}
		if g.Revision != expected || g.Deleted {
			return hub.ErrCompanionConflict
		}
		var active int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_companion_rounds WHERE group_id=? AND status IN ('running','waiting','interrupted','unknown')`, id).Scan(&active); err != nil {
			return err
		}
		if active > 0 {
			return errors.New("stop group round before deleting")
		}
		g.Revision++
		g.Enabled = false
		g.Deleted = true
		g.UpdatedAt = now()
		b, err := body(g)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `UPDATE hub_companion_groups SET revision=?,data=? WHERE owner=? AND id=?`, g.Revision, b, owner, id)
		return err
	})
}

func openCompanionConversation(ctx context.Context, tx *sql.Tx, owner, id, group string, expected uint64, workspace string) (hub.CompanionConversation, error) {
	var out hub.CompanionConversation
	p, err := companion(ctx, tx, owner, id)
	if err != nil {
		return out, err
	}
	if !p.Enabled || p.Deleted {
		return out, hub.ErrCompanionDisabled
	}
	scope := uint64(0)
	if group != "" {
		g, err := companionGroup(ctx, tx, owner, group)
		if err != nil {
			return out, err
		}
		if g.Revision != expected {
			return out, hub.ErrCompanionConflict
		}
		if !g.Enabled || g.Deleted {
			return out, hub.ErrCompanionDisabled
		}
		found := false
		for _, member := range g.Members {
			found = found || member == id
		}
		if !found {
			return out, hub.ErrNotFound
		}
		scope = g.ScopeRevision
		if workspace != g.Workspace || workspace == "" {
			return out, errors.New("group requires its explicitly selected workspace")
		}
	}
	out, err = read[hub.CompanionConversation](ctx, tx, `SELECT data FROM hub_companion_conversations WHERE owner=? AND companion=? AND group_id=? AND scope_revision=?`, owner, id, group, scope)
	if err == nil {
		out.Profile = p
		return out, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return out, err
	}
	if workspace == "" || !filepath.IsAbs(workspace) || filepath.Clean(workspace) != workspace {
		return out, errors.New("explicit absolute workspace required")
	}
	sid, err := hub.NewID("hub-")
	if err != nil {
		return out, err
	}
	bid, err := hub.NewID("branch-")
	if err != nil {
		return out, err
	}
	cid, err := hub.NewID("conversation-")
	if err != nil {
		return out, err
	}
	title := p.Name
	if group != "" {
		title = "群成员 · " + p.Name
	}
	session := hub.Session{ID: sid, Title: title, Workspace: workspace, ActiveBranchID: bid, Status: engine.Succeeded, CreatedAt: now(), UpdatedAt: now()}
	b, err := body(session)
	if err != nil {
		return out, err
	}
	if _, err = tx.ExecContext(ctx, `INSERT INTO hub_sessions VALUES(?,?,?,?)`, sid, session.Status, session.UpdatedAt, b); err != nil {
		return out, err
	}
	if err = insertBranch(ctx, tx, hub.Branch{ID: bid, SessionID: sid, Target: p.Target}); err != nil {
		return out, err
	}
	if err = saveSettings(ctx, tx, hub.Settings{Version: 1, SessionID: sid, CompanionSkills: &hub.CompanionSkillScope{}, SkillsEnabled: false}); err != nil {
		return out, err
	}
	out = hub.CompanionConversation{ID: cid, OwnerID: owner, CompanionID: id, GroupID: group, GroupRevision: scope, SessionID: sid, Profile: p, CreatedAt: now()}
	b, err = body(out)
	if err != nil {
		return out, err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_companion_conversations VALUES(?,?,?,?,?,?,?)`, cid, owner, id, group, scope, sid, b)
	return out, err
}
func (s *Store) OpenCompanionConversation(ctx context.Context, owner, id, group string, expected uint64, workspace string) (out hub.CompanionConversation, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var e error
		out, e = openCompanionConversation(ctx, tx, owner, id, group, expected, workspace)
		return e
	})
	return out, err
}

func (s *Store) BeginCompanionRound(ctx context.Context, owner, id string, expected uint64, prompt string, members []string) (out hub.CompanionRound, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if strings.TrimSpace(prompt) == "" || len(prompt) > 16<<10 || !utf8.ValidString(prompt) || strings.ContainsRune(prompt, 0) {
		return out, errors.New("group prompt requires 1..16 KiB")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		g, e := companionGroup(ctx, tx, owner, id)
		if e != nil {
			return e
		}
		if g.Revision != expected {
			return hub.ErrCompanionConflict
		}
		if !g.Enabled || g.Deleted {
			return hub.ErrCompanionDisabled
		}
		var active int
		if e = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_companion_rounds WHERE group_id=? AND status IN ('running','waiting','interrupted','unknown')`, id).Scan(&active); e != nil {
			return e
		}
		if active > 0 {
			return hub.ErrSessionBusy
		}
		selected := map[string]bool{}
		for _, member := range members {
			if selected[member] {
				return errors.New("duplicate selected group member")
			}
			selected[member] = true
		}
		if g.ReplyMode == "all" {
			if len(members) > 0 {
				return errors.New("all mode chooses the saved roster; do not supply mentions")
			}
			for _, member := range g.Members {
				selected[member] = true
			}
		}
		if len(selected) == 0 || len(selected) > g.ReplyLimit {
			return errors.New("select 1..reply-limit members")
		}
		out = hub.CompanionRound{OwnerID: owner, GroupID: id, GroupRevision: g.ScopeRevision, Prompt: prompt, Status: "running", Revision: 1, Stage: "ready", CreatedAt: now()}
		for _, member := range g.Members {
			if !selected[member] {
				continue
			}
			c, e := openCompanionConversation(ctx, tx, owner, member, id, expected, g.Workspace)
			if e != nil {
				return e
			}
			out.Members = append(out.Members, c)
			delete(selected, member)
		}
		if len(selected) > 0 {
			return hub.ErrNotFound
		}
		out.ID, e = hub.NewID("round-")
		if e != nil {
			return e
		}
		b, e := body(out)
		if e != nil {
			return e
		}
		if _, e = tx.ExecContext(ctx, `INSERT INTO hub_companion_rounds VALUES(?,?,?,?,?,?,?)`, out.ID, owner, id, g.ScopeRevision, out.Status, out.CreatedAt, b); e != nil {
			return e
		}
		mid, e := hub.NewID("message-")
		if e != nil {
			return e
		}
		m := hub.CompanionMessage{ID: mid, RoundID: out.ID, GroupID: id, GroupRevision: g.ScopeRevision, Author: "你", Text: prompt, Status: "saved", CreatedAt: now()}
		return appendCompanionMessage(ctx, tx, owner, m)
	})
	return out, err
}
func appendCompanionMessage(ctx context.Context, tx *sql.Tx, owner string, m hub.CompanionMessage) error {
	r, err := read[hub.CompanionRound](ctx, tx, `SELECT data FROM hub_companion_rounds WHERE owner=? AND id=?`, owner, m.RoundID)
	if err != nil {
		return err
	}
	if r.GroupID != m.GroupID || r.GroupRevision != m.GroupRevision || r.Status != "running" {
		return hub.ErrCompanionConflict
	}
	if len(m.Text) > 32<<10 || !utf8.ValidString(m.Text) || m.ID == "" || m.Author == "" {
		return errors.New("invalid bounded group message")
	}
	if m.CompanionID != "" {
		found := false
		for _, c := range r.Members {
			if c.CompanionID == m.CompanionID && c.SessionID == m.SessionID {
				found = true
			}
		}
		if !found {
			return hub.ErrNotFound
		}
	}
	b, err := body(m)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_companion_messages(id,owner,group_id,scope_revision,round_id,data) VALUES(?,?,?,?,?,?)`, m.ID, owner, m.GroupID, m.GroupRevision, m.RoundID, b)
	return err
}
func (s *Store) AppendCompanionMessage(ctx context.Context, owner string, m hub.CompanionMessage) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error { return appendCompanionMessage(ctx, tx, owner, m) })
}
func (s *Store) FinishCompanionRound(ctx context.Context, owner, id, status, problem string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	if status != "completed" && status != "failed" && status != "cancelled" && status != "waiting" {
		return errors.New("invalid group round outcome")
	}
	if len(problem) > 4096 {
		problem = problem[:4096]
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		r, err := read[hub.CompanionRound](ctx, tx, `SELECT data FROM hub_companion_rounds WHERE owner=? AND id=?`, owner, id)
		if err != nil {
			return err
		}
		if r.Status != "running" && r.Status != "waiting" && r.Status != "interrupted" && r.Status != "unknown" {
			return hub.ErrCompanionConflict
		}
		r.Status = status
		r.Revision++
		r.Error = problem
		r.EndedAt = now()
		b, err := body(r)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `UPDATE hub_companion_rounds SET status=?,data=? WHERE owner=? AND id=?`, status, b, owner, id)
		return err
	})
}
func (s *Store) CompanionMessages(ctx context.Context, owner, id string, scope uint64, limit int) (out []hub.CompanionMessage, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if limit < 1 || limit > 256 {
		return nil, errors.New("message limit must be 1..256")
	}
	g, err := companionGroup(ctx, s.db, owner, id)
	if err != nil {
		return nil, err
	}
	if scope == 0 {
		scope = g.ScopeRevision
	}
	if scope > g.ScopeRevision {
		return nil, hub.ErrNotFound
	}
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM (SELECT seq,data FROM hub_companion_messages WHERE owner=? AND group_id=? AND scope_revision=? ORDER BY seq DESC LIMIT ?) ORDER BY seq`, owner, id, scope, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var b []byte
		var m hub.CompanionMessage
		if err = rows.Scan(&b); err != nil {
			return nil, err
		}
		if len(b) > 128<<10 || json.Unmarshal(b, &m) != nil {
			return nil, errors.New("group message is corrupt")
		}
		out = append(out, m)
	}
	return out, rows.Err()
}
func (s *Store) CompanionRounds(ctx context.Context, owner, id string, scope uint64, limit int) (out []hub.CompanionRound, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if limit < 1 || limit > 128 {
		return nil, errors.New("round limit must be 1..128")
	}
	g, err := companionGroup(ctx, s.db, owner, id)
	if err != nil {
		return nil, err
	}
	if scope == 0 {
		scope = g.ScopeRevision
	}
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM hub_companion_rounds WHERE owner=? AND group_id=? AND scope_revision=? ORDER BY created DESC,id DESC LIMIT ?`, owner, id, scope, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		var b []byte
		var r hub.CompanionRound
		if err = rows.Scan(&b); err != nil {
			return nil, err
		}
		if len(b) > 128<<10 || json.Unmarshal(b, &r) != nil {
			return nil, errors.New("group round is corrupt")
		}
		out = append(out, r)
	}
	return out, rows.Err()
}
func (s *Store) RecoverCompanions(ctx context.Context) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		rows, e := tx.QueryContext(ctx, `SELECT data FROM hub_companion_rounds WHERE status='running'`)
		if e != nil {
			return e
		}
		var rounds []hub.CompanionRound
		for rows.Next() {
			var b []byte
			var r hub.CompanionRound
			if e = rows.Scan(&b); e != nil {
				rows.Close()
				return e
			}
			if json.Unmarshal(b, &r) != nil {
				rows.Close()
				return errors.New("group round is corrupt")
			}
			rounds = append(rounds, r)
		}
		e = rows.Err()
		rows.Close()
		if e != nil {
			return e
		}
		for _, r := range rounds {
			r.Status = "interrupted"
			r.Revision++
			r.Error = "process restarted; review the saved member sessions before explicitly starting another round"
			r.EndedAt = now()
			b, e := body(r)
			if e != nil {
				return e
			}
			if _, e = tx.ExecContext(ctx, `UPDATE hub_companion_rounds SET status=?,data=? WHERE id=?`, r.Status, b, r.ID); e != nil {
				return fmt.Errorf("recover group round: %w", e)
			}
		}
		return nil
	})
}

func (s *Store) CompanionConversation(ctx context.Context, owner, id, group string, scope uint64) (out hub.CompanionConversation, err error) {
	defer func() { err = normalizeStoreError(err) }()
	p, err := companion(ctx, s.db, owner, id)
	if err != nil {
		return out, err
	}
	if group != "" {
		g, e := companionGroup(ctx, s.db, owner, group)
		if e != nil {
			return out, e
		}
		if scope == 0 {
			scope = g.ScopeRevision
		}
		if scope > g.ScopeRevision {
			return out, hub.ErrNotFound
		}
	} else if scope != 0 {
		return out, hub.ErrNotFound
	}
	out, err = read[hub.CompanionConversation](ctx, s.db, `SELECT data FROM hub_companion_conversations WHERE owner=? AND companion=? AND group_id=? AND scope_revision=?`, owner, id, group, scope)
	if err != nil {
		return out, err
	}
	if out.OwnerID != owner || out.CompanionID != id || out.GroupID != group || out.GroupRevision != scope || out.SessionID == "" {
		return hub.CompanionConversation{}, errors.New("companion conversation identity is corrupt")
	}
	if group == "" {
		out.Profile = p
	}
	return out, nil
}

func (s *Store) CompanionSession(ctx context.Context, session string) (out hub.CompanionConversation, err error) {
	defer func() { err = normalizeStoreError(err) }()
	out, err = read[hub.CompanionConversation](ctx, s.db, `SELECT data FROM hub_companion_conversations WHERE session=?`, session)
	if err != nil {
		return out, err
	}
	if out.SessionID != session || out.ID == "" || out.OwnerID == "" || out.CompanionID == "" || out.Profile.Validate() != nil {
		return hub.CompanionConversation{}, errors.New("companion session binding is corrupt")
	}
	return out, nil
}

// companionExecutionAllowed runs inside every Attempt claim, including fallback
// and wait continuation. Historical bindings survive; deleted identities do not
// regain execution through an ordinary session or a direct Runtime caller.
func companionExecutionAllowed(ctx context.Context, tx *sql.Tx, session string) error {
	c, err := read[hub.CompanionConversation](ctx, tx, `SELECT data FROM hub_companion_conversations WHERE session=?`, session)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if c.SessionID != session || c.OwnerID == "" || c.CompanionID == "" {
		return errors.New("companion session binding is corrupt")
	}
	p, err := companion(ctx, tx, c.OwnerID, c.CompanionID)
	if err != nil {
		return err
	}
	if p.Deleted {
		return hub.ErrCompanionDisabled
	}
	if c.GroupID != "" {
		g, err := companionGroup(ctx, tx, c.OwnerID, c.GroupID)
		if err != nil {
			return err
		}
		if g.Deleted {
			return hub.ErrCompanionDisabled
		}
	}
	return nil
}

func (s *Store) CompanionArchive(ctx context.Context, owner string) (out hub.CompanionArchive, err error) {
	out.Profiles, err = s.companionProfiles(ctx, owner, true)
	if err == nil {
		out.Groups, err = s.companionGroups(ctx, owner, true)
	}
	return out, err
}

type companionHistoryCursor struct {
	Kind     string `json:"kind"`
	Owner    string `json:"owner"`
	Group    string `json:"group"`
	Scope    uint64 `json:"scope"`
	Sequence uint64 `json:"sequence,omitempty"`
	Created  int64  `json:"created,omitempty"`
	ID       string `json:"id,omitempty"`
}

func (s *Store) companionHistoryQuery(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery, kind string, maximum int) (companionHistoryCursor, error) {
	c := companionHistoryCursor{Kind: kind, Owner: owner, Group: id, Scope: scope}
	if query.Limit < 1 || query.Limit > maximum || len(query.Before) > 2048 {
		return c, errors.New("invalid companion history page")
	}
	g, err := companionGroup(ctx, s.db, owner, id)
	if err != nil {
		return c, err
	}
	if scope == 0 {
		scope = g.ScopeRevision
		c.Scope = scope
	}
	if scope > g.ScopeRevision {
		return c, hub.ErrNotFound
	}
	if query.Before != "" {
		b, err := base64.RawURLEncoding.DecodeString(query.Before)
		var before companionHistoryCursor
		if err != nil || json.Unmarshal(b, &before) != nil || before.Kind != kind || before.Owner != owner || before.Group != id || before.Scope != scope || (kind == "message" && before.Sequence == 0) || (kind == "round" && (before.Created <= 0 || before.ID == "")) {
			return c, errors.New("invalid or foreign companion history cursor")
		}
		c = before
	}
	return c, nil
}
func encodeCompanionHistoryCursor(c companionHistoryCursor) string {
	b, _ := json.Marshal(c)
	return base64.RawURLEncoding.EncodeToString(b)
}
func (s *Store) CompanionMessagePage(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery) (out hub.CompanionMessagePage, err error) {
	defer func() { err = normalizeStoreError(err) }()
	c, err := s.companionHistoryQuery(ctx, owner, id, scope, query, "message", 256)
	if err != nil {
		return out, err
	}
	rows, err := s.db.QueryContext(ctx, `SELECT seq,data FROM hub_companion_messages WHERE owner=? AND group_id=? AND scope_revision=? AND (?=0 OR seq<?) ORDER BY seq DESC LIMIT ?`, owner, id, c.Scope, c.Sequence, c.Sequence, query.Limit+1)
	if err != nil {
		return out, err
	}
	defer rows.Close()
	for rows.Next() {
		var seq uint64
		var b []byte
		var m hub.CompanionMessage
		if err = rows.Scan(&seq, &b); err != nil {
			return out, err
		}
		if len(b) > 128<<10 || json.Unmarshal(b, &m) != nil || m.GroupID != id || m.GroupRevision != c.Scope || m.ID == "" {
			return out, errors.New("group message is corrupt")
		}
		if len(out.Messages) == query.Limit {
			out.HasMore = true
			break
		}
		out.Messages = append(out.Messages, m)
		c.Sequence = seq
	}
	if err = rows.Err(); err != nil {
		return out, err
	}
	if out.HasMore {
		out.NextBefore = encodeCompanionHistoryCursor(c)
	}
	slices.Reverse(out.Messages)
	return out, nil
}
func (s *Store) CompanionRoundPage(ctx context.Context, owner, id string, scope uint64, query hub.CompanionHistoryQuery) (out hub.CompanionRoundPage, err error) {
	defer func() { err = normalizeStoreError(err) }()
	c, err := s.companionHistoryQuery(ctx, owner, id, scope, query, "round", 128)
	if err != nil {
		return out, err
	}
	rows, err := s.db.QueryContext(ctx, `SELECT created,id,data FROM hub_companion_rounds WHERE owner=? AND group_id=? AND scope_revision=? AND (?=0 OR created<? OR (created=? AND id<?)) ORDER BY created DESC,id DESC LIMIT ?`, owner, id, c.Scope, c.Created, c.Created, c.Created, c.ID, query.Limit+1)
	if err != nil {
		return out, err
	}
	defer rows.Close()
	for rows.Next() {
		var created int64
		var key string
		var b []byte
		var r hub.CompanionRound
		if err = rows.Scan(&created, &key, &b); err != nil {
			return out, err
		}
		if len(b) > 128<<10 || json.Unmarshal(b, &r) != nil || r.ID != key || r.CreatedAt != created || r.OwnerID != owner || r.GroupID != id || r.GroupRevision != c.Scope {
			return out, errors.New("group round is corrupt")
		}
		if len(out.Rounds) == query.Limit {
			out.HasMore = true
			break
		}
		out.Rounds = append(out.Rounds, r)
		c.Created, c.ID = created, key
	}
	if err = rows.Err(); err != nil {
		return out, err
	}
	if out.HasMore {
		out.NextBefore = encodeCompanionHistoryCursor(c)
	}
	return out, nil
}
