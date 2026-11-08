// INPUT: context, HubService admission and the Hub Runtime execution owner
// OUTPUT: TurnService: Send, owner-bound Start, identity-bound Stop and execution observation
// POS: [L7 Hub product] Narrow execution entry for desktop, HTTP and CLI; entries never call the Runtime directly
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/integration/engine"
)

// TurnService is the execution half of the product interface. Every start
// goes through HubService admission; every Stop names the execution the
// caller observed, so a late click never cancels a replacement owner.
type TurnService struct {
	service *HubService
}

// TurnStart is the receipt of an admitted Turn. Done delivers the execution
// owner's single result; ExecutionID is the identity Stop must name.
type TurnStart struct {
	TurnID      string
	ExecutionID string
	Done        <-chan error
}

// Send admits a plain prompt with explicit draft selections. The execution
// outlives the caller's context: a UI timeout never cancels an admitted Turn.
func (t *TurnService) Send(ctx context.Context, id, prompt string, references []DraftReferenceSelection) (TurnStart, error) {
	return t.service.admitDraftWithFiles(ctx, context.Background(), id, prompt, nil, references, nil, prompt)
}

// Start admits a plain prompt whose execution is owned by owner: cancelling
// owner cancels the run. Only a process-scoped caller such as the CLI uses it.
func (t *TurnService) Start(owner context.Context, id, prompt string) (TurnStart, error) {
	if owner == nil {
		return TurnStart{}, errors.New("execution owner context required")
	}
	return t.service.admitDraftWithFiles(owner, owner, id, prompt, nil, nil, nil, prompt)
}

// Active snapshots physical execution ownership. While admission is not yet
// committed, active is true and the identity is empty.
func (t *TurnService) Active(id string) (executionID string, active bool) {
	return t.service.Runtime.ActiveExecution(id)
}

// AnyActive reports whether any session still has a physical owner, including
// final cleanup; the last window cannot close over it.
func (t *TurnService) AnyActive() bool {
	return t.service.Runtime.HasActive()
}

// Stop cancels only the execution with the observed identity.
func (t *TurnService) Stop(ctx context.Context, id, executionID string) error {
	return t.service.Runtime.CancelExecution(ctx, id, executionID)
}

// Engine returns the immutable public description of a registered engine:
// its execution and inspection capabilities and configuration surface. It
// never exposes the engine's Runtime.
func (t *TurnService) Engine(id string) (engine.Description, bool) {
	descriptor, ok := t.service.Runtime.Descriptor(id)
	if !ok {
		return engine.Description{}, false
	}
	return descriptor.Description(id), true
}

// Engines lists every registered engine's public description.
func (t *TurnService) Engines() []engine.Description {
	return t.service.Runtime.EngineDescriptions()
}
