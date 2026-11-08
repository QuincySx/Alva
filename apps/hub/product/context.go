// INPUT: context, errors, sync, hub session detail and engine capability descriptors
// OUTPUT: ContextService one-shot manual compaction requests and the HubService entries the UI uses to request, cancel and observe them
// POS: [L7 Hub product] Manual compaction intent per product session; the engine performs and reports the compaction itself.
package product

import (
	"context"
	"errors"
	"sync"

	"github.com/QuincySx/Alva/apps/hub"
)

// ContextService holds "compact before the next request" intents keyed by
// product session. They live in process memory on purpose: the intent is a
// one-shot UI choice, not a product fact. Persisting it would need a product
// schema change and could fire days later after a restart; losing it on
// restart only means no compaction happens, which the chip shows.
type ContextService struct {
	mu      sync.Mutex
	pending map[string]bool
}

func NewContextService() *ContextService { return &ContextService{pending: map[string]bool{}} }

// Pending reports whether the session's next admitted request compacts first.
func (s *ContextService) Pending(sessionID string) bool {
	if s == nil {
		return false
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.pending[sessionID]
}

// Clear drops the intent; the assembly calls it once the engine admitted the
// request that carries it.
func (s *ContextService) Clear(sessionID string) {
	if s == nil {
		return
	}
	s.mu.Lock()
	delete(s.pending, sessionID)
	s.mu.Unlock()
}

func (s *ContextService) set(sessionID string) {
	s.mu.Lock()
	s.pending[sessionID] = true
	s.mu.Unlock()
}

// ErrContextCompactionUnsupported: the session's current engine cannot compact on request.
var ErrContextCompactionUnsupported = errors.New("the current engine cannot compact its context on request")

// ContextCompactionSupported says whether the session's active branch engine
// compacts on request; the UI offers 「压缩」 only then.
func (s *HubService) ContextCompactionSupported(detail hub.Detail) bool {
	if s == nil || s.Runtime == nil || s.Context == nil {
		return false
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return false
	}
	descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
	return ok && descriptor.Runtime != nil && descriptor.Runtime.Capabilities().ContextCompaction
}

// RequestContextCompaction makes the session's next admitted request compact
// its earlier conversation first. It sends nothing by itself.
func (s *HubService) RequestContextCompaction(ctx context.Context, sessionID string) error {
	if s == nil || s.Store == nil || s.Context == nil {
		return errors.New("context compaction is unavailable")
	}
	detail, err := s.Store.Detail(ctx, sessionID)
	if err != nil {
		return err
	}
	if !s.ContextCompactionSupported(detail) {
		return ErrContextCompactionUnsupported
	}
	s.Context.set(sessionID)
	return nil
}

// CancelContextCompaction withdraws a not yet admitted request.
func (s *HubService) CancelContextCompaction(sessionID string) {
	if s != nil {
		s.Context.Clear(sessionID)
	}
}

// ContextCompactionPending reports a request not yet admitted.
func (s *HubService) ContextCompactionPending(sessionID string) bool {
	return s != nil && s.Context.Pending(sessionID)
}
