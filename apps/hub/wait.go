// INPUT: Product session/branch/Turn identities, durable revision and engine checkpoint metadata.
// OUTPUT: WaitStore, explicit same-Turn continuation and input-blocking policy.
// POS: [L7 Hub product] Optional waiting persistence port; the ordinary ExecutionStore stays six methods.
package hub

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/integration/engine"
)

type WaitStore interface {
	SetDurableWaits(context.Context, string, bool) error
	BranchTurn(context.Context, string, string) (Turn, error)
	BeginWaitResume(context.Context, string, string, string, uint64) (Claim, error)
	AbandonWaitingTurn(context.Context, string, string, string) error
}

// BlocksInput protects a suspended Turn's route/settings until explicit resume or abandonment.
func BlocksInput(status engine.Outcome) bool { return status == Running || status == engine.Waiting }
func NewWithWaits(store interface {
	ExecutionStore
	WaitStore
}) *Runtime {
	r := New(store)
	r.waits = store
	return r
}
func (r *Runtime) ResumeWait(ctx context.Context, id, branch, turn string, revision uint64) (string, <-chan error, error) {
	if r.waits == nil || revision == 0 {
		return "", nil, errors.New("durable continuation store and reviewed revision required")
	}
	return r.start(ctx, id, func(ctx context.Context) (Claim, error) {
		return r.waits.BeginWaitResume(ctx, id, branch, turn, revision)
	})
}
