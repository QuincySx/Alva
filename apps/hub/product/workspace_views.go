// INPUT: Explicit workspace view store, caller cancellation and owner lifecycle.
// OUTPUT: WorkspaceViewsService with tracked durable view reads/saves and cancel/join Close.
// POS: [L7 Hub product] Owns observation I/O through actual completion without owning or stopping execution.
package product

import (
	"context"
	"errors"
	"reflect"
	"sync"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/storage"
)

type WorkspaceViewsService struct {
	store  hub.WorkspaceViewStore
	mu     sync.Mutex
	closed bool
	next   uint64
	active map[uint64]context.CancelFunc
	joined chan struct{}
}

func NewWorkspaceViewsService(store hub.WorkspaceViewStore) (*WorkspaceViewsService, error) {
	if storage.Missing(store) {
		return nil, errors.New("workspace view store is required")
	}
	return &WorkspaceViewsService{store: store, active: make(map[uint64]context.CancelFunc), joined: make(chan struct{})}, nil
}

func (s *WorkspaceViewsService) begin(ctx context.Context) (context.Context, func(), error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil, nil, errors.New("workspace view owner is closed")
	}
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	ctx, cancel := context.WithCancel(ctx)
	s.next++
	id := s.next
	s.active[id] = cancel
	return ctx, func() {
		cancel()
		s.mu.Lock()
		defer s.mu.Unlock()
		delete(s.active, id)
		if s.closed && len(s.active) == 0 {
			select {
			case <-s.joined:
			default:
				close(s.joined)
			}
		}
	}, nil
}

func (s *WorkspaceViewsService) Read(ctx context.Context, id string) (hub.WorkspaceView, error) {
	ctx, end, err := s.begin(ctx)
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	defer end()
	value, err := s.store.WorkspaceView(ctx, id)
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	if err = ctx.Err(); err != nil {
		return hub.WorkspaceView{}, err
	}
	if value.SessionID != id || value.Validate() != nil {
		return hub.WorkspaceView{}, errors.New("workspace view store returned an invalid or foreign read receipt")
	}
	return hub.CloneWorkspaceView(value), nil
}

func (s *WorkspaceViewsService) Save(ctx context.Context, view hub.WorkspaceView, expected uint64) (hub.WorkspaceView, error) {
	view = hub.CloneWorkspaceView(view)
	if expected >= 1<<63-1 {
		return hub.WorkspaceView{}, hub.ErrWorkspaceViewConflict
	}
	if err := view.Validate(); err != nil {
		return hub.WorkspaceView{}, err
	}
	ctx, end, err := s.begin(ctx)
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	defer end()
	value, err := s.store.SaveWorkspaceView(ctx, hub.CloneWorkspaceView(view), expected)
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	if err = ctx.Err(); err != nil {
		return hub.WorkspaceView{}, err
	}
	view.Revision = expected + 1
	if value.Validate() != nil || !reflect.DeepEqual(value, view) {
		return hub.WorkspaceView{}, errors.New("workspace view save returned an invalid receipt; reload committed positions before saving again")
	}
	return hub.CloneWorkspaceView(value), nil
}

func (s *WorkspaceViewsService) Close(ctx context.Context) error {
	s.mu.Lock()
	s.closed = true
	for _, cancel := range s.active {
		cancel()
	}
	if len(s.active) == 0 {
		select {
		case <-s.joined:
		default:
			close(s.joined)
		}
	}
	joined := s.joined
	s.mu.Unlock()
	select {
	case <-joined:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
