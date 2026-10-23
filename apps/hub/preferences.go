// INPUT: context and pure application appearance values and optimistic revisions.
// OUTPUT: Preferences, PreferenceUpdate and a small application-scoped persistence port.
// POS: [L7 Hub product] One preference policy independent of UI, SQLite and session execution.
package hub

import (
	"context"
	"errors"
	"fmt"
	"math"
	"slices"
)

var ErrPreferenceConflict = errors.New("preferences changed; reload before saving")
var ErrInvalidPreferences = errors.New("invalid application preferences")

// Preferences is the committed application appearance, independent of sessions.
// Revision is a positive SQLite-compatible optimistic concurrency token.
type Preferences struct {
	Revision uint64 `json:"revision"`
	Theme    string `json:"theme"`
	Density  string `json:"density"`
	FontSize int    `json:"font_size"`
}

// PreferenceUpdate changes only supplied fields at the expected revision.
// Nil fields are unchanged; an update must supply at least one field.
type PreferenceUpdate struct {
	ExpectedRevision uint64  `json:"expected_revision"`
	Theme            *string `json:"theme,omitempty"`
	Density          *string `json:"density,omitempty"`
	FontSize         *int    `json:"font_size,omitempty"`
}

type PreferenceOptions struct {
	Themes    []string
	Densities []string
	FontSizes []int
}

// PreferenceChoices returns fresh option slices for all consumers.
func PreferenceChoices() PreferenceOptions {
	return PreferenceOptions{Themes: []string{"system", "light", "dark"}, Densities: []string{"comfortable", "compact"}, FontSizes: []int{14, 16}}
}

// DefaultPreferences is used only when initializing a new preference singleton.
func DefaultPreferences() Preferences {
	return Preferences{Revision: 1, Theme: "system", Density: "comfortable", FontSize: 14}
}

func (p Preferences) Validate() error {
	options := PreferenceChoices()
	if p.Revision == 0 || p.Revision > math.MaxInt64 {
		return fmt.Errorf("%w: revision", ErrInvalidPreferences)
	}
	if !slices.Contains(options.Themes, p.Theme) {
		return fmt.Errorf("%w: theme", ErrInvalidPreferences)
	}
	if !slices.Contains(options.Densities, p.Density) {
		return fmt.Errorf("%w: density", ErrInvalidPreferences)
	}
	if !slices.Contains(options.FontSizes, p.FontSize) {
		return fmt.Errorf("%w: body size must be 14 or 16", ErrInvalidPreferences)
	}
	return nil
}

// Apply is the sole merge/validation policy. Adapters call it inside their
// atomic compare-and-swap; it never changes execution or account settings.
func (p Preferences) Apply(update PreferenceUpdate) (Preferences, error) {
	if err := p.Validate(); err != nil {
		return Preferences{}, err
	}
	if update.ExpectedRevision == 0 || update.Theme == nil && update.Density == nil && update.FontSize == nil {
		return Preferences{}, fmt.Errorf("%w: expected revision and at least one change required", ErrInvalidPreferences)
	}
	if update.ExpectedRevision != p.Revision {
		return Preferences{}, ErrPreferenceConflict
	}
	if p.Revision == math.MaxInt64 {
		return Preferences{}, fmt.Errorf("%w: revision exhausted", ErrInvalidPreferences)
	}
	if update.Theme != nil {
		p.Theme = *update.Theme
	}
	if update.Density != nil {
		p.Density = *update.Density
	}
	if update.FontSize != nil {
		p.FontSize = *update.FontSize
	}
	p.Revision++
	if err := p.Validate(); err != nil {
		return Preferences{}, err
	}
	return p, nil
}

// PreferencesStore must return a validated durable singleton. Update commits
// all changes at the expected revision or none, and never resets on read error.
type PreferencesStore interface {
	Preferences(context.Context) (Preferences, error)
	UpdatePreferences(context.Context, PreferenceUpdate) (Preferences, error)
}
