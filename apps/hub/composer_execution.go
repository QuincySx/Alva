// INPUT: Explicit product composer text, captured engine input files and actual execution admission.
// OUTPUT: ComposerTurnInput, optional atomic composer admission and Runtime.StartComposerOnBranch.
// POS: [L7 Hub product] Adds typed file admission without widening Runtime's six-method execution store.
package hub

import (
	"context"
	"github.com/QuincySx/Alva/integration/engine"
)

type ComposerTurnInput struct {
	Prompt   string
	UserText string
	Files    []engine.InputFile
	Skills   []engine.InputSkill
}

type ComposerExecutionStore interface {
	BeginComposerTurn(context.Context, string, string, ComposerTurnInput) (Claim, error)
}

func (r *Runtime) StartComposerOnBranch(owner context.Context, id, branch string, input ComposerTurnInput) (string, <-chan error, error) {
	if err := engine.ValidateInputFiles(input.Files); err != nil {
		return "", nil, err
	}
	if err := engine.ValidateInputSkills(input.Skills); err != nil {
		return "", nil, err
	}
	store, ok := r.store.(ComposerExecutionStore)
	if !ok {
		return "", nil, engine.ErrUnsupported
	}
	input.Files = engine.CloneInputFiles(input.Files)
	input.Skills = engine.CloneInputSkills(input.Skills)
	return r.start(owner, id, func(ctx context.Context) (Claim, error) { return store.BeginComposerTurn(ctx, id, branch, input) })
}
