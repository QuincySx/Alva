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

func TestPreferenceSidebarSparseUpdatesPreserveIndependentAppearance(t *testing.T) {
	value := hub.DefaultPreferences()
	value.Theme, value.FontSize, value.ReducedMotion = "dark", 16, true
	width, collapsed := 348, true
	saved, err := value.Apply(hub.PreferenceUpdate{ExpectedRevision: value.Revision, SidebarWidth: &width, SidebarCollapsed: &collapsed})
	if err != nil || saved.SidebarWidth != width || !saved.SidebarCollapsed || saved.Theme != value.Theme || saved.FontSize != value.FontSize || !saved.ReducedMotion {
		t.Fatal("layout update changed independent appearance", saved, err)
	}
	theme := "light"
	appearance, err := saved.Apply(hub.PreferenceUpdate{ExpectedRevision: saved.Revision, Theme: &theme})
	if err != nil || appearance.Theme != theme || appearance.SidebarWidth != width || !appearance.SidebarCollapsed {
		t.Fatal("appearance update replaced layout", appearance, err)
	}
	width, collapsed = 0, false
	reset, err := appearance.Apply(hub.PreferenceUpdate{ExpectedRevision: appearance.Revision, SidebarWidth: &width, SidebarCollapsed: &collapsed})
	if err != nil || reset.SidebarWidth != 0 || reset.SidebarCollapsed || reset.Theme != theme || reset.FontSize != 16 || !reset.ReducedMotion {
		t.Fatal("explicit default width/expanded state was treated as absent", reset, err)
	}
}

func TestPreferenceSidebarUsesSharedBoundedPolicy(t *testing.T) {
	initial := hub.DefaultPreferences()
	for _, width := range []int{0, hub.MinSidebarWidth, 348, hub.MaxSidebarWidth} {
		if _, err := initial.Apply(hub.PreferenceUpdate{ExpectedRevision: initial.Revision, SidebarWidth: &width}); err != nil {
			t.Fatal("valid width rejected", width, err)
		}
	}
	for _, width := range []int{-1, 1, hub.MinSidebarWidth - 1, hub.MaxSidebarWidth + 1} {
		actual, err := initial.Apply(hub.PreferenceUpdate{ExpectedRevision: initial.Revision, SidebarWidth: &width})
		if !errors.Is(err, hub.ErrInvalidPreferences) || actual != (hub.Preferences{}) {
			t.Fatal("invalid width escaped the shared policy", width, actual, err)
		}
	}
}
