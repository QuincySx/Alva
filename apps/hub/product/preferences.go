// INPUT: Explicit application preference storage and shared Hub preference values.
// OUTPUT: PreferencesService with shared committed snapshots and joined storage shutdown.
// POS: [L7 product service] Owns serial preference I/O without executing sessions or selecting a database.
package product

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
)

type PreferencesService struct {
	access  chan struct{}
	store   hub.PreferencesStore
	current atomic.Pointer[hub.Preferences]
	mu      sync.Mutex
	closed  bool
	stopped chan struct{}
	cancel  context.CancelFunc
	done    chan struct{}
}

// NewPreferencesService loads the selected backend before exposing any snapshot.
func NewPreferencesService(ctx context.Context, store hub.PreferencesStore) (*PreferencesService, error) {
	if storage.Missing(store) {
		return nil, errors.New("application preference storage required")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	value, err := store.Preferences(ctx)
	if err != nil {
		return nil, err
	}
	if err = value.Validate(); err != nil {
		return nil, errors.New("preference store returned an invalid snapshot")
	}
	s := &PreferencesService{store: store, access: make(chan struct{}, 1), stopped: make(chan struct{})}
	s.access <- struct{}{}
	s.current.Store(&value)
	return s, nil
}

// Snapshot is the last committed observation, not a fallback for a failed read.
// Build/poll may read it without performing SQL or changing a caller's draft.
func (s *PreferencesService) Snapshot() hub.Preferences {
	return *s.current.Load()
}

// Read refreshes the committed observation without allowing an older read to
// overwrite an update. Failed reads leave the last committed snapshot intact.
func (s *PreferencesService) Read(ctx context.Context) (hub.Preferences, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.Preferences{}, err
	}
	defer s.release()
	value, err := s.store.Preferences(ctx)
	if err != nil {
		return hub.Preferences{}, err
	}
	if err = s.publish(value); err != nil {
		return hub.Preferences{}, err
	}
	return value, nil
}

// Update publishes only a successfully committed and validated backend result.
func (s *PreferencesService) Update(ctx context.Context, update hub.PreferenceUpdate) (hub.Preferences, error) {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.Preferences{}, err
	}
	defer s.release()
	value, err := s.store.UpdatePreferences(ctx, update)
	if err != nil {
		return hub.Preferences{}, err
	}
	if err = s.publish(value); err != nil {
		return hub.Preferences{}, err
	}
	return value, nil
}

// Close stops admission, cancels the admitted storage call and joins its actual
// return. A timeout retains the call for a later Close; a late committed success
// still publishes its actual result. Snapshot remains available after closing.
func (s *PreferencesService) Close(ctx context.Context) error {
	s.mu.Lock()
	if !s.closed {
		s.closed = true
		close(s.stopped)
	}
	cancel, done := s.cancel, s.done
	s.mu.Unlock()
	if cancel == nil {
		return nil
	}
	cancel()
	select {
	case <-done:
		return nil
	default:
	}
	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (s *PreferencesService) acquire(ctx context.Context) (context.Context, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-s.stopped:
		return nil, storage.ErrClosed
	case <-s.access:
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		s.access <- struct{}{}
		return nil, storage.ErrClosed
	}
	if err := ctx.Err(); err != nil {
		s.access <- struct{}{}
		return nil, err
	}
	owned, cancel := context.WithCancel(ctx)
	s.cancel = cancel
	s.done = make(chan struct{})
	return owned, nil
}

func (s *PreferencesService) release() {
	s.mu.Lock()
	s.cancel()
	s.cancel = nil
	close(s.done)
	s.done = nil
	s.mu.Unlock()
	s.access <- struct{}{}
}

func (s *PreferencesService) publish(value hub.Preferences) error {
	if err := value.Validate(); err != nil {
		return errors.New("preference store returned an invalid snapshot")
	}
	current := s.Snapshot()
	if value.Revision < current.Revision || value.Revision == current.Revision && value != current {
		return errors.New("preference store returned a stale snapshot")
	}
	s.current.Store(&value)
	return nil
}
