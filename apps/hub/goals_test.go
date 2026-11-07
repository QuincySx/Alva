// INPUT: Pure Goal creation values and explicit immutable project/session scopes.
// OUTPUT: Bounded names/criteria, revision and owner policy regressions.
// POS: [L7 Hub product tests] Validates desired outcomes without storage, runtime or UI policy.
package hub

import (
	"errors"
	"strings"
	"testing"
)

func TestGoalPolicyRequiresNameCriteriaAndExplicitScope(t *testing.T) {
	valid := GoalCreate{RequestID: "explicit-request", Name: "Ship goal", CompletionCriteria: "The build passes.\nThe receipt is retained.", Scope: GoalScope{Kind: GoalProjectScope, Workspace: t.TempDir()}}
	if err := valid.Validate(); err != nil {
		t.Fatal(err)
	}
	for _, change := range []func(*GoalCreate){
		func(v *GoalCreate) { v.RequestID = "" },
		func(v *GoalCreate) { v.Name = " " },
		func(v *GoalCreate) { v.Name = strings.Repeat("a", 257) },
		func(v *GoalCreate) { v.CompletionCriteria = "" },
		func(v *GoalCreate) { v.CompletionCriteria = strings.Repeat("a", 8193) },
		func(v *GoalCreate) { v.Scope.Workspace = "relative/project" },
		func(v *GoalCreate) { v.Scope.SessionID = "hidden-session" },
		func(v *GoalCreate) { v.Scope.Kind = GoalSessionScope },
	} {
		request := valid
		change(&request)
		if err := request.Validate(); !errors.Is(err, ErrInvalidGoal) {
			t.Fatal("invalid Goal accepted", request, err)
		}
	}
	goal := Goal{ID: "goal-stable", Revision: 1, RequestID: valid.RequestID, Name: valid.Name, CompletionCriteria: valid.CompletionCriteria, Scope: valid.Scope, CreatedAt: 1}
	if err := goal.Validate(); err != nil || goal.Creation() != valid {
		t.Fatal("immutable original request changed", err)
	}
	goal.Revision = 2
	if err := goal.Validate(); !errors.Is(err, ErrInvalidGoal) {
		t.Fatal("unsupported revision accepted", err)
	}
}
