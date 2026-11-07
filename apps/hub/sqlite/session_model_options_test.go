// INPUT: Real temporary SQLite, explicit session overrides and durable Attempt/wait admission.
// OUTPUT: CAS/reopen/rollback, concurrent settings preservation and immutable admitted recipe regressions.
// POS: [L7 product storage tests] Exercises session scope without native execution or external accounts.
package sqlite

import (
	"errors"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestSessionModelOptionsPersistCASClearAndAttemptFreeze(t *testing.T) {
	ctx, directory := t.Context(), filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	route := target("alva")
	defaults := storedModelPreference(route)
	if _, err = s.SaveModelPreference(ctx, defaults); err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(ctx, "session options", t.TempDir(), route)
	if err != nil {
		t.Fatal(err)
	}
	override := defaults.Options.Clone()
	override.SystemPrompt = "session override"
	edit := hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: route, Options: &override}
	saved, err := s.SaveSessionModelOptions(ctx, edit)
	if err != nil || saved.Revision != 1 {
		t.Fatal(saved, err)
	}
	if _, err = s.SaveSessionModelOptions(ctx, edit); !errors.Is(err, hub.ErrSessionModelOptionsConflict) {
		t.Fatal("stale edit accepted", err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "first")
	if err != nil || !reflect.DeepEqual(claim.Attempt.ModelOptions, override) {
		t.Fatal("override absent at admission", claim, err)
	}
	override.SystemPrompt = "next turn only"
	edit.ExpectedRevision = saved.Revision
	saved, err = s.SaveSessionModelOptions(ctx, edit)
	if err != nil {
		t.Fatal(err)
	}
	frozen, err := read[hub.Attempt](ctx, s.db, `SELECT data FROM hub_attempts WHERE id=?`, claim.Attempt.ID)
	if err != nil || frozen.ModelOptions.SystemPrompt != "session override" {
		t.Fatal("running Attempt changed", frozen, err)
	}
	settings, err := s.Settings(ctx, session.ID)
	if err != nil || settings.ModelOptions.SystemPrompt != "session override" {
		t.Fatal("admitted recipe changed", settings, err)
	}
	complete(t, s, claim, "native-session-options")
	settings.SessionModelOptions = hub.SessionModelOptionsSnapshot{}
	settings.SkillsEnabled = true
	if err = s.SaveSettings(ctx, settings); err != nil {
		t.Fatal(err)
	}
	after, err := s.SessionModelOptions(ctx, session.ID)
	if err != nil || !reflect.DeepEqual(after, saved) {
		t.Fatal("whole settings import replaced session overrides", after, err)
	}
	next, err := s.BeginTurn(ctx, session.ID, "next")
	if err != nil || next.Attempt.ModelOptions.SystemPrompt != "next turn only" {
		t.Fatal("next Attempt did not use updated override", next, err)
	}
	complete(t, s, next, "native-session-options")
	edit.Options, edit.ExpectedRevision = nil, saved.Revision
	saved, err = s.SaveSessionModelOptions(ctx, edit)
	if err != nil || len(saved.Entries) != 0 {
		t.Fatal("clear failed", saved, err)
	}
	next, err = s.BeginTurn(ctx, session.ID, "default again")
	if err != nil || !reflect.DeepEqual(next.Attempt.ModelOptions, defaults.Options) {
		t.Fatal("clear did not restore model defaults", next, err)
	}
	complete(t, s, next, "native-session-options")
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	after, err = s.SessionModelOptions(ctx, session.ID)
	if err != nil || after.Revision != saved.Revision || len(after.Entries) != 0 {
		t.Fatal("reopen lost clear receipt", after, err)
	}
}

func TestSessionModelOptionsRollbackReopenAndWaitingKeepsOldRecipe(t *testing.T) {
	ctx, directory := t.Context(), filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	route := target("alva")
	session, err := s.CreateSession(ctx, "wait options", t.TempDir(), route)
	if err != nil {
		t.Fatal(err)
	}
	options := hub.ModelOptions{SystemPrompt: "original session recipe", MaxTurns: 4}
	edit := hub.SessionModelOptionsUpdate{SessionID: session.ID, BranchID: session.ActiveBranchID, Target: route, Options: &options}
	saved, err := s.SaveSessionModelOptions(ctx, edit)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	reopened, err := s.SessionModelOptions(ctx, session.ID)
	if err != nil || !reflect.DeepEqual(reopened, saved) {
		t.Fatal("reopen lost override", reopened, err)
	}
	if err = s.SetDurableWaits(ctx, session.ID, true); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "original prompt")
	if err != nil {
		t.Fatal(err)
	}
	recordWaiting(t, s, claim)
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Waiting); err != nil {
		t.Fatal(err)
	}
	options.SystemPrompt = "future session recipe"
	edit.ExpectedRevision = saved.Revision
	if _, err = s.db.ExecContext(ctx, `CREATE TRIGGER fixture_override_failure BEFORE UPDATE ON hub_settings BEGIN SELECT RAISE(ABORT,'fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SaveSessionModelOptions(ctx, edit); err == nil {
		t.Fatal("failed write succeeded")
	}
	after, err := s.SessionModelOptions(ctx, session.ID)
	if err != nil || !reflect.DeepEqual(after, saved) {
		t.Fatal("failed write mutated saved override", after, err)
	}
	if _, err = s.db.ExecContext(ctx, `DROP TRIGGER fixture_override_failure`); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SaveSessionModelOptions(ctx, edit); err != nil {
		t.Fatal(err)
	}
	resumed, err := s.BeginWaitResume(ctx, session.ID, claim.Branch.ID, claim.Turn.ID, 4)
	if err != nil || !reflect.DeepEqual(resumed.Attempt.ModelOptions, claim.Attempt.ModelOptions) {
		t.Fatal("waiting resume followed edited override", resumed, err)
	}
	edit.BranchID, edit.ExpectedRevision = "stale-branch", 2
	if _, err = s.SaveSessionModelOptions(ctx, edit); !errors.Is(err, hub.ErrSessionModelOptionsConflict) {
		t.Fatal("stale branch edit accepted", err)
	}
}
