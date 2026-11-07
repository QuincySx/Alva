// INPUT: Saved session identity, exact configured routes and the existing joined connection owner.
// OUTPUT: Cold session override reads and explicit CAS saves with verified durable receipts.
// POS: [L7 product service] Next-Attempt configuration never changes route, admitted options or execution.
package product

import (
	"context"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *HubService) SessionModelOptions(ctx context.Context, id string) (hub.SessionModelOptionsSnapshot, error) {
	if s.Connections == nil {
		return hub.SessionModelOptionsSnapshot{}, errors.New("model configuration owner unavailable")
	}
	ctx, err := s.Connections.acquire(ctx)
	if err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	defer s.Connections.release()
	v, err := s.Store.SessionModelOptions(ctx, id)
	if err != nil {
		return v, err
	}
	return v.Clone(), v.Validate()
}

func (s *HubService) SaveSessionModelOptions(ctx context.Context, edit hub.SessionModelOptionsUpdate) (hub.SessionModelOptionsSnapshot, error) {
	if s.Connections == nil {
		return hub.SessionModelOptionsSnapshot{}, errors.New("model configuration owner unavailable")
	}
	ctx, err := s.Connections.acquire(ctx)
	if err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	defer s.Connections.release()
	s.mu.Lock()
	defer s.mu.Unlock()
	if err = edit.Target.Validate(); err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	available := false
	for _, target := range s.AvailableTargets() {
		available = available || hub.RouteKey(target) == hub.RouteKey(edit.Target)
	}
	if !available {
		return hub.SessionModelOptionsSnapshot{}, errors.New("model route changed; reopen configuration")
	}
	descriptor, declared := s.Runtime.Descriptor(edit.Target.EngineID)
	if !declared {
		return hub.SessionModelOptionsSnapshot{}, engine.ErrUnsupported
	}
	if edit.Options != nil {
		options := edit.Options.Clone()
		edit.Options = &options
		if err = options.ValidateTarget(edit.Target); err != nil {
			return hub.SessionModelOptionsSnapshot{}, err
		}
		if !options.Generation.Empty() && !descriptor.Config.SupportsGeneration || options.SystemPrompt != "" && !descriptor.Config.SupportsSystemPrompt || options.MaxTurns != 0 && !descriptor.Config.SupportsMaxTurns {
			return hub.SessionModelOptionsSnapshot{}, engine.ErrUnsupported
		}
	}
	before, err := s.Store.SessionModelOptions(ctx, edit.SessionID)
	if err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	if err = before.Validate(); err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	if before.Revision != edit.ExpectedRevision {
		return hub.SessionModelOptionsSnapshot{}, hub.ErrSessionModelOptionsConflict
	}
	value, err := s.Store.SaveSessionModelOptions(ctx, edit)
	if err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	if err = value.Validate(); err != nil {
		return hub.SessionModelOptionsSnapshot{}, err
	}
	want := before.Clone()
	want.Revision++
	key := hub.ModelPreferenceKey(edit.Target)
	want.Entries = make([]hub.SessionModelOption, 0, len(before.Entries)+1)
	for _, entry := range before.Entries {
		if entry.Key != key {
			want.Entries = append(want.Entries, entry)
		}
	}
	if edit.Options != nil {
		want.Entries = append(want.Entries, hub.SessionModelOption{Key: key, Options: edit.Options.Clone()})
	}
	if !reflect.DeepEqual(value, want) {
		return hub.SessionModelOptionsSnapshot{}, errors.New("session model options save returned an unverified receipt; reload before retrying")
	}
	return value.Clone(), nil
}
