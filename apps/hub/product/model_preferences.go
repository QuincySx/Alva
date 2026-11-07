// INPUT: Explicit model preference saves and committed per-account/model snapshots.
// OUTPUT: Shared favorites/defaults with independent revision and existing connection owner shutdown.
// POS: [L7 product service] Saving metadata never changes credentials, selects a route or executes a model.
package product

import (
	"context"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *ConnectionsService) ModelPreference(target hub.Target) hub.ModelPreference {
	key := hub.ModelPreferenceKey(target)
	s.mu.RLock()
	defer s.mu.RUnlock()
	for _, p := range s.modelPrefs.Entries {
		if p.Key == key {
			return p.Clone()
		}
	}
	return hub.ModelPreference{Key: key}
}

func (s *ConnectionsService) SaveModelPreference(ctx context.Context, target hub.Target, p hub.ModelPreference) (hub.ModelPreference, error) {
	if err := target.Validate(); err != nil {
		return hub.ModelPreference{}, err
	}
	if p.Key != hub.ModelPreferenceKey(target) {
		return hub.ModelPreference{}, errors.New("model preference does not match selected account/model")
	}
	if err := p.Validate(); err != nil {
		return hub.ModelPreference{}, err
	}
	ctx, err := s.acquire(ctx)
	if err != nil {
		return hub.ModelPreference{}, err
	}
	defer s.release()
	current := s.ModelPreference(target)
	if !reflect.DeepEqual(current.Options, p.Options) {
		if err = p.Options.ValidateTarget(target); err != nil {
			return hub.ModelPreference{}, err
		}
	}
	v, err := s.store.SaveModelPreference(ctx, p)
	if err != nil {
		return hub.ModelPreference{}, connectionError("save model preference", err)
	}
	if err = v.Validate(); err != nil {
		return hub.ModelPreference{}, err
	}
	expected := p.Clone()
	expected.Revision++
	matched := false
	for _, receipt := range v.Entries {
		if receipt.Key == expected.Key {
			matched = reflect.DeepEqual(receipt, expected)
			break
		}
	}
	if !matched {
		return hub.ModelPreference{}, errors.New("model preference save returned an unverified receipt; reload before retrying")
	}
	s.mu.Lock()
	if v.Revision <= s.modelPrefs.Revision {
		s.mu.Unlock()
		return hub.ModelPreference{}, errors.New("stale model preference snapshot")
	}
	s.modelPrefs = v.Clone()
	s.mu.Unlock()
	return s.ModelPreference(target), nil
}

// ReloadModelPreferences explicitly reconciles uncertain saves or another
// application's changes. It never retries a write or replaces a UI draft.
func (s *ConnectionsService) ReloadModelPreferences(ctx context.Context) error {
	ctx, err := s.acquire(ctx)
	if err != nil {
		return err
	}
	defer s.release()
	v, err := s.store.ModelPreferences(ctx)
	if err != nil {
		return connectionError("reload model preferences", err)
	}
	if err = v.Validate(); err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if v.Revision < s.modelPrefs.Revision || v.Revision == s.modelPrefs.Revision && !reflect.DeepEqual(v, s.modelPrefs) {
		return errors.New("stale model preference snapshot")
	}
	s.modelPrefs = v.Clone()
	return nil
}
