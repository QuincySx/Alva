// INPUT: Real private SQLite product stores, a v9 database rebuilt by hand, and ordinary Turns with Alva/other engine branches
// OUTPUT: v10/v11 migration, version line creation/switching/visibility, segmented history, handoff and fork regressions
// POS: [L7 Hub product] Proves a version never loses or leaks history: the visible lines read exactly their Turns, and a session without versions reads as before
package sqlite

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

// turnReply runs one Turn to success with a reply the history shows.
func turnReply(t *testing.T, s *Store, id string, input hub.ComposerTurnInput, native, reply string) hub.Claim {
	t.Helper()
	ctx := context.Background()
	if input.UserText == "" {
		input.UserText = input.Prompt
	}
	claim, err := s.BeginComposerTurn(ctx, id, "", input)
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: native},
		{Kind: engine.Message, ID: "m", Text: reply},
		{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded, NativeTurn: "nt-" + claim.Attempt.ID},
	} {
		if err = s.Record(ctx, claim.Attempt.ID, event, false); err != nil {
			t.Fatal(err)
		}
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	return claim
}

func visibleText(t *testing.T, s *Store, id string) string {
	t.Helper()
	entries, err := s.History(context.Background(), id, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var parts []string
	for _, entry := range entries {
		switch {
		case entry.Input != "":
			parts = append(parts, "U:"+entry.Input)
		case entry.Event.Kind == engine.Message:
			parts = append(parts, "A:"+entry.Event.Text)
		}
	}
	return strings.Join(parts, "|")
}

func visibleBefore(t *testing.T, s *Store, id string) string {
	t.Helper()
	entries, err := s.HistoryBefore(context.Background(), id, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var parts []string
	for _, entry := range entries {
		switch {
		case entry.Input != "":
			parts = append(parts, "U:"+entry.Input)
		case entry.Event.Kind == engine.Message:
			parts = append(parts, "A:"+entry.Event.Text)
		}
	}
	return strings.Join(parts, "|")
}

func threeTurns(t *testing.T, s *Store) (hub.Session, []hub.Claim) {
	t.Helper()
	session, err := s.CreateSession(context.Background(), "versions", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	var claims []hub.Claim
	for _, pair := range [][2]string{{"one", "reply-one"}, {"two", "reply-two"}, {"three", "reply-three"}} {
		claims = append(claims, turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: pair[0]}, "native-main", pair[1]))
	}
	return session, claims
}

func TestEditCreatesVersionLineAndHidesTheOldOne(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	before := visibleText(t, s, session.ID)
	if before != "U:one|A:reply-one|U:two|A:reply-two|U:three|A:reply-three" {
		t.Fatal("setup history", before)
	}
	if lines, err := s.Lines(ctx, session.ID); err != nil || len(lines) != 0 {
		t.Fatal("a session without versions has no lines", lines, err)
	}
	// Edit the second Turn: history before it stays, the old tail is hidden.
	edited := turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID}}, "native-edit", "reply-edited")
	if got := visibleText(t, s, session.ID); got != "U:one|A:reply-one|U:two-edited|A:reply-edited" {
		t.Fatal("edited version shows the wrong history", got)
	}
	if got := visibleBefore(t, s, session.ID); got != "U:one|A:reply-one|U:two-edited|A:reply-edited" {
		t.Fatal("backward read shows the wrong history", got)
	}
	lines, err := s.Lines(ctx, session.ID)
	if err != nil || len(lines) != 2 {
		t.Fatal("edit must leave the original and the new line", lines, err)
	}
	detail, err := s.Detail(ctx, session.ID)
	if err != nil || detail.Session.ActiveLineID == "" || detail.Session.ActiveLineID != lines[1].ID {
		t.Fatal("the new line must be active", detail.Session, err)
	}
	if len(detail.Branches) != 1 || detail.Branches[0].ID != detail.Session.ActiveBranchID {
		t.Fatal("the new line owns exactly its own branch", detail.Branches)
	}
	// Both versions of the second Turn share one fork position.
	var group string
	for _, g := range lines[1].Groups {
		if g.TurnID == edited.Turn.ID {
			group = g.Group
		}
	}
	if group == "" {
		t.Fatal("the edited Turn must be placed at its fork position", lines[1].Groups)
	}
	found := false
	for _, g := range lines[0].Groups {
		found = found || g.Group == group && g.TurnID == claims[1].Turn.ID
	}
	if !found {
		t.Fatal("the original Turn must sit at the same position", lines[0].Groups)
	}
	// Switch back: the original history is whole again, and nothing was lost.
	if err = s.SelectLine(ctx, session.ID, lines[0].ID); err != nil {
		t.Fatal(err)
	}
	if got := visibleText(t, s, session.ID); got != before {
		t.Fatal("switching back must show every original Turn", got)
	}
	if latest, err := s.LatestTurn(ctx, session.ID); err != nil || latest.ID != claims[2].Turn.ID {
		t.Fatal("latest Turn of the original version", latest.ID, err)
	}
	// A new message on the original line must not show the edited one's Turns.
	turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "four"}, "native-main", "reply-four")
	if got := visibleText(t, s, session.ID); got != before+"|U:four|A:reply-four" {
		t.Fatal("continuing the original version leaked or lost history", got)
	}
	if err = s.SelectLine(ctx, session.ID, lines[1].ID); err != nil {
		t.Fatal(err)
	}
	if got := visibleText(t, s, session.ID); got != "U:one|A:reply-one|U:two-edited|A:reply-edited" {
		t.Fatal("the edited version must not see the original's later Turn", got)
	}
}

func TestRegenerateKeepsPromptAndAddsAVersion(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	// Only the last reply can be regenerated.
	_, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "two", UserText: "two", Line: &hub.LineFork{TurnID: claims[1].Turn.ID, Regenerate: true}})
	if !errors.Is(err, hub.ErrVersionUnsupported) {
		t.Fatal("regenerating a middle Turn must be refused", err)
	}
	if lines, _ := s.Lines(ctx, session.ID); len(lines) != 0 {
		t.Fatal("a refused regenerate must leave no line", lines)
	}
	turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "three", Line: &hub.LineFork{TurnID: claims[2].Turn.ID, Regenerate: true}}, "native-regen", "reply-three-again")
	if got := visibleText(t, s, session.ID); got != "U:one|A:reply-one|U:two|A:reply-two|U:three|A:reply-three-again" {
		t.Fatal("regenerated version", got)
	}
	lines, _ := s.Lines(ctx, session.ID)
	if len(lines) != 2 {
		t.Fatal("regenerate adds exactly one version", lines)
	}
}

func TestVersionsRefuseTurnsThatCannotHaveThem(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	running, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "running", UserText: "running"})
	if err != nil {
		t.Fatal(err)
	}
	_, err = s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "x", UserText: "x", Line: &hub.LineFork{TurnID: claims[0].Turn.ID}})
	if err == nil {
		t.Fatal("a running session must not start a version")
	}
	if err = s.SelectLine(ctx, session.ID, "line-none"); err == nil {
		t.Fatal("a session without lines has nothing to select")
	}
	if err = s.Record(ctx, running.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, running.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	// A Turn with files, a continue Turn and an unknown Turn are refused.
	withFile, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "p", UserText: "p", Files: []engine.InputFile{{ID: "f1", URI: "alva-artifact://f", Name: "f.txt", Path: filepath.Join(t.TempDir(), "f.txt"), MediaType: "text/plain", SHA256: strings.Repeat("a", 64), Bytes: 1}}})
	if err != nil {
		t.Fatal("file fixture must be admissible: ", err)
	}
	if err = s.Record(ctx, withFile.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, withFile.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	versionable, err := s.TurnVersionable(ctx, session.ID, withFile.Turn.ID)
	if err != nil || versionable.Reason == "" {
		t.Fatal("a Turn with files must say why it cannot have versions", versionable, err)
	}
	if _, err = s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "x", UserText: "x", Line: &hub.LineFork{TurnID: withFile.Turn.ID}}); !errors.Is(err, hub.ErrVersionUnsupported) {
		t.Fatal("admission itself must refuse a version of a Turn with files", err)
	}
	if lines, _ := s.Lines(ctx, session.ID); len(lines) != 0 {
		t.Fatal("a refused version leaves no line behind", lines)
	}
	if _, err = s.TurnVersionable(ctx, session.ID, "turn-unknown"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("unknown Turn", err)
	}
	if versionable, err = s.TurnVersionable(ctx, session.ID, claims[0].Turn.ID); err != nil || versionable.Reason != "" || versionable.UserText != "one" {
		t.Fatal("an ordinary Turn is versionable and returns its text", versionable, err)
	}
}

func TestTeamLeadSessionsHaveNoVersions(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	if _, err := s.db.ExecContext(ctx, `INSERT INTO hub_automations(id,session,enabled,next,running,data) VALUES('a1',?,0,0,'','{}')`, session.ID); err != nil {
		t.Fatal(err)
	}
	versionable, err := s.TurnVersionable(ctx, session.ID, claims[0].Turn.ID)
	if err != nil || versionable.Reason == "" {
		t.Fatal("a session with automations stays unversioned in v1", versionable, err)
	}
}

func TestNewVersionForksNativelyOrFallsBackToTheIndex(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	edit := func(native bool, text string) hub.Claim {
		return turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: text, Line: &hub.LineFork{TurnID: claims[1].Turn.ID, NativeFork: native}}, "native-"+text, "r-"+text)
	}
	forked, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "forked", UserText: "forked", Line: &hub.LineFork{TurnID: claims[1].Turn.ID, NativeFork: true}})
	if err != nil {
		t.Fatal(err)
	}
	// Turns two and three ran on the source native session: both are dropped.
	if forked.Branch.Fork == nil || forked.Branch.Fork.Session != "native-main" || forked.Branch.Fork.Owner != session.ID || forked.Branch.Fork.AnchorTurn != "nt-"+claims[1].Attempt.ID || forked.Branch.Fork.AnchorKeep {
		t.Fatal("native fork intent", forked.Branch.Fork)
	}
	if forked.Branch.Watermark != claims[0].Turn.Sequence || forked.Branch.NativeSessionID != "" {
		t.Fatal("the new branch keeps only the earlier Turns as seen", forked.Branch)
	}
	if forked.Handoff != nil {
		t.Fatal("a native fork needs no handoff index", forked.Handoff)
	}
	// The intent is persisted with the branch and cleared by SessionStarted.
	detail, _ := s.Detail(ctx, session.ID)
	if detail.Branches[0].Fork == nil {
		t.Fatal("fork intent must be durable before the engine runs")
	}
	if err = s.Record(ctx, forked.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: "native-forked"}, false); err != nil {
		t.Fatal(err)
	}
	detail, _ = s.Detail(ctx, session.ID)
	if detail.Branches[0].Fork != nil || detail.Branches[0].NativeSessionID != "native-forked" {
		t.Fatal("the fork intent must clear once the native session exists", detail.Branches[0])
	}
	if err = s.Record(ctx, forked.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, forked.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	// An engine that cannot fork: fresh native conversation, the index of the
	// visible earlier Turns, and a visible notice.
	lines, _ := s.Lines(ctx, session.ID)
	if err = s.SelectLine(ctx, session.ID, lines[0].ID); err != nil {
		t.Fatal(err)
	}
	plain := edit(false, "plain")
	_ = plain
	entries, err := s.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	notice := false
	for _, entry := range entries {
		notice = notice || entry.Timeline != nil && entry.Timeline.Kind == "fork_degraded"
	}
	if !notice {
		t.Fatal("a version of an engine without native fork must say the model sees only a summary")
	}
}

func TestVersionHandoffIndexesOnlyTheVisibleLine(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	// A different engine is asked on the edited version: its handoff index must
	// list the kept earlier Turn and the edited one, never the hidden tail.
	turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID}}, "native-edit", "reply-edited")
	if _, err := s.Switch(ctx, session.ID, target("other"), "route to another engine"); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "next")
	if err != nil {
		t.Fatal(err)
	}
	if claim.Handoff == nil {
		t.Fatal("a new branch must receive the Turn index")
	}
	var prompts []string
	for _, entry := range claim.Handoff.Entries {
		prompts = append(prompts, entry.Prompt)
	}
	if strings.Join(prompts, "|") != "one|two-edited" {
		t.Fatal("handoff index must follow the visible version", prompts)
	}
}

func TestSelectVersionOnlyWhenIdleAndRestoresItsBranch(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	edited := turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID}}, "native-edit", "reply-edited")
	lines, _ := s.Lines(ctx, session.ID)
	editedBranch := edited.Branch.ID
	running, err := s.BeginTurn(ctx, session.ID, "running")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SelectLine(ctx, session.ID, lines[0].ID); !errors.Is(err, hub.ErrVersionBusy) {
		t.Fatal("switching while running must be refused", err)
	}
	if err = s.Record(ctx, running.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Failed}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, running.Turn.ID, engine.Failed); err != nil {
		t.Fatal(err)
	}
	if err = s.SelectLine(ctx, session.ID, lines[0].ID); err != nil {
		t.Fatal(err)
	}
	detail, _ := s.Detail(ctx, session.ID)
	if detail.Session.ActiveBranchID == editedBranch || detail.Session.Status != engine.Succeeded {
		t.Fatal("the original version restores its own branch and its own status", detail.Session)
	}
	if err = s.SelectLine(ctx, session.ID, lines[1].ID); err != nil {
		t.Fatal(err)
	}
	detail, _ = s.Detail(ctx, session.ID)
	if detail.Session.ActiveBranchID != editedBranch || detail.Session.Status != engine.Failed {
		t.Fatal("the edited version restores its branch and the failure it ended with", detail.Session)
	}
	if err = s.SelectLine(ctx, session.ID, "line-missing"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("unknown line", err)
	}
}

func TestLineBudgetAndBranchesPerVersion(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "budget", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	first := turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "q"}, "native-main", "a")
	var failure error
	for i := 0; i < hub.MaxLinesPerSession+2 && failure == nil; i++ {
		current, e := s.LatestTurn(ctx, session.ID)
		if e != nil {
			t.Fatal(e)
		}
		claim, e := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "q", UserText: "q", Line: &hub.LineFork{TurnID: current.ID, Regenerate: true}})
		if e != nil {
			failure = e
			break
		}
		if e = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Message, ID: "m", Text: "a"}, false); e != nil {
			t.Fatal(e)
		}
		if e = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); e != nil {
			t.Fatal(e)
		}
		if e = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); e != nil {
			t.Fatal(e)
		}
	}
	if !errors.Is(failure, hub.ErrVersionBudget) {
		t.Fatal("the 65th line must be refused with the budget error", failure)
	}
	lines, _ := s.Lines(ctx, session.ID)
	if len(lines) != hub.MaxLinesPerSession {
		t.Fatal("line budget", len(lines))
	}
	_ = first
	if got := visibleText(t, s, session.ID); got != "U:q|A:a" {
		t.Fatal("after many regenerations exactly one version is shown", got)
	}
}

func TestForkSessionCarriesOriginAndNativeIntent(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	forked, err := s.ForkSession(ctx, session.ID, hub.SessionFork{TurnID: claims[1].Turn.ID, NativeFork: true})
	if err != nil {
		t.Fatal(err)
	}
	if forked.ID == session.ID || forked.ForkedFrom == nil || forked.ForkedFrom.SessionID != session.ID || forked.ForkedFrom.TurnID != claims[1].Turn.ID || forked.ForkedFrom.EventSequence == 0 {
		t.Fatal("fork origin", forked.ForkedFrom)
	}
	detail, err := s.Detail(ctx, forked.ID)
	if err != nil || len(detail.Branches) != 1 {
		t.Fatal(err)
	}
	branch := detail.Branches[0]
	if branch.Fork == nil || branch.Fork.Owner != session.ID || branch.Fork.Session != "native-main" || branch.Fork.AnchorTurn != "nt-"+claims[1].Attempt.ID || !branch.Fork.AnchorKeep {
		t.Fatal("the fork keeps through the second Turn and drops only the third", branch.Fork)
	}
	if got := visibleText(t, s, forked.ID); got != "" {
		t.Fatal("a fork copies no history", got)
	}
	// An engine without native fork gets the bounded index of the kept Turns.
	degraded, err := s.ForkSession(ctx, session.ID, hub.SessionFork{TurnID: claims[1].Turn.ID})
	if err != nil {
		t.Fatal(err)
	}
	detail, _ = s.Detail(ctx, degraded.ID)
	if detail.Branches[0].Fork != nil || len(detail.Branches[0].Index) != 2 {
		t.Fatal("index of the kept Turns", detail.Branches[0])
	}
	claim, err := s.BeginTurn(ctx, degraded.ID, "continue here")
	if err != nil || claim.Handoff == nil || len(claim.Handoff.Entries) != 2 || claim.Handoff.Entries[1].Prompt != "two" {
		t.Fatal("the first Turn of a degraded fork carries the index", claim.Handoff, err)
	}
	// The source is untouched, and a running source cannot be forked.
	if got := visibleText(t, s, session.ID); !strings.HasSuffix(got, "A:reply-three") {
		t.Fatal("source history changed", got)
	}
	running, err := s.BeginTurn(ctx, session.ID, "running")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.ForkSession(ctx, session.ID, hub.SessionFork{TurnID: claims[0].Turn.ID}); !errors.Is(err, hub.ErrSessionRunning) {
		t.Fatal("a running source must refuse a fork", err)
	}
	_ = running
}

func TestForkSessionFromAnotherVersionKeepsOnlyItsHistory(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, claims := threeTurns(t, s)
	edited := turnReply(t, s, session.ID, hub.ComposerTurnInput{Prompt: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID}}, "native-edit", "reply-edited")
	if _, err := s.ForkSession(ctx, session.ID, hub.SessionFork{TurnID: claims[2].Turn.ID}); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("a Turn of a hidden version cannot be forked", err)
	}
	forked, err := s.ForkSession(ctx, session.ID, hub.SessionFork{TurnID: edited.Turn.ID})
	if err != nil {
		t.Fatal(err)
	}
	detail, _ := s.Detail(ctx, forked.ID)
	var prompts []string
	for _, entry := range detail.Branches[0].Index {
		prompts = append(prompts, entry.Prompt)
	}
	if strings.Join(prompts, "|") != "one|two-edited" {
		t.Fatal("fork index follows the visible version", prompts)
	}
}

// legacyV9 turns an open store's database back into the v9 shape: the old
// branch table without a line column, no queue and no line table.
func legacyV9(t *testing.T, dir string) {
	t.Helper()
	db, err := private.Open(context.Background(), dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	for _, statement := range []string{
		`DROP TABLE hub_lines`,
		`DROP TABLE hub_pending_inputs`,
		`CREATE TABLE hub_branches_old(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),route TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(session,route))`,
		`INSERT INTO hub_branches_old SELECT id,session,route,data FROM hub_branches`,
		`DROP TABLE hub_branches`,
		`ALTER TABLE hub_branches_old RENAME TO hub_branches`,
		`CREATE INDEX hub_native_route ON hub_branches(json_extract(data,'$.target.engine_id'),json_extract(data,'$.native_session_id'))`,
		`PRAGMA user_version=9`,
	} {
		if _, err = db.ExecContext(context.Background(), statement); err != nil {
			t.Fatal(statement, err)
		}
	}
}

func TestSchemaTenAndElevenMigrateV9WithoutLosingFacts(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	session, claims := threeTurns(t, store)
	if _, err = store.Switch(ctx, session.ID, target("other"), "second engine"); err != nil {
		t.Fatal(err)
	}
	type branchRow struct{ id, session, route, data string }
	snapshot := func(s *Store) (rows []branchRow, turns, events int) {
		r, err := s.db.QueryContext(ctx, `SELECT id,session,route,data FROM hub_branches ORDER BY id`)
		if err != nil {
			t.Fatal(err)
		}
		defer r.Close()
		for r.Next() {
			var row branchRow
			if err = r.Scan(&row.id, &row.session, &row.route, &row.data); err != nil {
				t.Fatal(err)
			}
			rows = append(rows, row)
		}
		return rows, rowCount(t, s, `SELECT count(*) FROM hub_turns`), rowCount(t, s, `SELECT count(*) FROM hub_events`)
	}
	beforeRows, beforeTurns, beforeEvents := snapshot(store)
	if len(beforeRows) != 2 || beforeTurns != 3 {
		t.Fatal("fixture", beforeRows, beforeTurns)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	legacyV9(t, dir)
	reopened, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	afterRows, afterTurns, afterEvents := snapshot(reopened)
	if len(afterRows) != len(beforeRows) || afterTurns != beforeTurns || afterEvents != beforeEvents {
		t.Fatal("migration changed the number of facts", afterRows, afterTurns, afterEvents)
	}
	for i := range beforeRows {
		if beforeRows[i] != afterRows[i] {
			t.Fatal("branch row changed in migration", beforeRows[i], afterRows[i])
		}
	}
	if rowCount(t, reopened, `PRAGMA user_version`) != 11 || rowCount(t, reopened, `SELECT count(*) FROM sqlite_master WHERE name IN ('hub_lines','hub_pending_inputs','hub_lines_session','hub_pending_session','hub_native_route')`) != 5 {
		t.Fatal("v10 and v11 objects missing after migration")
	}
	// Old sessions still read and still admit; their branches are line ''.
	if rowCount(t, reopened, `SELECT count(*) FROM hub_branches WHERE line=''`) != 2 {
		t.Fatal("migrated branches must keep the empty line")
	}
	if got := visibleText(t, reopened, session.ID); !strings.HasSuffix(got, "A:reply-three") {
		t.Fatal("history after migration", got)
	}
	detail, err := reopened.Detail(ctx, session.ID)
	if err != nil || len(detail.Branches) != 2 {
		t.Fatal("detail after migration", detail, err)
	}
	// The rebuilt uniqueness is (session,line,route): the same route in a
	// different line is allowed, in the same line it is not.
	insert := func(id, line string) error {
		_, err := reopened.db.ExecContext(ctx, `INSERT INTO hub_branches(id,session,route,data,line) VALUES(?,?,?,?,?)`, id, session.ID, "engine:alva", []byte(`{}`), line)
		return err
	}
	if insert("b-same", "") == nil {
		t.Fatal("the same session, line and route must be unique")
	}
	if err = insert("b-other-line", "line-x"); err != nil {
		t.Fatal("the same route in another line must be allowed", err)
	}
	// Versions work on the migrated session.
	turnReply(t, reopened, session.ID, hub.ComposerTurnInput{Prompt: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID}}, "native-edit", "reply-edited")
	if got := visibleText(t, reopened, session.ID); got != "U:one|A:reply-one|U:two-edited|A:reply-edited" {
		t.Fatal("version on a migrated session", got)
	}
}

func TestSchemaElevenMigrationFailureRollsBackBothVersions(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	session, _ := threeTurns(t, store)
	_ = session
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	legacyV9(t, dir)
	db, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	// A foreign table in the way makes the v11 rebuild fail after v10 succeeded.
	if _, err = db.ExecContext(ctx, `CREATE TABLE hub_branches_v11(foreign_fact TEXT); INSERT INTO hub_branches_v11 VALUES('keep')`); err != nil {
		t.Fatal(err)
	}
	db.Close()
	if opened, openErr := Open(ctx, dir); openErr == nil {
		opened.Close()
		t.Fatal("a failing v11 rebuild must fail Open")
	}
	db, err = private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, branches, pending, lines int
	if err = db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	_ = db.QueryRowContext(ctx, `SELECT count(*) FROM hub_branches`).Scan(&branches)
	_ = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE name='hub_pending_inputs'`).Scan(&pending)
	_ = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE name='hub_lines'`).Scan(&lines)
	if version != 9 || branches != 1 || pending != 0 || lines != 0 {
		t.Fatalf("migration is not atomic: version=%d branches=%d pending=%d lines=%d", version, branches, pending, lines)
	}
}

func TestContinueTurnsHaveNoVersions(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, _ := threeTurns(t, s)
	stopped, err := s.BeginTurn(ctx, session.ID, "stopped")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, stopped.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Cancelled}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, stopped.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	resumed, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Origin: hub.TurnOriginContinue, Prompt: "continue", UserText: ""})
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, resumed.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, resumed.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	versionable, err := s.TurnVersionable(ctx, session.ID, resumed.Turn.ID)
	if err != nil || versionable.Reason == "" {
		t.Fatal("a continue Turn cannot be edited or regenerated", versionable, err)
	}
}

// referencedTurn runs a Turn whose reply references native records seq..seq+1.
func referencedTurn(t *testing.T, s *Store, id string, input hub.ComposerTurnInput, native string, seq uint64) hub.Claim {
	t.Helper()
	ctx := context.Background()
	input.UserText = input.Prompt
	claim, err := s.BeginComposerTurn(ctx, id, "", input)
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: native},
		{Kind: engine.ToolStart, ID: "c", Name: "read_file", Effect: engine.EffectRead, Reference: &engine.Reference{SessionID: native, RecordID: "r1", Sequence: seq}},
		{Kind: engine.ToolEnd, ID: "c", Name: "read_file", Reference: &engine.Reference{SessionID: native, RecordID: "r2", Sequence: seq + 1}},
		{Kind: engine.Message, ID: "m", Text: "reply", Reference: &engine.Reference{SessionID: native, RecordID: "r3", Sequence: seq + 3}},
		{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded, NativeTurn: "nt-" + claim.Attempt.ID},
	} {
		if err = s.Record(ctx, claim.Attempt.ID, event, false); err != nil {
			t.Fatal(err)
		}
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	return claim
}

func TestForkAnchorIsFixedFromFactsThatNeverChange(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "anchor", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	var claims []hub.Claim
	for i, prompt := range []string{"one", "two", "three"} {
		claims = append(claims, referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: prompt}, "native-main", uint64(10*(i+1))))
	}
	anchorOf := func(i int) string { return "nt-" + claims[i].Attempt.ID }
	// Dropping the second Turn: its first native record is 20 and the engine's
	// own identity of it is the anchor.
	anchor, ok, err := s.ForkAnchor(ctx, session.ID, claims[1].Turn.ID, false)
	if err != nil || !ok || anchor.BeforeSequence != 20 || anchor.AnchorTurn != anchorOf(1) || anchor.AnchorKeep || anchor.Session != "native-main" {
		t.Fatal("drop anchor", anchor, ok, err)
	}
	// Keeping through a Turn: everything up to the last record it referenced.
	anchor, ok, err = s.ForkAnchor(ctx, session.ID, claims[1].Turn.ID, true)
	if err != nil || !ok || anchor.ThroughSequence != 23 || anchor.BeforeSequence != 0 || anchor.AnchorTurn != anchorOf(1) || !anchor.AnchorKeep {
		t.Fatal("keep anchor", anchor, ok, err)
	}
	anchor, ok, err = s.ForkAnchor(ctx, session.ID, claims[2].Turn.ID, true)
	if err != nil || !ok || anchor.ThroughSequence != 33 || anchor.AnchorTurn != anchorOf(2) {
		t.Fatal("keep-last anchor", anchor, ok, err)
	}
	// The source grows: the recorded cut of an earlier fork does not change.
	recorded, err := s.ForkSession(ctx, session.ID, hub.SessionFork{TurnID: claims[1].Turn.ID, NativeFork: true})
	if err != nil {
		t.Fatal(err)
	}
	referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: "four"}, "native-main", 40)
	referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: "five"}, "native-main", 50)
	detail, _ := s.Detail(ctx, recorded.ID)
	fork := detail.Branches[0].Fork
	if fork == nil || fork.ThroughSequence != 23 || fork.AnchorTurn != anchorOf(1) || !fork.AnchorKeep {
		t.Fatal("the recorded cut moved with the source", fork)
	}
	// A Turn that reported no native identity and left no references has no
	// anchor: its engine cannot vouch for where it started.
	anon, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "anonymous", UserText: "anonymous"})
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []engine.Event{{Kind: engine.SessionStarted, SessionID: "native-main"}, {Kind: engine.Completed, SessionID: "native-main", Outcome: engine.Succeeded}} {
		if err = s.Record(ctx, anon.Attempt.ID, event, false); err != nil {
			t.Fatal(err)
		}
	}
	if err = s.FinishTurn(ctx, anon.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	for _, keep := range []bool{false, true} {
		if _, ok, err = s.ForkAnchor(ctx, session.ID, anon.Turn.ID, keep); err != nil || ok {
			t.Fatal("an anonymous Turn must not be anchored", keep, ok, err)
		}
	}
	// A Turn that did not run on the branch's own native conversation has no
	// reliable anchor: inherited through an earlier fork, or never run natively.
	edited := referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID, NativeFork: true}}, "native-forked", 100)
	if _, ok, err = s.ForkAnchor(ctx, session.ID, claims[0].Turn.ID, false); err != nil || ok {
		t.Fatal("the first Turn lives in the parent branch's conversation; no anchor on the edited line", ok, err)
	}
	anchor, ok, err = s.ForkAnchor(ctx, session.ID, edited.Turn.ID, false)
	if err != nil || !ok || anchor.Session != "native-forked" || anchor.AnchorTurn != "nt-"+edited.Attempt.ID || anchor.BeforeSequence != 100 {
		t.Fatal("the edited Turn's own anchor names its own native turn", anchor, ok, err)
	}
}

func TestRecordedForkCutSurvivesAFailedStartAndSwitchingVersions(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, _ := s.CreateSession(ctx, "stable", t.TempDir(), target("alva"))
	var claims []hub.Claim
	for i, prompt := range []string{"one", "two", "three"} {
		claims = append(claims, referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: prompt}, "native-main", uint64(10*(i+1))))
	}
	failed, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "two-edited", UserText: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID, NativeFork: true}})
	if err != nil {
		t.Fatal(err)
	}
	recorded := *failed.Branch.Fork
	if err = s.Record(ctx, failed.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Failed}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, failed.Turn.ID, engine.Failed); err != nil {
		t.Fatal(err)
	}
	lines, _ := s.Lines(ctx, session.ID)
	if err = s.SelectLine(ctx, session.ID, lines[0].ID); err != nil {
		t.Fatal(err)
	}
	referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: "four"}, "native-main", 40)
	referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: "five"}, "native-main", 50)
	if err = s.SelectLine(ctx, session.ID, lines[1].ID); err != nil {
		t.Fatal(err)
	}
	retry, err := s.BeginTurn(ctx, session.ID, "retry")
	if err != nil {
		t.Fatal(err)
	}
	if retry.Branch.Fork == nil || *retry.Branch.Fork != recorded {
		t.Fatal("the retry must carry the cut recorded at the edit", retry.Branch.Fork, recorded)
	}
	if recorded.BeforeSequence != 20 || recorded.AnchorTurn != "nt-"+claims[1].Attempt.ID {
		t.Fatal("the recorded cut", recorded)
	}
}

func TestDegradeForkTurnsTheIntentIntoTheIndexInOneTransaction(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, _ := s.CreateSession(ctx, "degrade", t.TempDir(), target("alva"))
	var claims []hub.Claim
	for i, prompt := range []string{"one", "two", "three"} {
		claims = append(claims, referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: prompt}, "native-main", uint64(10*(i+1))))
	}
	edit, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "two-edited", UserText: "two-edited", Line: &hub.LineFork{TurnID: claims[1].Turn.ID, NativeFork: true}})
	if err != nil || edit.Branch.Fork == nil {
		t.Fatal(err, edit.Branch.Fork)
	}
	h, err := s.DegradeFork(ctx, edit.Attempt.ID)
	if err != nil || h == nil || len(h.Entries) != 1 || h.Entries[0].Prompt != "one" {
		t.Fatal("the index lists the kept Turn only", h, err)
	}
	detail, _ := s.Detail(ctx, session.ID)
	if detail.Branches[0].Fork != nil {
		t.Fatal("the fork intent must be gone at once, not only after the rerun starts", detail.Branches[0].Fork)
	}
	entries, _ := s.History(ctx, session.ID, 0, 128)
	notice := false
	for _, entry := range entries {
		notice = notice || entry.Timeline != nil && entry.Timeline.Kind == "fork_degraded" && entry.TurnID == edit.Turn.ID
	}
	if !notice {
		t.Fatal("fork_degraded notice missing")
	}
	if _, err = s.DegradeFork(ctx, edit.Attempt.ID); err == nil {
		t.Fatal("a branch without an unspent fork cannot be degraded again")
	}
}

func TestForkAnchorTakesTheEarliestNativeTurnOfARetriedTurn(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, _ := s.CreateSession(ctx, "retried", t.TempDir(), target("alva"))
	first := referencedTurn(t, s, session.ID, hub.ComposerTurnInput{Prompt: "one"}, "native-main", 10)
	// The second Turn's first attempt reached the native thread (turn "nt-early")
	// and failed; it was retried on the same conversation ("nt-retry"). The cut
	// of an edit must drop the old prompt that the first attempt left behind.
	turn, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "two", UserText: "two"})
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []engine.Event{{Kind: engine.SessionStarted, SessionID: "native-main"}, {Kind: engine.Completed, SessionID: "native-main", Outcome: engine.Failed, NativeTurn: "nt-early"}} {
		if err = s.Record(ctx, turn.Attempt.ID, event, false); err != nil {
			t.Fatal(err)
		}
	}
	if err = s.FinishTurn(ctx, turn.Turn.ID, engine.Failed); err != nil {
		t.Fatal(err)
	}
	later, _ := json.Marshal(hub.HistoryEntry{SessionID: session.ID, TurnID: turn.Turn.ID, AttemptID: turn.Attempt.ID, Event: engine.Event{Kind: engine.Completed, SessionID: "native-main", Outcome: engine.Succeeded, NativeTurn: "nt-retry"}})
	if _, err = s.db.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, session.ID, turn.Turn.ID, turn.Attempt.ID, later); err != nil {
		t.Fatal(err)
	}
	// A foreign engine's identity for the same Turn never becomes the anchor.
	other, _ := json.Marshal(hub.HistoryEntry{SessionID: session.ID, TurnID: turn.Turn.ID, AttemptID: turn.Attempt.ID, Event: engine.Event{Kind: engine.Completed, SessionID: "native-other", Outcome: engine.Succeeded, NativeTurn: "nt-foreign"}})
	if _, err = s.db.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, session.ID, turn.Turn.ID, turn.Attempt.ID, other); err != nil {
		t.Fatal(err)
	}
	anchor, ok, err := s.ForkAnchor(ctx, session.ID, turn.Turn.ID, false)
	if err != nil || !ok || anchor.AnchorTurn != "nt-early" {
		t.Fatal("the anchor must be the earliest native turn any attempt reported on this conversation", anchor, ok, err)
	}
	_ = first
}
