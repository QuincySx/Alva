// INPUT: Actual product SQLite settings, independently approved capabilities and Turn transactions.
// OUTPUT: Plan JSON persistence, approval preservation, import isolation and atomic running/external rejection.
// POS: [L7 storage tests] A plan transition changes one stored value and never clears unrelated host consent.
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestPlanModeRejectsExternalStartAfterRouteSwitch(t *testing.T) {
	s := newStore(t)
	p, err := s.CreateSession(t.Context(), "read-only native", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	branch, err := s.Switch(t.Context(), p.ID, target("external"), "explicit route choice is not policy approval")
	if err != nil {
		t.Fatal(err)
	}
	for _, begin := range []func() (hub.Claim, error){
		func() (hub.Claim, error) { return s.BeginTurn(t.Context(), p.ID, "must not run") },
		func() (hub.Claim, error) { return s.BeginTurnOnBranch(t.Context(), p.ID, branch.ID, "must not run") },
	} {
		if _, err = begin(); !errors.Is(err, engine.ErrUnsupported) {
			t.Fatal("external Turn lifted native Plan policy", err)
		}
	}
	if _, err = s.BranchTurn(t.Context(), p.ID, branch.ID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("rejected admission left a partially claimed Turn", err)
	}
}

func TestPlanModeBlocksAuthorizedExternalFallback(t *testing.T) {
	s := newStore(t)
	p, err := s.CreateSession(t.Context(), "read-only fallback", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	fallback := target("external")
	fallback.AutomaticAuthorized = true
	if err = s.SetFallback(t.Context(), p.ID, []hub.Target{fallback}); err != nil {
		t.Fatal(err)
	}
	runtime := hub.New(s)
	defer runtime.Close(t.Context())
	if err = runtime.Register("alva", runtimeFunc{run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := emit(engine.Event{Kind: engine.Error, Text: "fixture rate limit", Failure: engine.FailureRateLimit}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
	}}); err != nil {
		t.Fatal(err)
	}
	externalCalls := 0
	if err = runtime.Register("external", runtimeFunc{run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		externalCalls++
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	}}); err != nil {
		t.Fatal(err)
	}
	if err = runtime.Run(t.Context(), p.ID, "Plan remains read-only"); err == nil {
		t.Fatal("unsafe external fallback was reported successful")
	}
	detail, err := s.Detail(t.Context(), p.ID)
	if err != nil || externalCalls != 0 || len(detail.Branches) != 1 || detail.Branches[0].Target.EngineID != "alva" {
		t.Fatal("authorized fallback escaped Plan or committed a phantom branch", externalCalls, detail, err)
	}
}

func TestPlanModePreservesApprovalsAndPersists(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	s, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	p, err := s.CreateSession(t.Context(), "plan", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.ReplaceMCP(t.Context(), p.ID, []hub.MCPServer{{ID: "approved", Command: "never-started"}}); err != nil {
		t.Fatal(err)
	}
	if err = s.SetMCPEnabled(t.Context(), p.ID, "approved", true, true); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	value, err := s.Settings(t.Context(), p.ID)
	if err != nil || !value.PlanMode || !value.MCP[0].Enabled || !value.MCP[0].HostAccessApproved {
		t.Fatal("plan change cleared unrelated approval", value, err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	value, err = s.Settings(t.Context(), p.ID)
	if err != nil || !value.PlanMode {
		t.Fatal("plan choice not durable", value.PlanMode, err)
	}
	// Generic configuration import follows its existing disabled-import policy,
	// but cannot lift a previously selected read-only policy.
	value.PlanMode = false
	if err = s.SaveSettings(t.Context(), value); err != nil {
		t.Fatal(err)
	}
	value, err = s.Settings(t.Context(), p.ID)
	if err != nil || !value.PlanMode {
		t.Fatal("configuration import changed permission policy", err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, false); err != nil {
		t.Fatal(err)
	}
	value, err = s.Settings(t.Context(), p.ID)
	if err != nil || value.PlanMode {
		t.Fatal("explicit plan exit failed", err)
	}
}

func TestPlanModeRejectsAdmittedAndExternalBranches(t *testing.T) {
	s, err := Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	p, err := s.CreateSession(t.Context(), "plan", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	if _, err = s.BeginTurn(t.Context(), p.ID, "admitted"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, false); err == nil {
		t.Fatal("admitted Turn policy changed")
	}
	other, err := s.CreateSession(t.Context(), "external", t.TempDir(), target("external"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), other.ID, true); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("unimplemented external profile accepted", err)
	}
	if err = s.SetPlanMode(t.Context(), "missing", true); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("backend absence did not retain shared vocabulary", err)
	}
}
