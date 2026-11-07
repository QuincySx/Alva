// INPUT: Real shared preferences, two logical observers and native paint-only loading controls.
// OUTPUT: Explicit durable application motion preference, cross-window projection and stopped-frame regressions.
// POS: [L7 UI acceptance] Application reduction augments system preferences without mutable global UI state.
//go:build desktop

package hubdesktop

import (
	"crypto/sha256"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestNativeGeneralReducedMotionCommitsAndStopsLoaderPaints(t *testing.T) {
	s := preferenceFixture(t)
	p, other := NewNativePage(s), NewNativePage(s)
	tester, _ := preferenceTester(p)
	otherTester, _ := preferenceTester(other)
	openPreferencePage(t, p, tester)
	loader := ui.NewTester(func(c *ui.Context) {
		p.applyPreferences(c)
		p.workspaceControlAction(c, "busy", "Working", "", "primary", true, func() { t.Error("busy action ran") })
	}, 160, 60)
	before := sha256.Sum256(loader.Image().Pix)
	loader.Advance(210 * time.Millisecond)
	if before == sha256.Sum256(loader.Image().Pix) {
		t.Fatal("default loading paint did not animate")
	}
	if err := tester.Click("Reduce application motion"); err != nil {
		t.Fatal(err)
	}
	if s.Preferences.Snapshot().ReducedMotion || !p.preferences.dirty() {
		t.Fatal("editing motion bypassed explicit Save")
	}
	if err := tester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	otherTester.Frame()
	stored, err := s.Store.(hub.PreferencesStore).Preferences(t.Context())
	if err != nil || !stored.ReducedMotion || !other.preferences.base.ReducedMotion {
		t.Fatal("motion preference was not durably shared", stored, err)
	}
	loader.Frame()
	before = sha256.Sum256(loader.Image().Pix)
	loader.Advance(210 * time.Millisecond)
	if before != sha256.Sum256(loader.Image().Pix) || loader.FramePending() {
		t.Fatal("committed application reduction kept requesting animation frames")
	}
}
