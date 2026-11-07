// INPUT: Real SQLite Turn claims, explicit file/Skill identities and deterministic fallback runtimes.
// OUTPUT: Frozen structured input delivery, unsupported-adapter isolation and preserved user-history regressions.
// POS: [L7 Hub testing] Verifies one existing Runtime owns admission/fallback without a second composer executor.
package hub_test

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

type composerInputEngine struct {
	identityEngine
	inputs bool
}

func (e composerInputEngine) Capabilities() engine.Capabilities {
	c := e.identityEngine.Capabilities()
	c.FileInputs, c.SkillInputs = e.inputs, e.inputs
	return c
}

func TestComposerTurnInputsSurviveFallbackWithoutDowngradingToText(t *testing.T) {
	store, session := identityStore(t)
	r := hub.New(store)
	ownIdentityRuntime(t, r)
	for _, id := range []string{"fixture", "text-only", "input-capable"} {
		id := id
		e := composerInputEngine{inputs: id != "text-only"}
		e.run = func(_ context.Context, request engine.Request, emit func(engine.Event) error) error {
			if id == "text-only" {
				t.Error("fallback silently received files/Skills without supporting them")
			}
			if id == "fixture" {
				if err := emit(engine.Event{Kind: engine.Error, Failure: engine.FailureStartup, Text: "fixture startup failed before side effects"}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
			}
			if len(request.InputFiles) != 1 || request.InputFiles[0].Name != "selected.txt" || len(request.InputSkills) != 1 || request.InputSkills[0].Name != "inspect" {
				t.Errorf("fallback lost frozen inputs: %+v %+v", request.InputFiles, request.InputSkills)
			}
			if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "input-native"}); err != nil {
				return err
			}
			return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
		}
		if err := r.Register(id, e); err != nil {
			t.Fatal(err)
		}
	}
	var fallback []hub.Target
	for _, id := range []string{"text-only", "input-capable"} {
		target := identityTarget(id)
		target.AutomaticAuthorized = true
		fallback = append(fallback, target)
	}
	if err := store.SetFallback(t.Context(), session.ID, fallback); err != nil {
		t.Fatal(err)
	}
	files := []engine.InputFile{{ID: "captured", Name: "selected.txt", URI: "composer://captured", Path: filepath.Join(t.TempDir(), "original"), MediaType: "text/plain", SHA256: strings.Repeat("a", 64), Bytes: 42}}
	skills := []engine.InputSkill{{Name: "inspect", Path: filepath.Join(session.Workspace, ".alva", "skills", "inspect", "SKILL.md"), SHA256: strings.Repeat("b", 64)}}
	_, done, err := r.StartComposerOnBranch(t.Context(), session.ID, session.ActiveBranchID, hub.ComposerTurnInput{Prompt: "user text plus bounded model-only metadata", UserText: "user text", Files: files, Skills: skills})
	if err != nil {
		t.Fatal(err)
	}
	files[0].Name = "changed after admission"
	skills[0].Name = "changed after admission"
	if err = identityReceive(t, done); err != nil {
		t.Fatal(err)
	}
	turn, err := store.BranchTurn(t.Context(), session.ID, session.ActiveBranchID)
	if err != nil || len(turn.InputFiles) != 1 || turn.InputFiles[0].Name != "selected.txt" || turn.InputSkills[0].Name != "inspect" {
		t.Fatal("durable Turn input changed", turn, err)
	}
	history, err := store.History(t.Context(), session.ID, 0, 100)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range history {
		if entry.Input != "" && (entry.Input != "user text" || len(entry.InputFiles) != 1 || len(entry.InputSkills) != 1) {
			t.Fatal("model-only manifest polluted original history", entry)
		}
	}
}

func TestComposerTurnRejectsInputsBeforeUnsupportedExecute(t *testing.T) {
	store, session := identityStore(t)
	r := hub.New(store)
	ownIdentityRuntime(t, r)
	calls := make(chan struct{}, 1)
	e := composerInputEngine{identityEngine: identityEngine{run: func(context.Context, engine.Request, func(engine.Event) error) error { calls <- struct{}{}; return nil }}}
	if err := r.Register("fixture", e); err != nil {
		t.Fatal(err)
	}
	_, done, err := r.StartComposerOnBranch(t.Context(), session.ID, session.ActiveBranchID, hub.ComposerTurnInput{Prompt: "use selected Skill", UserText: "use selected Skill", Skills: []engine.InputSkill{{Name: "inspect", Path: filepath.Join(session.Workspace, "SKILL.md"), SHA256: strings.Repeat("a", 64)}}})
	if err != nil {
		t.Fatal(err)
	}
	if err = identityReceive(t, done); !errors.Is(err, engine.ErrUnsupported) || len(calls) != 0 {
		t.Fatal("unsupported input reached Execute", err)
	}
}
