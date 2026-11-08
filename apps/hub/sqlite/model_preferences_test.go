// INPUT: Real private SQLite, explicit preference edits and actual Attempt/wait transactions.
// OUTPUT: Independent CAS, rollback/reopen, concurrent per-model writes and frozen execution defaults.
// POS: [L7 product storage testing] No external account, model request or native execution.
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

func storedModelPreference(target hub.Target) hub.ModelPreference {
	temperature, tokens := .4, uint32(2048)
	return hub.ModelPreference{Key: hub.ModelPreferenceKey(target), Favorite: true, PreferredEngine: target.EngineID,
		Options: hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &temperature, MaxTokens: &tokens, ReasoningEffort: "low"}, SystemPrompt: "Explicit default", MaxTurns: 3}}
}

func TestModelPreferencesCASRollbackAndReopen(t *testing.T) {
	ctx, directory := t.Context(), filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	initial, err := s.ModelPreferences(ctx)
	if err != nil || initial.Revision != 1 || len(initial.Entries) != 0 {
		t.Fatal(initial, err)
	}
	p := storedModelPreference(target("alva"))
	saved, err := s.SaveModelPreference(ctx, p)
	if err != nil || saved.Revision != 2 || len(saved.Entries) != 1 || saved.Entries[0].Revision != 1 {
		t.Fatal(saved, err)
	}
	if _, err = s.SaveModelPreference(ctx, p); !errors.Is(err, hub.ErrModelPreferenceConflict) {
		t.Fatal("stale create accepted", err)
	}
	p = saved.Entries[0].Clone()
	if _, err = s.db.ExecContext(ctx, `CREATE TRIGGER fixture_model_preference_failure BEFORE UPDATE ON hub_model_preferences BEGIN SELECT RAISE(ABORT,'fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	p.Favorite, p.Options.SystemPrompt = false, "must not publish"
	if _, err = s.SaveModelPreference(ctx, p); err == nil {
		t.Fatal("injected transaction failure succeeded")
	}
	after, err := s.ModelPreferences(ctx)
	if err != nil || !reflect.DeepEqual(after, saved) {
		t.Fatal("failed preference partially committed", after, err)
	}
	if _, err = s.db.ExecContext(ctx, `DROP TRIGGER fixture_model_preference_failure`); err != nil {
		t.Fatal(err)
	}
	canceled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err = s.SaveModelPreference(canceled, p); !errors.Is(err, context.Canceled) {
		t.Fatal("canceled save committed", err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	reopened, err := s.ModelPreferences(ctx)
	if err != nil || !reflect.DeepEqual(reopened, saved) {
		t.Fatal("reopen lost committed preferences", reopened, err)
	}
	*reopened.Entries[0].Options.Generation.Temperature = .9
	fresh, err := s.ModelPreferences(ctx)
	if err != nil || *fresh.Entries[0].Options.Generation.Temperature != .4 {
		t.Fatal("caller mutated durable preferences", err)
	}
}

func TestModelPreferencesConcurrentCASIsPerModel(t *testing.T) {
	for _, sameModel := range []bool{false, true} {
		name := "different-models"
		if sameModel {
			name = "same-model"
		}
		t.Run(name, func(t *testing.T) {
			s := newStore(t)
			start, results := make(chan struct{}), make(chan error, 16)
			var ready sync.WaitGroup
			ready.Add(16)
			for i := range 16 {
				go func() {
					value := target("alva")
					if !sameModel {
						value.ModelID = string(rune('a' + i))
					}
					ready.Done()
					<-start
					_, err := s.SaveModelPreference(t.Context(), storedModelPreference(value))
					results <- err
				}()
			}
			ready.Wait()
			close(start)
			wins, conflicts := 0, 0
			for range 16 {
				err := <-results
				if err == nil {
					wins++
				} else if errors.Is(err, hub.ErrModelPreferenceConflict) {
					conflicts++
				} else {
					t.Fatal(err)
				}
			}
			wantWins := 16
			if sameModel {
				wantWins = 1
			}
			if wins != wantWins || conflicts != 16-wantWins {
				t.Fatal("CAS crossed model identities", wins, conflicts)
			}
			value, err := s.ModelPreferences(t.Context())
			if err != nil || len(value.Entries) != wantWins || value.Revision != uint64(wantWins+1) {
				t.Fatal(value, err)
			}
		})
	}
}

func TestModelOptionsFreezeAtAdmissionAndOrdinarySettingsCannotReplaceThem(t *testing.T) {
	s, route := newStore(t), target("alva")
	p := storedModelPreference(route)
	snapshot, err := s.SaveModelPreference(t.Context(), p)
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(t.Context(), "frozen options", t.TempDir(), route)
	if err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(t.Context(), session.ID, "first")
	if err != nil || !reflect.DeepEqual(claim.Attempt.ModelOptions, p.Options) {
		t.Fatal("admission ignored preference defaults", claim, err)
	}
	p = snapshot.Entries[0].Clone()
	p.Options.SystemPrompt = "later preference"
	*p.Options.Generation.Temperature = .8
	if _, err = s.SaveModelPreference(t.Context(), p); err != nil {
		t.Fatal(err)
	}
	stored, err := read[hub.Attempt](t.Context(), s.db, `SELECT data FROM hub_attempts WHERE id=?`, claim.Attempt.ID)
	if err != nil || stored.ModelOptions.SystemPrompt != "Explicit default" || *stored.ModelOptions.Generation.Temperature != .4 {
		t.Fatal("running Attempt followed changed defaults", stored, err)
	}
	settings, err := s.Settings(t.Context(), session.ID)
	if err != nil || !reflect.DeepEqual(settings.ModelOptions, claim.Attempt.ModelOptions) {
		t.Fatal("settings did not mirror admitted options", settings, err)
	}
	complete(t, s, claim, "native-options")
	settings.ModelOptions = hub.ModelOptions{SystemPrompt: "forged import", MaxTurns: 900}
	if err = s.SaveSettings(t.Context(), settings); err != nil {
		t.Fatal(err)
	}
	settings, err = s.Settings(t.Context(), session.ID)
	if err != nil || !reflect.DeepEqual(settings.ModelOptions, claim.Attempt.ModelOptions) {
		t.Fatal("ordinary settings overwrote admitted options", settings, err)
	}
	next, err := s.BeginTurn(t.Context(), session.ID, "next")
	if err != nil || !reflect.DeepEqual(next.Attempt.ModelOptions, p.Options) {
		t.Fatal("next admission did not apply newly saved defaults", next, err)
	}
}

func TestModelOptionsWaitingResumeRetainsOriginalAttemptDefaults(t *testing.T) {
	s, route := newStore(t), target("alva")
	p := storedModelPreference(route)
	snapshot, err := s.SaveModelPreference(t.Context(), p)
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(t.Context(), "waiting options", t.TempDir(), route)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetDurableWaits(t.Context(), session.ID, true); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(t.Context(), session.ID, "original prompt")
	if err != nil {
		t.Fatal(err)
	}
	recordWaiting(t, s, claim)
	if err = s.FinishTurn(t.Context(), claim.Turn.ID, engine.Waiting); err != nil {
		t.Fatal(err)
	}
	p = snapshot.Entries[0].Clone()
	p.Options.SystemPrompt, p.Options.MaxTurns = "changed while waiting", 9
	*p.Options.Generation.Temperature = 1.1
	if _, err = s.SaveModelPreference(t.Context(), p); err != nil {
		t.Fatal(err)
	}
	resumed, err := s.BeginWaitResume(t.Context(), session.ID, claim.Branch.ID, claim.Turn.ID, 4)
	if err != nil || resumed.Attempt.ID == claim.Attempt.ID || resumed.Turn.ID != claim.Turn.ID || !reflect.DeepEqual(resumed.Attempt.ModelOptions, claim.Attempt.ModelOptions) {
		t.Fatal("same Turn resume adopted later preferences", resumed, err)
	}
}

func TestModelOptionsUnsupportedTargetRejectsAdmissionAtomically(t *testing.T) {
	for _, agent := range []string{"claude", "pi", "codex"} {
		t.Run(agent, func(t *testing.T) {
			s := newStore(t)
			route := target("alva")
			if _, err := s.SaveModelPreference(t.Context(), storedModelPreference(route)); err != nil {
				t.Fatal(err)
			}
			session, err := s.CreateSession(t.Context(), "unsupported defaults", t.TempDir(), route)
			if err != nil {
				t.Fatal(err)
			}
			route.EngineID = agent
			if _, err = s.Switch(t.Context(), session.ID, route, "Explicit configured Agent choice"); err != nil {
				t.Fatal(err)
			}
			before, err := s.Detail(t.Context(), session.ID)
			if err != nil {
				t.Fatal(err)
			}
			settingsBefore, err := s.Settings(t.Context(), session.ID)
			if err != nil {
				t.Fatal(err)
			}
			if _, err = s.BeginTurn(t.Context(), session.ID, "must remain a draft"); !errors.Is(err, engine.ErrUnsupported) {
				t.Fatal("unsupported defaults were admitted", err)
			}
			after, err := s.Detail(t.Context(), session.ID)
			if err != nil || !reflect.DeepEqual(after, before) {
				t.Fatal("rejected admission changed product detail", after, err)
			}
			settingsAfter, err := s.Settings(t.Context(), session.ID)
			if err != nil || !reflect.DeepEqual(settingsAfter, settingsBefore) {
				t.Fatal("rejected admission changed frozen settings", settingsAfter, err)
			}
			var facts int
			if err = s.db.QueryRowContext(t.Context(), `SELECT (SELECT count(*) FROM hub_turns WHERE session=?)+(SELECT count(*) FROM hub_attempts WHERE session=?)`, session.ID, session.ID).Scan(&facts); err != nil || facts != 0 {
				t.Fatal("unsupported model defaults created a Turn or Attempt", facts, err)
			}
		})
	}
}

func TestModelPreferencesCorruptionFailsWithoutReset(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(t.Context(), `UPDATE hub_model_preferences SET data='{}' WHERE id=1`); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ModelPreferences(t.Context()); err == nil {
		t.Fatal("corrupt snapshot accepted")
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if reopened, err := Open(t.Context(), directory); err == nil {
		_ = reopened.Close()
		t.Fatal("corrupt preferences were silently reset")
	}
}

func TestModelPreferencesCurrentSchemaMissingTableIsRejectedWithoutRepair(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(t.Context(), `DROP TABLE hub_model_preferences`); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if reopened, err := Open(t.Context(), directory); err == nil {
		_ = reopened.Close()
		t.Fatal("current missing model-preference authority was repaired silently")
	}
	db, err := private.Open(t.Context(), directory, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var version, tableCount int
	if err = db.QueryRowContext(t.Context(), `PRAGMA user_version`).Scan(&version); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRowContext(t.Context(), `SELECT count(*) FROM sqlite_master WHERE type='table' AND name='hub_model_preferences'`).Scan(&tableCount); err != nil || version != 9 || tableCount != 0 {
		t.Fatal("failed open changed the current database", version, tableCount, err)
	}
}
