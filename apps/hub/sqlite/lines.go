// INPUT: context, database/sql, encoding/json, hub Line/LineFork/SessionFork, branch and Turn rows
// OUTPUT: hub_lines schema and the hub_branches line column (v11), line creation inside Turn admission, version switching, session forks, segment visibility predicates and segmented history reads
// POS: [L7 Hub product storage] Version lines own engine branches and a list of visible event/Turn ranges; sessions without lines read exactly as before
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// versionLinesSchema creates hub_lines and rebuilds hub_branches with its
// line column and (session,line,route) uniqueness: create the new table, copy
// every row, drop the old table, rename. Existing rows keep line ”.
func versionLinesSchema(ctx context.Context, tx *sql.Tx) error {
	for _, statement := range []string{
		`CREATE TABLE IF NOT EXISTS hub_lines(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),data BLOB NOT NULL)`,
		`CREATE INDEX IF NOT EXISTS hub_lines_session ON hub_lines(session)`,
		`CREATE TABLE hub_branches_v11(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),route TEXT NOT NULL,data BLOB NOT NULL,line TEXT NOT NULL DEFAULT '',UNIQUE(session,line,route))`,
		`INSERT INTO hub_branches_v11(id,session,route,data) SELECT id,session,route,data FROM hub_branches ORDER BY rowid`,
		`DROP TABLE hub_branches`,
		`ALTER TABLE hub_branches_v11 RENAME TO hub_branches`,
		`CREATE INDEX IF NOT EXISTS hub_native_route ON hub_branches(json_extract(data,'$.target.engine_id'),json_extract(data,'$.native_session_id'))`,
	} {
		if _, err := tx.ExecContext(ctx, statement); err != nil {
			return err
		}
	}
	return nil
}

func readLines(ctx context.Context, tx *sql.Tx, session string) ([]hub.Line, error) {
	rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_lines WHERE session=? ORDER BY rowid LIMIT 66`, session)
	if err != nil {
		return nil, err
	}
	return scanLines(rows)
}

func scanLines(rows *sql.Rows) ([]hub.Line, error) {
	defer rows.Close()
	var lines []hub.Line
	for rows.Next() {
		var data []byte
		var line hub.Line
		if err := rows.Scan(&data); err != nil {
			return nil, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &line) != nil {
			return nil, errors.New("version line is corrupt")
		}
		lines = append(lines, line)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if len(lines) > hub.MaxLinesPerSession {
		return nil, errors.New("version line budget exceeded")
	}
	return lines, nil
}

func insertLine(ctx context.Context, tx *sql.Tx, line hub.Line) error {
	data, err := body(line)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_lines(id,session,data) VALUES(?,?,?)`, line.ID, line.SessionID, data)
	return err
}

func writeLine(ctx context.Context, tx *sql.Tx, line hub.Line) error {
	data, err := body(line)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_lines SET data=? WHERE id=? AND session=?`, data, line.ID, line.SessionID)
	return err
}

// Lines lists a session's version lines.
func (s *Store) Lines(ctx context.Context, id string) (lines []hub.Line, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if _, err = read[hub.Session](ctx, s.list, liveSessionQuery, id); err != nil {
		return nil, err
	}
	rows, err := s.list.QueryContext(ctx, `SELECT data FROM hub_lines WHERE session=? ORDER BY rowid LIMIT 66`, id)
	if err != nil {
		return nil, err
	}
	return scanLines(rows)
}

// activeLineOf reads the session's current line, or false when it has none.
func activeLineOf(ctx context.Context, q querier, session hub.Session) (hub.Line, bool, error) {
	if session.ActiveLineID == "" {
		return hub.Line{}, false, nil
	}
	line, err := read[hub.Line](ctx, q, `SELECT data FROM hub_lines WHERE id=? AND session=?`, session.ActiveLineID, session.ID)
	if err != nil {
		return hub.Line{}, false, err
	}
	return line, true, nil
}

// seqPredicate renders the visibility of a line as an SQL condition over
// column col. turn picks Turn ordinals instead of event sequences. An empty
// predicate (no line) means everything is visible; a line with nothing
// visible renders a false condition.
func seqPredicate(col string, line hub.Line, turn bool) (string, []any) {
	var parts []string
	var args []any
	for _, seg := range line.Segments {
		from, to := seg.FromSeq, seg.ToSeq
		if turn {
			from, to = seg.FromTurn, seg.ToTurn
		}
		switch {
		case !seg.Closed:
			parts = append(parts, col+`>=?`)
			args = append(args, from)
		case to >= from:
			parts = append(parts, col+` BETWEEN ? AND ?`)
			args = append(args, from, to)
		}
	}
	if len(parts) == 0 {
		return "0", nil
	}
	return "(" + strings.Join(parts, " OR ") + ")", args
}

// versionPredicate is seqPredicate for the session's active line: "" when it
// has no versions, so callers keep their original query.
func versionPredicate(ctx context.Context, q querier, session hub.Session, col string, turn bool) (string, []any, error) {
	line, ok, err := activeLineOf(ctx, q, session)
	if err != nil || !ok {
		return "", nil, err
	}
	predicate, args := seqPredicate(col, line, turn)
	return predicate, args, nil
}

// lineFilter restricts a hub_branches query to the session's current version
// line. A session without versions needs no restriction (and so no line
// column), which keeps its queries exactly as they always were.
func lineFilter(session hub.Session) string {
	if session.ActiveLineID == "" {
		return ""
	}
	return ` AND line=?`
}

func lineArgs(session hub.Session, id string) []any {
	if session.ActiveLineID == "" {
		return []any{id}
	}
	return []any{id, session.ActiveLineID}
}

// segmentRanges lists a line's event ranges, ascending.
func segmentRanges(line hub.Line) [][2]uint64 {
	var ranges [][2]uint64
	for _, seg := range line.Segments {
		if !seg.Closed {
			ranges = append(ranges, [2]uint64{seg.FromSeq, math.MaxInt64})
		} else if seg.ToSeq >= seg.FromSeq {
			ranges = append(ranges, [2]uint64{seg.FromSeq, seg.ToSeq})
		}
	}
	return ranges
}

// historySegments reads history entries inside a line's ranges. Each range is
// its own indexed (session,seq) query, so the read is bounded by the page and
// never sorts a union. Forward returns ascending entries after `after`;
// backward returns the newest entries below `before` (0 = end), ascending.
func (s *Store) historySegments(ctx context.Context, id string, line hub.Line, after, before uint64, limit int, backward bool) ([]hub.HistoryEntry, error) {
	ranges := segmentRanges(line)
	var result []hub.HistoryEntry
	run := func(lo, hi uint64, remaining int, desc bool) error {
		order := "ASC"
		if desc {
			order = "DESC"
		}
		rows, err := s.db.QueryContext(ctx, `SELECT e.seq,e.data,COALESCE(t.seq,0) FROM hub_events e LEFT JOIN hub_turns t ON t.id=e.turn WHERE e.session=? AND e.seq>=? AND e.seq<=? ORDER BY e.seq `+order+` LIMIT ?`, id, lo, hi, remaining)
		if err != nil {
			return err
		}
		defer rows.Close()
		page, err := historyRows(rows)
		if err != nil {
			return err
		}
		result = append(result, page...)
		return nil
	}
	if !backward {
		for _, r := range ranges {
			lo, hi := r[0], r[1]
			if after+1 > lo {
				lo = after + 1
			}
			if lo > hi || len(result) >= limit {
				continue
			}
			if err := run(lo, hi, limit-len(result), false); err != nil {
				return nil, err
			}
		}
		return result, nil
	}
	for i := len(ranges) - 1; i >= 0; i-- {
		lo, hi := ranges[i][0], ranges[i][1]
		if before != 0 && before-1 < hi {
			hi = before - 1
		}
		if lo > hi || len(result) >= limit {
			continue
		}
		if err := run(lo, hi, limit-len(result), true); err != nil {
			return nil, err
		}
	}
	for i, j := 0, len(result)-1; i < j; i, j = i+1, j-1 {
		result[i], result[j] = result[j], result[i]
	}
	return result, nil
}

// versionSession reports why a session cannot have versions or forks: Team
// leads and Workers, companion conversations and sessions with automations
// stay unversioned in v1.
func versionSession(ctx context.Context, q interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}, id string) (string, error) {
	var count int
	if err := q.QueryRowContext(ctx, `SELECT (SELECT count(*) FROM hub_workers WHERE session=?)+(SELECT count(*) FROM hub_teams WHERE lead=?)+(SELECT count(*) FROM hub_companion_conversations WHERE session=?)+(SELECT count(*) FROM hub_automations WHERE session=?)`, id, id, id, id).Scan(&count); err != nil {
		return "", err
	}
	if count != 0 {
		return "团队、伙伴和自动化会话暂不支持版本与分叉", nil
	}
	return "", nil
}

// versionableTurn decides whether a Turn of the session's current line can be
// edited, regenerated or forked, and why not.
func versionableTurn(ctx context.Context, tx querierRows, session hub.Session, turnID string) (hub.Turn, string, error) {
	turn, err := read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE id=? AND session=?`, turnID, session.ID)
	if errors.Is(err, sql.ErrNoRows) {
		return turn, "", hub.ErrNotFound
	}
	if err != nil {
		return turn, "", err
	}
	predicate, args, err := versionPredicate(ctx, tx, session, "seq", true)
	if err != nil {
		return turn, "", err
	}
	if predicate != "" {
		var visible int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_turns WHERE id=? AND `+predicate, append([]any{turnID}, args...)...).Scan(&visible); err != nil {
			return turn, "", err
		}
		if visible == 0 {
			return turn, "", hub.ErrNotFound
		}
	}
	if reason, err := versionSession(ctx, tx, session.ID); err != nil || reason != "" {
		return turn, reason, err
	}
	switch {
	case turn.Origin != "":
		return turn, "“继续生成”不能编辑或重新生成", nil
	case turn.AutomationID != "":
		return turn, "自动化触发的轮次不支持版本", nil
	case len(turn.InputFiles) > 0 || len(turn.InputSkills) > 0:
		return turn, "带附件或技能的轮次不支持编辑、重新生成和分叉后重发", nil
	case turn.Status == hub.Running || turn.Status == engine.Waiting:
		return turn, "这一轮还没有结束", nil
	}
	return turn, "", nil
}

// querierRows is the read surface shared by transactions and connections.
type querierRows interface {
	QueryContext(context.Context, string, ...any) (*sql.Rows, error)
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

// VersionsDisabled says why the conversation as a whole cannot have versions.
func (s *Store) VersionsDisabled(ctx context.Context, id string) (reason string, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if _, err = read[hub.Session](ctx, s.list, liveSessionQuery, id); err != nil {
		return "", err
	}
	return versionSession(ctx, s.list, id)
}

// TurnVersionable reports a Turn's version eligibility without changing it.
func (s *Store) TurnVersionable(ctx context.Context, id, turnID string) (result hub.VersionTurn, err error) {
	defer func() { err = normalizeStoreError(err) }()
	session, err := read[hub.Session](ctx, s.list, liveSessionQuery, id)
	if err != nil {
		return result, err
	}
	result.Turn, result.Reason, err = versionableTurn(ctx, s.list, session, turnID)
	if err != nil {
		return result, err
	}
	var entry []byte
	if err = s.list.QueryRowContext(ctx, `SELECT data FROM hub_events WHERE turn=? ORDER BY seq LIMIT 1`, turnID).Scan(&entry); err != nil {
		return result, err
	}
	var first hub.HistoryEntry
	if len(entry) <= 128<<10 && json.Unmarshal(entry, &first) == nil {
		result.UserText = first.Input
	}
	return result, nil
}

type pendingLine struct {
	line  hub.Line
	group string
	// degraded: the engine could not fork natively although the source branch
	// has a native conversation, so the new version starts from the Turn index.
	degraded bool
}

// forkLine runs inside Turn admission. It makes the session's current line
// explicit if it never had one, cuts a new line before the Turn, closes the
// parent's open segment, creates the new line's branch on the active engine
// and switches the session to it. finishLine adds the new Turn once admitted.
func forkLine(ctx context.Context, tx *sql.Tx, session *hub.Session, fork hub.LineFork) (*pendingLine, error) {
	turn, reason, err := versionableTurn(ctx, tx, *session, fork.TurnID)
	if err != nil {
		return nil, err
	}
	if reason != "" {
		return nil, fmt.Errorf("%w: %s", hub.ErrVersionUnsupported, reason)
	}
	lines, err := readLines(ctx, tx, session.ID)
	if err != nil {
		return nil, err
	}
	if len(lines) >= hub.MaxLinesPerSession {
		return nil, hub.ErrVersionBudget
	}
	var maxEvent, maxTurn, firstEvent uint64
	if err = tx.QueryRowContext(ctx, `SELECT coalesce(max(seq),0) FROM hub_events WHERE session=?`, session.ID).Scan(&maxEvent); err != nil {
		return nil, err
	}
	if err = tx.QueryRowContext(ctx, `SELECT coalesce(max(seq),0) FROM hub_turns WHERE session=?`, session.ID).Scan(&maxTurn); err != nil {
		return nil, err
	}
	if err = tx.QueryRowContext(ctx, `SELECT coalesce(min(seq),0) FROM hub_events WHERE turn=?`, turn.ID).Scan(&firstEvent); err != nil {
		return nil, err
	}
	if firstEvent == 0 {
		return nil, errors.New("the Turn has no history to cut before")
	}
	var parent hub.Line
	if session.ActiveLineID == "" {
		id, idErr := hub.NewID("line-")
		if idErr != nil {
			return nil, idErr
		}
		parent = hub.Line{ID: id, SessionID: session.ID, Ordinal: 1, CreatedAt: now(), TouchedAt: now(), Segments: []hub.LineSegment{{FromSeq: 1, FromTurn: 1}}, ActiveBranchID: session.ActiveBranchID}
		if err = insertLine(ctx, tx, parent); err != nil {
			return nil, err
		}
		if _, err = tx.ExecContext(ctx, `UPDATE hub_branches SET line=? WHERE session=? AND line=''`, parent.ID, session.ID); err != nil {
			return nil, err
		}
		session.ActiveLineID = parent.ID
		lines = append(lines, parent)
		if len(lines) >= hub.MaxLinesPerSession {
			return nil, hub.ErrVersionBudget
		}
	} else if parent, _, err = activeLineOf(ctx, tx, *session); err != nil {
		return nil, err
	}
	if fork.Regenerate {
		predicate, args, e := versionPredicate(ctx, tx, *session, "seq", true)
		if e != nil {
			return nil, e
		}
		query := `SELECT coalesce(max(seq),0) FROM hub_turns WHERE session=?`
		queryArgs := []any{session.ID}
		if predicate != "" {
			query += ` AND ` + predicate
			queryArgs = append(queryArgs, args...)
		}
		var last uint64
		if e = tx.QueryRowContext(ctx, query, queryArgs...).Scan(&last); e != nil {
			return nil, e
		}
		if last != turn.Sequence {
			return nil, fmt.Errorf("%w: only the last reply can be regenerated", hub.ErrVersionUnsupported)
		}
	}
	oldBranch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, session.ID)
	if err != nil {
		return nil, err
	}
	predicate, predicateArgs := seqPredicate("t.seq", parent, true)
	// The prompts the source branch's native conversation holds from this Turn
	// on, and its last own Turn before it: they decide the native cut and which
	// earlier Turns a handoff must still index.
	var lastOwn uint64
	if oldBranch.NativeSessionID != "" {
		before := `SELECT coalesce(max(t.seq),0) FROM hub_turns t JOIN hub_attempts a ON a.id=json_extract(t.data,'$.active_attempt_id') WHERE t.session=? AND t.seq<? AND json_extract(a.data,'$.branch_id')=? AND ` + predicate
		if err = tx.QueryRowContext(ctx, before, append([]any{session.ID, turn.Sequence, oldBranch.ID}, predicateArgs...)...).Scan(&lastOwn); err != nil {
			return nil, err
		}
	}
	// Cut the parent's segments before the Turn, then open the new line.
	var segments []hub.LineSegment
	for _, seg := range parent.Segments {
		if seg.FromTurn >= turn.Sequence || seg.FromSeq >= firstEvent {
			continue
		}
		if !seg.Closed || seg.ToTurn >= turn.Sequence || seg.ToSeq >= firstEvent {
			seg.Closed, seg.ToTurn, seg.ToSeq = true, turn.Sequence-1, firstEvent-1
		}
		segments = append(segments, seg)
	}
	segments = append(segments, hub.LineSegment{FromSeq: maxEvent + 1, FromTurn: maxTurn + 1})
	if len(segments) > hub.MaxSegmentsPerLine {
		return nil, hub.ErrVersionBudget
	}
	// Close the parent where it stands now and remember its route.
	closeLine(&parent, maxEvent, maxTurn)
	parent.ActiveBranchID = session.ActiveBranchID
	group := ""
	for _, g := range parent.Groups {
		if g.TurnID == turn.ID {
			group = g.Group
		}
	}
	if group == "" {
		if group, err = hub.NewID("group-"); err != nil {
			return nil, err
		}
		parent.Groups = append(parent.Groups, hub.LineGroup{Group: group, TurnID: turn.ID, Seq: turn.Sequence})
	}
	if err = writeLine(ctx, tx, parent); err != nil {
		return nil, err
	}
	lineID, err := hub.NewID("line-")
	if err != nil {
		return nil, err
	}
	next := hub.Line{ID: lineID, SessionID: session.ID, ParentID: parent.ID, Ordinal: len(lines) + 1, CreatedAt: now(), TouchedAt: now(), Segments: segments}
	for _, g := range parent.Groups {
		if g.Seq < turn.Sequence {
			next.Groups = append(next.Groups, g)
		}
	}
	branchID, err := hub.NewID("branch-")
	if err != nil {
		return nil, err
	}
	branch := hub.Branch{ID: branchID, SessionID: session.ID, Target: oldBranch.Target, Watermark: lastOwn}
	anchor, anchored, err := forkAnchor(ctx, tx, session.ID, oldBranch, turn, false)
	if err != nil {
		return nil, err
	}
	if fork.NativeFork && anchored {
		branch.Fork = &anchor
	} else {
		branch.Watermark = 0
	}
	next.ActiveBranchID = branch.ID
	if err = insertLine(ctx, tx, next); err != nil {
		return nil, err
	}
	if err = insertLineBranch(ctx, tx, branch, next.ID); err != nil {
		return nil, err
	}
	oldBranch.Parked = true
	if err = writeBranch(ctx, tx, oldBranch); err != nil {
		return nil, err
	}
	session.ActiveLineID, session.ActiveBranchID = next.ID, branch.ID
	if err = writeSession(ctx, tx, *session); err != nil {
		return nil, err
	}
	return &pendingLine{line: next, group: group, degraded: branch.Fork == nil && oldBranch.NativeSessionID != ""}, nil
}

// closeLine closes the open segment where the session's history stands now.
func closeLine(line *hub.Line, maxEvent, maxTurn uint64) {
	var kept []hub.LineSegment
	for _, seg := range line.Segments {
		if !seg.Closed {
			seg.Closed, seg.ToSeq, seg.ToTurn = true, maxEvent, maxTurn
		}
		if seg.ToSeq < seg.FromSeq && seg.ToTurn < seg.FromTurn {
			continue
		}
		kept = append(kept, seg)
	}
	line.Segments = kept
	line.TouchedAt = now()
}

// finishLine places the admitted Turn at the fork position of the new line.
func finishLine(ctx context.Context, tx *sql.Tx, pending *pendingLine, turn hub.Turn) error {
	if pending == nil {
		return nil
	}
	pending.line.Groups = append(pending.line.Groups, hub.LineGroup{Group: pending.group, TurnID: turn.ID, Seq: turn.Sequence})
	return writeLine(ctx, tx, pending.line)
}

// SelectLine switches an idle session to another version line.
func (s *Store) SelectLine(ctx context.Context, id, lineID string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, e := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if e != nil {
			return e
		}
		if hub.BlocksInput(session.Status) {
			return hub.ErrVersionBusy
		}
		if session.ActiveLineID == "" || session.ActiveLineID == lineID {
			if session.ActiveLineID == lineID {
				return nil
			}
			return hub.ErrNotFound
		}
		target, e := read[hub.Line](ctx, tx, `SELECT data FROM hub_lines WHERE id=? AND session=?`, lineID, id)
		if errors.Is(e, sql.ErrNoRows) {
			return hub.ErrNotFound
		}
		if e != nil {
			return e
		}
		current, _, e := activeLineOf(ctx, tx, session)
		if e != nil {
			return e
		}
		var maxEvent, maxTurn uint64
		if e = tx.QueryRowContext(ctx, `SELECT coalesce(max(seq),0) FROM hub_events WHERE session=?`, id).Scan(&maxEvent); e != nil {
			return e
		}
		if e = tx.QueryRowContext(ctx, `SELECT coalesce(max(seq),0) FROM hub_turns WHERE session=?`, id).Scan(&maxTurn); e != nil {
			return e
		}
		closeLine(&current, maxEvent, maxTurn)
		current.ActiveBranchID = session.ActiveBranchID
		if e = writeLine(ctx, tx, current); e != nil {
			return e
		}
		if len(target.Segments) >= hub.MaxSegmentsPerLine {
			return hub.ErrVersionBudget
		}
		target.Segments = append(target.Segments, hub.LineSegment{FromSeq: maxEvent + 1, FromTurn: maxTurn + 1})
		target.TouchedAt = now()
		branchID := target.ActiveBranchID
		var count int
		if e = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_branches WHERE id=? AND session=? AND line=?`, branchID, id, lineID).Scan(&count); e != nil {
			return e
		}
		if count == 0 {
			if e = tx.QueryRowContext(ctx, `SELECT id FROM hub_branches WHERE session=? AND line=? ORDER BY rowid LIMIT 1`, id, lineID).Scan(&branchID); e != nil {
				return e
			}
			target.ActiveBranchID = branchID
		}
		if e = writeLine(ctx, tx, target); e != nil {
			return e
		}
		old, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, id)
		if e != nil {
			return e
		}
		old.Parked = true
		if e = writeBranch(ctx, tx, old); e != nil {
			return e
		}
		branch, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, branchID, id)
		if e != nil {
			return e
		}
		branch.Parked = false
		if e = writeBranch(ctx, tx, branch); e != nil {
			return e
		}
		session.ActiveLineID, session.ActiveBranchID = lineID, branchID
		// The session shows the status of what the chosen version ended with.
		predicate, args := seqPredicate("seq", target, true)
		latest, e := read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE session=? AND `+predicate+` ORDER BY seq DESC LIMIT 1`, append([]any{id}, args...)...)
		switch {
		case e == nil:
			session.Status = latest.Status
		case errors.Is(e, sql.ErrNoRows):
			session.Status = engine.Succeeded
		default:
			return e
		}
		return writeSession(ctx, tx, session)
	})
}

// ForkSession creates a new session that continues from the end of a Turn of
// id. Nothing is copied: the new branch holds the intent to cut a native copy
// (or, for an engine that cannot, the bounded Turn index) and the session
// records where it came from.
func (s *Store) ForkSession(ctx context.Context, id string, input hub.SessionFork) (result hub.Session, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		source, e := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if e != nil {
			return e
		}
		if hub.BlocksInput(source.Status) {
			return hub.ErrSessionRunning
		}
		var managed int
		if e = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_sessions WHERE id=? AND `+ordinarySessionFilter, id).Scan(&managed); e != nil {
			return e
		}
		if managed != 1 {
			return hub.ErrNotFound
		}
		turn, e := read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE id=? AND session=?`, input.TurnID, id)
		if errors.Is(e, sql.ErrNoRows) {
			return hub.ErrNotFound
		}
		if e != nil {
			return e
		}
		if reason, e := versionSession(ctx, tx, id); e != nil || reason != "" {
			if e != nil {
				return e
			}
			return fmt.Errorf("%w: %s", hub.ErrVersionUnsupported, reason)
		}
		if turn.Status == hub.Running || turn.Status == engine.Waiting {
			return fmt.Errorf("%w: 这一轮还没有结束", hub.ErrVersionUnsupported)
		}
		predicate, args, e := versionPredicate(ctx, tx, source, "t.seq", true)
		if e != nil {
			return e
		}
		visible := "1"
		if predicate != "" {
			visible = predicate
		}
		var inLine int
		if e = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_turns t WHERE t.id=? AND `+visible, append([]any{turn.ID}, args...)...).Scan(&inLine); e != nil {
			return e
		}
		if inLine == 0 {
			return hub.ErrNotFound
		}
		oldBranch, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, source.ActiveBranchID, id)
		if e != nil {
			return e
		}
		var lastEvent uint64
		if e = tx.QueryRowContext(ctx, `SELECT coalesce(max(seq),0) FROM hub_events WHERE turn=?`, turn.ID).Scan(&lastEvent); e != nil {
			return e
		}
		title := "分叉 · " + source.Title
		for len(title) > 256 || !utf8.ValidString(title) {
			title = title[:len(title)-1]
		}
		created, e := createSessionTx(ctx, tx, title, source.Workspace, oldBranch.Target)
		if e != nil {
			return e
		}
		branch, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, created.ActiveBranchID, created.ID)
		if e != nil {
			return e
		}
		anchor, anchored, e := forkAnchor(ctx, tx, id, oldBranch, turn, true)
		if e != nil {
			return e
		}
		if input.NativeFork && anchored {
			branch.Fork = &anchor
		} else {
			if branch.Index, e = forkIndexEntries(ctx, tx, source, turn.Sequence); e != nil {
				return e
			}
		}
		if e = writeBranch(ctx, tx, branch); e != nil {
			return e
		}
		created.ForkedFrom = &hub.ForkOrigin{SessionID: id, Title: source.Title, TurnID: turn.ID, TurnSequence: turn.Sequence, EventSequence: lastEvent}
		if e = writeSession(ctx, tx, created); e != nil {
			return e
		}
		result = created
		return nil
	})
	return result, err
}

// indexHandoff is the first Turn's handoff of a forked conversation whose
// engine cannot fork natively: the bounded index of the source Turns.
func indexHandoff(session hub.Session, branch hub.Branch) (*hub.Handoff, error) {
	if len(branch.Index) == 0 || branch.NativeSessionID != "" || session.ForkedFrom == nil {
		return nil, nil
	}
	id, err := hub.NewID("handoff-")
	if err != nil {
		return nil, err
	}
	value := &hub.Handoff{ID: id, SessionID: session.ID, TargetBranchID: branch.ID, Workspace: session.Workspace, Through: session.ForkedFrom.TurnSequence, Entries: append([]hub.HandoffEntry(nil), branch.Index...), Processing: "This conversation was forked from another one. Bounded index of the earlier Turns only: prompt excerpts, no replies, files or native transcripts."}
	for {
		if _, err = hub.EncodeHandoff(value); err == nil {
			break
		}
		if len(value.Entries) == 0 {
			return nil, err
		}
		value.Entries = value.Entries[1:]
	}
	return protectHandoff(value)
}

// forkIndexEntries is the bounded Turn index of a source conversation's visible
// Turns up to and including throughSeq (at most 32, oldest first).
func forkIndexEntries(ctx context.Context, q querierRows, source hub.Session, throughSeq uint64) ([]hub.HandoffEntry, error) {
	predicate, args, err := versionPredicate(ctx, q, source, "t.seq", true)
	if err != nil {
		return nil, err
	}
	visible := "1"
	if predicate != "" {
		visible = predicate
	}
	rows, err := q.QueryContext(ctx, `SELECT t.data FROM hub_turns t WHERE t.session=? AND t.seq<=? AND `+visible+` ORDER BY t.seq DESC LIMIT 32`, append([]any{source.ID, throughSeq}, args...)...)
	if err != nil {
		return nil, err
	}
	var entries []hub.HandoffEntry
	for rows.Next() {
		var data []byte
		var kept hub.Turn
		if err = rows.Scan(&data); err != nil {
			break
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &kept) != nil {
			err = errors.New("fork source Turn is corrupt")
			break
		}
		prompt := kept.Prompt
		if len(prompt) > 256 {
			prompt = prompt[:256]
			for !utf8.ValidString(prompt) {
				prompt = prompt[:len(prompt)-1]
			}
			prompt += " [excerpt]"
		}
		entries = append(entries, hub.HandoffEntry{TurnID: kept.ID, Prompt: prompt, Status: kept.Status})
	}
	if err = errors.Join(err, rows.Err(), rows.Close()); err != nil {
		return nil, err
	}
	for l, r := 0, len(entries)-1; l < r; l, r = l+1, r-1 {
		entries[l], entries[r] = entries[r], entries[l]
	}
	return entries, nil
}

// forkAnchor fixes the native cut of a fork as an absolute anchor from
// identities the source already holds: the engine's own turn identity the
// attempt reported, and native record sequences the Turn's events reference.
// Neither moves when the source later grows or is rolled back and extended. It
// reports false when the Turn did not run on this branch's own native
// conversation (inherited through an earlier fork, never ran) or left no
// identity behind, so no reliable anchor exists and the caller must not fork
// natively. Only primary-key and (turn,seq) index lookups: no Turn scan, so it
// is cheap inside the admission transaction.
//
// keepThrough selects a fork that keeps turn (a new conversation) instead of
// dropping it (edit, regenerate).
func forkAnchor(ctx context.Context, q querierRows, sessionID string, branch hub.Branch, turn hub.Turn, keepThrough bool) (hub.BranchFork, bool, error) {
	none := hub.BranchFork{}
	native := branch.NativeSessionID
	if native == "" {
		return none, false, nil
	}
	attempt, err := read[hub.Attempt](ctx, q, `SELECT data FROM hub_attempts WHERE id=?`, turn.ActiveAttemptID)
	if errors.Is(err, sql.ErrNoRows) || err == nil && attempt.BranchID != branch.ID {
		return none, false, nil
	}
	if err != nil {
		return none, false, err
	}
	// The Turn may have run more than once on this conversation (a failed
	// attempt retried): the cut must lie before the earliest prompt it put in the
	// native thread, so take the earliest native turn identity any attempt of
	// the Turn reported on it.
	var anchorTurn sql.NullString
	if err = q.QueryRowContext(ctx, `SELECT json_extract(data,'$.event.native_turn') FROM hub_events WHERE turn=? AND json_extract(data,'$.event.native_turn') IS NOT NULL AND json_extract(data,'$.event.session_id')=? ORDER BY seq LIMIT 1`, turn.ID, native).Scan(&anchorTurn); err != nil && !errors.Is(err, sql.ErrNoRows) {
		return none, false, err
	}
	result := hub.BranchFork{Owner: sessionID, Session: native, AnchorTurn: anchorTurn.String, AnchorKeep: keepThrough}
	const refs = `json_extract(data,'$.event.reference.session_id')=? AND turn=?`
	var seq uint64
	if keepThrough {
		// Keep every message up to the last record the kept Turn referenced.
		// An interjection is committed before the model request that answers
		// it, so it precedes that Turn's final reply and is inside the cut; one
		// that was never delivered is not in the native log at all.
		err = q.QueryRowContext(ctx, `SELECT coalesce(max(json_extract(data,'$.event.reference.sequence')),0) FROM hub_events WHERE `+refs, native, turn.ID).Scan(&seq)
		result.ThroughSequence = seq
	} else {
		err = q.QueryRowContext(ctx, `SELECT coalesce(min(json_extract(data,'$.event.reference.sequence')),0) FROM hub_events WHERE `+refs, native, turn.ID).Scan(&seq)
		result.BeforeSequence = seq
	}
	if err != nil {
		return none, false, err
	}
	return result, result.AnchorTurn != "" || seq != 0, nil
}

// DegradeFork turns a recorded native fork into a Turn-index continuation,
// in one transaction: the branch loses its fork intent and gets the index
// (the visible earlier Turns, or the source's for a new conversation), the
// reader gets the fork_degraded notice, and the Turn can run again with the
// returned handoff.
func (s *Store) DegradeFork(ctx context.Context, attemptID string) (handoffValue *hub.Handoff, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		attempt, e := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=?`, attemptID)
		if e != nil {
			return e
		}
		session, e := read[hub.Session](ctx, tx, liveSessionQuery, attempt.SessionID)
		if e != nil {
			return e
		}
		branch, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, attempt.BranchID, attempt.SessionID)
		if e != nil {
			return e
		}
		if branch.Fork == nil || branch.NativeSessionID != "" {
			return errors.New("branch has no unspent native fork to degrade")
		}
		sourceOwner := branch.Fork.Owner
		branch.Fork, branch.Watermark = nil, 0
		if sourceOwner != session.ID && session.ForkedFrom != nil {
			source, e := read[hub.Session](ctx, tx, rawSessionQuery, session.ForkedFrom.SessionID)
			if e != nil {
				return e
			}
			if branch.Index, e = forkIndexEntries(ctx, tx, source, session.ForkedFrom.TurnSequence); e != nil {
				return e
			}
			if handoffValue, e = indexHandoff(session, branch); e != nil {
				return e
			}
		} else if handoffValue, e = handoff(ctx, tx, session, branch); e != nil {
			return e
		}
		if e = writeBranch(ctx, tx, branch); e != nil {
			return e
		}
		if e = saveHandoff(ctx, tx, session.ID, attempt.TurnID, attempt.ID, handoffValue); e != nil {
			return e
		}
		return timeline(ctx, tx, session.ID, attempt.TurnID, attempt.ID, hub.TimelineEvent{Kind: "fork_degraded", Text: "此引擎不能原生分叉，模型只看到摘要"})
	})
	return handoffValue, err
}

// ForkAnchor is the read-only form of the anchor the next fork would record.
func (s *Store) ForkAnchor(ctx context.Context, id, turnID string, keepThrough bool) (anchor hub.BranchFork, ok bool, err error) {
	defer func() { err = normalizeStoreError(err) }()
	session, err := read[hub.Session](ctx, s.list, liveSessionQuery, id)
	if err != nil {
		return anchor, false, err
	}
	turn, _, err := versionableTurn(ctx, s.list, session, turnID)
	if err != nil {
		return anchor, false, err
	}
	branch, err := read[hub.Branch](ctx, s.list, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, id)
	if err != nil {
		return anchor, false, err
	}
	return forkAnchor(ctx, s.list, id, branch, turn, keepThrough)
}
