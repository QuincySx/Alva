// INPUT: Explicit product composer text, captured engine input files and actual execution admission.
// OUTPUT: ComposerTurnInput, optional atomic composer admission and Runtime.StartComposerOnBranch.
// POS: [L7 Hub product] Adds typed file admission without widening Runtime's six-method execution store.
package hub

import (
	"context"
	"github.com/QuincySx/Alva/integration/engine"
)

// TurnOriginContinue marks a Turn started by the product's Continue action.
const TurnOriginContinue = "continue"

type ComposerTurnInput struct {
	// Origin is TurnOriginContinue or empty. A continue Turn carries no user
	// text: the message stream shows a status block instead of a user message.
	Origin   string
	Prompt   string
	UserText string
	Files    []engine.InputFile
	Skills   []engine.InputSkill
}

type ComposerExecutionStore interface {
	BeginComposerTurn(context.Context, string, string, ComposerTurnInput) (Claim, error)
}

func (r *Runtime) StartComposerOnBranch(owner context.Context, id, branch string, input ComposerTurnInput) (string, <-chan error, error) {
	turn, _, done, err := r.StartComposerOnBranchOwned(owner, id, branch, input)
	return turn, done, err
}

// StartComposerOnBranchOwned also returns the execution identity Stop must name.
func (r *Runtime) StartComposerOnBranchOwned(owner context.Context, id, branch string, input ComposerTurnInput) (string, string, <-chan error, error) {
	if err := engine.ValidateInputFiles(input.Files); err != nil {
		return "", "", nil, err
	}
	if err := engine.ValidateInputSkills(input.Skills); err != nil {
		return "", "", nil, err
	}
	store, ok := r.store.(ComposerExecutionStore)
	if !ok {
		return "", "", nil, engine.ErrUnsupported
	}
	input.Files = engine.CloneInputFiles(input.Files)
	input.Skills = engine.CloneInputSkills(input.Skills)
	return r.startOwned(owner, id, func(ctx context.Context) (Claim, error) { return store.BeginComposerTurn(ctx, id, branch, input) })
}
