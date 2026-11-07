// INPUT: Explicit session/account/model overrides and existing model option validation.
// OUTPUT: Bounded session override snapshots, CAS edits and a shared storage capability.
// POS: [L7 Hub product] Overrides affect future Attempts; admitted execution recipes remain immutable.
package hub

import (
	"context"
	"errors"
)

var ErrSessionModelOptionsConflict = errors.New("session model options changed; reload before saving")

type SessionModelOption struct {
	Key     string       `json:"key"`
	Options ModelOptions `json:"options"`
}

type SessionModelOptionsSnapshot struct {
	Revision uint64               `json:"revision"`
	Entries  []SessionModelOption `json:"entries,omitempty"`
}

func (v SessionModelOptionsSnapshot) Clone() SessionModelOptionsSnapshot {
	v.Entries = append([]SessionModelOption(nil), v.Entries...)
	for i := range v.Entries {
		v.Entries[i].Options = v.Entries[i].Options.Clone()
	}
	return v
}

func (v SessionModelOptionsSnapshot) Validate() error {
	if len(v.Entries) > 64 || v.Revision == 0 && len(v.Entries) != 0 {
		return errors.New("invalid session model options snapshot")
	}
	seen := map[string]bool{}
	for _, entry := range v.Entries {
		if err := (ModelPreference{Key: entry.Key, Options: entry.Options}).Validate(); err != nil {
			return err
		}
		if seen[entry.Key] {
			return errors.New("duplicate session model override")
		}
		seen[entry.Key] = true
	}
	return nil
}

func (v SessionModelOptionsSnapshot) Options(target Target) (ModelOptions, bool) {
	key := ModelPreferenceKey(target)
	for _, entry := range v.Entries {
		if entry.Key == key {
			return entry.Options.Clone(), true
		}
	}
	return ModelOptions{}, false
}

type SessionModelOptionsUpdate struct {
	SessionID        string `json:"session_id"`
	BranchID         string `json:"branch_id"`
	Target           Target `json:"target"`
	ExpectedRevision uint64 `json:"expected_revision"`
	// A nil Options clears the override; an empty value explicitly uses Agent defaults.
	Options *ModelOptions `json:"options,omitempty"`
}

type SessionModelOptionsStore interface {
	SessionModelOptions(context.Context, string) (SessionModelOptionsSnapshot, error)
	SaveSessionModelOptions(context.Context, SessionModelOptionsUpdate) (SessionModelOptionsSnapshot, error)
}
