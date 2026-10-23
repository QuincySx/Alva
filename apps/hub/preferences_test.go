// INPUT: Public preference choices and independent consumer edits.
// OUTPUT: Option ownership and application-wide appearance policy regressions.
// POS: [L7 Hub product] Consumers cannot change shared validation by editing their option lists.
package hub_test

import (
	"errors"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestPreferenceChoicesBelongToConsumer(t *testing.T) {
	choices := hub.PreferenceChoices()
	choices.Themes[0] = "consumer-theme"
	choices.Densities[0] = "consumer-density"
	choices.FontSizes[0] = 99
	defaults := hub.DefaultPreferences()
	if err := defaults.Validate(); err != nil {
		t.Fatalf("editing choices changed default policy: %v", err)
	}
	_, err := defaults.Apply(hub.PreferenceUpdate{
		ExpectedRevision: defaults.Revision,
		Theme:            &choices.Themes[0],
	})
	if !errors.Is(err, hub.ErrInvalidPreferences) {
		t.Fatalf("consumer option unexpectedly became valid: %v", err)
	}
}
