// INPUT: Pinned MyGo headless controls, shared preference owner and real Hub SQLite/bootstrap storage.
// OUTPUT: Session-free preferences, explicit durable Save/Reload, cross-window appearance and failure-safe drafts.
// POS: [L7 Hub UI testing] Exercises actual controls without opening OS windows or executing models.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"math"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func preferenceTester(p *NativePage) (*ui.Tester, *ui.Theme) {
	theme := new(ui.Theme)
	tester := ui.NewTester(func(c *ui.Context) { c.DisableAnimations(); p.Build(c); *theme = *c.Theme() }, 1240, 850)
	return tester, theme
}

func settlePreferences(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !p.preferences.loading && !p.preferences.saving {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("preference storage did not settle")
}

func choosePreference(t *testing.T, tester *ui.Tester, label, value string) {
	t.Helper()
	if err := tester.Click("Select " + label); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click(preferenceValueLabel(value)); err != nil {
		t.Fatal(err)
	}
}

func openPreferencePage(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	if err := tester.Click("General settings"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
}

func preferenceFixture(t *testing.T) *product.HubService {
	t.Helper()
	s, _ := fixture(t)
	store, ok := s.Store.(hub.PreferencesStore)
	if !ok {
		t.Fatal("fixture store does not implement preference contract")
	}
	var err error
	s.Preferences, err = product.NewPreferencesService(t.Context(), store)
	if err != nil {
		t.Fatal(err)
	}
	return s
}

func TestNativeGeneralPreferencesExplicitSaveSharedAppearanceAndReopen(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	open := func() *bootstrap.Application {
		a, err := bootstrap.Open(t.Context(), bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: directory, Demo: true})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := a.Close(context.Background()); err != nil {
				t.Error(err)
			}
		})
		return a
	}
	a := open()
	p, other := NewNativePage(a.Service), NewNativePage(a.Service)
	tester, theme := preferenceTester(p)
	otherTester, otherTheme := preferenceTester(other)
	openPreferencePage(t, p, tester)
	if p.selected != "" || !tester.HasText("通用") || !tester.HasText("外观") || tester.HasText("Select Language") || tester.HasText("Select Motion") {
		t.Fatal("General preferences depend on a session or advertise unsupported controls")
	}
	for _, label := range []string{"Color theme", "Body size"} {
		r, ok := tester.Find("Select " + label)
		if !ok || r.X < 0 || r.Y < 0 || r.X+r.W > 1240 || r.Y+r.H > 850 {
			t.Fatalf("%s is outside the native viewport: %+v", label, r)
		}
	}
	labelBefore, _ := tester.Find("Color theme")
	before := a.Service.Preferences.Snapshot()
	choosePreference(t, tester, "Color theme", "dark")
	choosePreference(t, tester, "Body size", "16")
	stored, err := a.Store.Preferences(t.Context())
	if err != nil || stored != before || a.Service.Preferences.Snapshot() != before || theme.Dark || theme.FontSize != 14 || theme.Spacing != 4 {
		t.Fatal("editing changed committed appearance or storage", stored, err, *theme)
	}
	if err = tester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	otherTester.Frame()
	want := hub.Preferences{Revision: before.Revision + 1, Theme: "dark", Density: before.Density, FontSize: 16}
	stored, err = a.Store.Preferences(t.Context())
	labelAfter, _ := tester.Find("Color theme")
	if err != nil || stored != want || p.preferences.base != want || other.preferences.base != want || !theme.Dark || !otherTheme.Dark || theme.FontSize != 16 || otherTheme.FontSize != 16 || theme.Spacing != 4 || otherTheme.Spacing != 4 || labelAfter.H != labelBefore.H || labelAfter.H != 19.5 {
		t.Fatal("saved reading theme or fixed normal UI label scale did not reach both observers", stored, err, *theme, *otherTheme, labelBefore, labelAfter)
	}
	tester.SetPreferences(ui.Preferences{ReduceMotion: true, TextScale: 1.5})
	if theme.FontSize != 24 {
		t.Fatal("system accessibility preferences were lost", *theme)
	}
	sessions, err := a.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || a.Runtime.HasActive() {
		t.Fatal("preferences created or executed a session", sessions, err)
	}
	if err = a.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	reopened := open()
	if reopened.Service.Preferences.Snapshot() != want {
		t.Fatal("saved preferences did not survive application reopen", reopened.Service.Preferences.Snapshot())
	}
	newPage := NewNativePage(reopened.Service)
	newTester, newTheme := preferenceTester(newPage)
	if !newTheme.Dark || newTheme.FontSize != 16 || newTheme.Spacing != 4 {
		t.Fatal("new window ignored durable appearance", *newTheme)
	}
	openPreferencePage(t, newPage, newTester)
	choosePreference(t, newTester, "Color theme", "system")
	if err = newTester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, newPage, newTester)
	newTester.SetDark(false)
	if newTheme.Dark {
		t.Fatal("system theme stayed dark after system switched light")
	}
	newTester.SetDark(true)
	if !newTheme.Dark {
		t.Fatal("system theme ignored dark appearance")
	}
}

func TestNativeGeneralPreferencesCanonicalRowsKeepSelectKeyboardAndDraftValues(t *testing.T) {
	s := preferenceFixture(t)
	p := NewNativePage(s)
	tester, _ := preferenceTester(p)
	openPreferencePage(t, p, tester)
	for _, text := range []string{"通用", "外观", "Color theme", "Body size", "跟随系统", "14 px", "Reduce application motion", "Save preferences", "Reload preferences"} {
		if !tester.HasText(text) {
			t.Fatalf("canonical preference copy is missing: %s", text)
		}
	}
	title, _ := tester.Find("General page title")
	card, _ := tester.Find("Preferences appearance")
	save, _ := tester.Find("Save preferences")
	if card.Y < title.Y+title.H+16 || save.Y < card.Y+card.H || save.H != 32 {
		t.Fatal("page title, appearance card and explicit save are out of order", title, card, save)
	}
	for _, label := range []string{"Color theme", "Body size"} {
		name, okName := tester.Find(label)
		trigger, okTrigger := tester.Find("Select " + label)
		if !okName || !okTrigger || trigger.H != 32 || name.X < card.X || trigger.X+trigger.W > card.X+card.W || math.Abs(float64(name.Y+name.H/2-trigger.Y-trigger.H/2)) > 1 {
			t.Fatalf("flat setting row %s lost its shared bounds, 36px trigger or vertical centering: %+v %+v", label, name, trigger)
		}
	}
	before := s.Preferences.Snapshot()
	if err := tester.Click("Select Color theme"); err != nil {
		t.Fatal(err)
	}
	tester.Key(0, ui.KeyEnd)
	tester.Key(0, ui.KeyEnter)
	if p.preferences.theme != "dark" || !tester.HasText("深色") || s.Preferences.Snapshot() != before {
		t.Fatal("translated keyboard choice did not retain raw draft value and explicit save", p.preferences)
	}
	if err := tester.Click("Select Color theme"); err != nil {
		t.Fatal(err)
	}
	tester.Key(0, ui.KeyHome)
	tester.Key(0, ui.KeyEscape)
	if p.preferences.theme != "dark" || s.Preferences.Snapshot() != before {
		t.Fatal("closing the native select applied the highlighted choice", p.preferences)
	}
}

func TestNativeGeneralUnimplementedDensityAndLanguageAreHidden(t *testing.T) {
	s := preferenceFixture(t)
	p := NewNativePage(s)
	tester, _ := preferenceTester(p)
	openPreferencePage(t, p, tester)
	before := s.Preferences.Snapshot()
	for _, label := range []string{"Information density", "Select Information density", "Select Language", "语言切换暂不可用"} {
		if tester.HasText(label) {
			t.Fatal("fixed preference advertised as a working setting", label)
		}
	}
	if p.preferences.density != before.Density || p.preferences.dirty() {
		t.Fatal("hiding an unavailable control mutated its saved value")
	}
}

func TestNativeGeneralPreferencesRetainsDraftAcrossWindowsConflictAndReload(t *testing.T) {
	s := preferenceFixture(t)
	first, second := NewNativePage(s), NewNativePage(s)
	firstTester, firstTheme := preferenceTester(first)
	secondTester, _ := preferenceTester(second)
	openPreferencePage(t, first, firstTester)
	openPreferencePage(t, second, secondTester)
	base := first.preferences.base
	choosePreference(t, firstTester, "Color theme", "dark")
	if err := firstTester.Click("Back to workspace"); err != nil {
		t.Fatal(err)
	}
	openPreferencePage(t, first, firstTester)
	if first.preferences.theme != "dark" || first.preferences.base != base {
		t.Fatal("navigation discarded a preference draft")
	}
	choosePreference(t, secondTester, "Body size", "16")
	if err := secondTester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, second, secondTester)
	firstTester.Frame()
	if first.preferences.theme != "dark" || first.preferences.base != base || !first.preferences.dirty() || firstTheme.Dark || firstTheme.Spacing != 4 || !firstTester.HasText("Your draft is retained") {
		t.Fatal("external save overwrote draft or draft changed committed theme", first.preferences, *firstTheme)
	}
	if err := firstTester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, first, firstTester)
	if first.preferences.theme != "dark" || first.preferences.base != base || !firstTester.HasText("Your draft is retained") || s.Preferences.Snapshot().Revision != base.Revision+1 {
		t.Fatal("conflict lost draft or committed stale values", first.preferences)
	}
	if err := firstTester.Click("Reload preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, first, firstTester)
	if first.preferences.dirty() || first.preferences.base != s.Preferences.Snapshot() || first.preferences.theme != "system" || first.preferences.size != "16" || first.preferences.problem != "" {
		t.Fatal("explicit reload did not replace draft with latest saved values", first.preferences)
	}
}

type preferenceFaultStore struct {
	hub.PreferencesStore
	readFailure atomic.Bool
	saveFailure atomic.Int32
	reads       atomic.Int32
	writes      atomic.Int32
	entered     chan struct{}
	release     <-chan struct{}
}

func (s *preferenceFaultStore) Preferences(ctx context.Context) (hub.Preferences, error) {
	s.reads.Add(1)
	if s.readFailure.Load() {
		return hub.Preferences{}, errors.New("fixture read failed")
	}
	return s.PreferencesStore.Preferences(ctx)
}

func (s *preferenceFaultStore) UpdatePreferences(ctx context.Context, u hub.PreferenceUpdate) (hub.Preferences, error) {
	s.writes.Add(1)
	if s.entered != nil {
		s.entered <- struct{}{}
	}
	if s.release != nil {
		select {
		case <-s.release:
		case <-ctx.Done():
			return hub.Preferences{}, ctx.Err()
		}
	}
	switch s.saveFailure.Load() {
	case 1:
		return hub.Preferences{}, errors.New("fixture save failed")
	case 2:
		return hub.Preferences{}, context.Canceled
	}
	return s.PreferencesStore.UpdatePreferences(ctx, u)
}

func installPreferenceFaultStore(t *testing.T, s *product.HubService, store *preferenceFaultStore) {
	t.Helper()
	var err error
	s.Preferences, err = product.NewPreferencesService(t.Context(), store)
	if err != nil {
		t.Fatal(err)
	}
}

func TestNativeGeneralPreferencesReadSaveAndCanceledStorageKeepDraftAndSavedValues(t *testing.T) {
	s := preferenceFixture(t)
	store := &preferenceFaultStore{PreferencesStore: s.Store.(hub.PreferencesStore)}
	installPreferenceFaultStore(t, s, store)
	p := NewNativePage(s)
	tester, _ := preferenceTester(p)
	openPreferencePage(t, p, tester)
	before := s.Preferences.Snapshot()
	choosePreference(t, tester, "Color theme", "dark")
	store.readFailure.Store(true)
	if err := tester.Click("Reload preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	if !p.preferences.readFailed || p.preferences.theme != "dark" || p.preferences.base != before || s.Preferences.Snapshot() != before {
		t.Fatal("failed read lost saved values or the draft", p.preferences)
	}
	if err := tester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if store.writes.Load() != 0 {
		t.Fatal("failed read enabled a save")
	}
	store.readFailure.Store(false)
	if err := tester.Click("Reload preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	choosePreference(t, tester, "Color theme", "dark")
	for _, failure := range []int32{1, 2} {
		store.saveFailure.Store(failure)
		if err := tester.Click("Save preferences"); err != nil {
			t.Fatal(err)
		}
		settlePreferences(t, p, tester)
		stored, err := store.PreferencesStore.Preferences(t.Context())
		if err != nil || stored != before || s.Preferences.Snapshot() != before || p.preferences.base != before || p.preferences.theme != "dark" || !p.preferences.dirty() || !tester.HasText("Your draft is retained") {
			t.Fatal("failed/canceled save changed committed state or lost draft", failure, p.preferences, stored, err)
		}
	}
	store.saveFailure.Store(0)
	if err := tester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	if p.preferences.dirty() || p.preferences.base.Theme != "dark" || p.preferences.problem != "" {
		t.Fatal("retry did not save retained draft", p.preferences)
	}
}

func TestNativeGeneralPreferencesBuildIsColdAndCloseWaitsForActualSave(t *testing.T) {
	s := preferenceFixture(t)
	release := make(chan struct{})
	defer func() {
		select {
		case <-release:
		default:
			close(release)
		}
	}()
	store := &preferenceFaultStore{PreferencesStore: s.Store.(hub.PreferencesStore), entered: make(chan struct{}, 1), release: release}
	installPreferenceFaultStore(t, s, store)
	p := NewNativePage(s)
	tester, theme := preferenceTester(p)
	openPreferencePage(t, p, tester)
	reads := store.reads.Load()
	for i := 0; i < 3; i++ {
		tester.Frame()
		p.poll()
	}
	if store.reads.Load() != reads || store.writes.Load() != 0 {
		t.Fatal("Build/observation performed storage IO")
	}
	choosePreference(t, tester, "Color theme", "dark")
	beforeSave, _ := tester.Find("Save preferences")
	if err := tester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-store.entered:
	case <-time.After(3 * time.Second):
		t.Fatal("save did not enter storage")
	}
	tester.Frame()
	if !p.preferences.saving || theme.Dark || s.Preferences.Snapshot().Theme != "system" {
		t.Fatal("pending storage was reported committed")
	}
	if afterSave, ok := tester.Find("Save preferences"); !ok || afterSave != beforeSave || !tester.Focused("Save preferences") {
		t.Fatal("pending storage changed Save geometry, name or focus", beforeSave, afterSave)
	}
	if err := tester.Click("Save preferences"); err != nil {
		t.Fatal(err)
	}
	tester.Key(0, ui.KeyEnter)
	if store.writes.Load() != 1 {
		t.Fatal("busy Save allowed a duplicate durable write", store.writes.Load())
	}
	p.setPane("")
	event := new(mygo.CloseEvent)
	if !p.guardPreferencesClose(event) || !event.DefaultPrevented() || !p.preferences.saving || p.pane != "preferences" {
		t.Fatal("close released an actual save owner")
	}
	close(release)
	settlePreferences(t, p, tester)
	event = new(mygo.CloseEvent)
	if p.guardPreferencesClose(event) || event.DefaultPrevented() || !theme.Dark || p.preferences.base.Theme != "dark" {
		t.Fatal("finished save still prevented close or lost commit")
	}
}
