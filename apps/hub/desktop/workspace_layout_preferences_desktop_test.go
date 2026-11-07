// INPUT: Real Hub SQLite/bootstrap, blocked preference storage and MyGo headless layout controls.
// OUTPUT: Durable sidebar restore, sparse CAS, gesture receipt isolation and safe close regressions.
// POS: [L7 Hub UI tests] Verifies presentation persistence without OS windows or changing execution owners.
//go:build desktop

package hubdesktop

import (
	"context"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func settleWorkspaceLayout(t *testing.T, p *NativePage, tester *ui.Tester, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if done() {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("layout persistence did not settle", p.layout, p.layoutPersistence, p.problem)
}

func TestWorkspaceLayoutPreferencesRealSQLiteReopenAndUnsavedForm(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	open := func() *bootstrap.Application {
		a, err := bootstrap.Open(t.Context(), bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: directory, Demo: true})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = a.Close(context.Background()) })
		return a
	}
	a := open()
	p := NewNativePage(a.Service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	edge, _ := tester.Find("Resize session navigation")
	tester.Press(edge.X+4, edge.Y+300)
	tester.Move(edge.X+132, edge.Y+300)
	tester.Release(edge.X+132, edge.Y+300)
	connectionClick(t, tester, "Collapse session navigation")
	p.model.Draft = "仍在编辑的聊天草稿"
	if err := workspaceTestMenu(tester, "设置"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	choosePreference(t, tester, "Color theme", "dark")
	form := p.preferences
	before := a.Service.Preferences.Snapshot()
	p.flushWorkspaceLayoutPreferences(true)
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving && p.layoutPersistence.saved == p.layout.changed })
	saved := a.Service.Preferences.Snapshot()
	if saved.SidebarWidth != 348 || !saved.SidebarCollapsed || saved.Theme != before.Theme || p.preferences != form || p.model.Draft != "仍在编辑的聊天草稿" {
		t.Fatal("layout save lost its width or changed an independent form/draft", saved, p.preferences)
	}
	connectionClick(t, tester, "Back to workspace")
	other := NewNativePage(a.Service)
	otherTester := ui.NewTester(workspaceStaticView(other), 1240, 850)
	rail, railFound := otherTester.Find("Session navigation")
	if railFound || other.layout.sidebarWidth != 348 {
		t.Fatal("another observer did not adopt the committed layout", rail, other.layout)
	}
	if err := a.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	reopened := open()
	restored := NewNativePage(reopened.Service)
	restoreTester := ui.NewTester(workspaceStaticView(restored), 1240, 850)
	rail, railFound = restoreTester.Find("Session navigation")
	if railFound || restored.layout.sidebarWidth != 348 || !restored.layout.sidebarCollapsed {
		t.Fatal("SQLite reopen did not restore the committed sidebar", rail, restored.layout)
	}
	connectionClick(t, restoreTester, "Expand session navigation")
	expanded, _ := restoreTester.Find("Session navigation")
	if expanded.W != 348 || reopened.Service.Runtime.HasActive() {
		t.Fatal("restored expansion lost its preferred width or executed", expanded)
	}
}

func TestWorkspaceLayoutPreferencesFirstObservationRetainsLocalGestureAndBuildIsCold(t *testing.T) {
	service := preferenceFixture(t)
	store := &preferenceFaultStore{PreferencesStore: service.Store.(hub.PreferencesStore)}
	installPreferenceFaultStore(t, service, store)
	p := NewNativePage(service)
	p.changeWorkspaceSidebar(316, true)
	reads := store.reads.Load()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	for i := 0; i < 3; i++ {
		tester.Frame()
	}
	if p.layout.sidebarWidth != 316 || !p.layout.sidebarCollapsed || store.reads.Load() != reads || store.writes.Load() != 0 {
		t.Fatal("first observation replaced local intent or Build performed I/O", p.layout)
	}
	p.flushWorkspaceLayoutPreferences(false)
	if store.writes.Load() != 0 || p.layoutPersistence.saving {
		t.Fatal("a drag frame bypassed the debounce")
	}
	p.flushWorkspaceLayoutPreferences(true)
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving && p.layoutPersistence.saved == p.layout.changed })
	if saved := service.Preferences.Snapshot(); saved.SidebarWidth != 316 || !saved.SidebarCollapsed {
		t.Fatal("initial retained gesture did not reach storage", saved)
	}
}

func TestWorkspaceLayoutPreferencesOldReceiptCloseWaitAndExecutionOwner(t *testing.T) {
	service := preferenceFixture(t)
	release := make(chan struct{})
	var once sync.Once
	defer once.Do(func() { close(release) })
	store := &preferenceFaultStore{PreferencesStore: service.Store.(hub.PreferencesStore), entered: make(chan struct{}, 3), release: release}
	installPreferenceFaultStore(t, service, store)
	owner := &settingsRunningEngine{entered: make(chan struct{}), release: make(chan struct{})}
	if err := service.Runtime.Register("alva", owner); err != nil {
		t.Fatal(err)
	}
	session, err := service.Create(t.Context(), "layout background owner", "")
	if err != nil {
		t.Fatal(err)
	}
	_, done, err := service.Runtime.Start(t.Context(), session.ID, "explicit local fixture")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { close(owner.release); <-done }()
	<-owner.entered
	identity, _ := service.Runtime.ActiveExecution(session.ID)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.changeWorkspaceSidebar(300, true)
	version := p.layout.changed
	event := new(mygo.CloseEvent)
	if !p.guardWorkspaceLayoutClose(event) || !event.DefaultPrevented() {
		t.Fatal("close released unsaved layout before the admitted write")
	}
	select {
	case <-store.entered:
	case <-time.After(time.Second):
		t.Fatal("layout save never entered its actual storage owner")
	}
	p.changeWorkspaceSidebar(364, false)
	p.flushWorkspaceLayoutPreferences(true)
	event = new(mygo.CloseEvent)
	if !p.guardWorkspaceLayoutClose(event) || !event.DefaultPrevented() || store.writes.Load() != 1 {
		t.Fatal("close/flush duplicated or abandoned the pending write")
	}
	once.Do(func() { close(release) })
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving })
	if p.layoutPersistence.saved != version || p.layout.changed == version || p.layout.sidebarWidth != 364 || p.layout.sidebarCollapsed || p.layoutPersistence.closeRequested {
		t.Fatal("older close receipt consumed the later gesture", p.layout, p.layoutPersistence)
	}
	p.flushWorkspaceLayoutPreferences(true)
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving && p.layoutPersistence.saved == p.layout.changed })
	event = new(mygo.CloseEvent)
	current, active := service.Runtime.ActiveExecution(session.ID)
	if p.guardWorkspaceLayoutClose(event) || event.DefaultPrevented() || !active || current != identity || owner.cancels.Load() != 0 || owner.starts.Load() != 1 {
		t.Fatal("settled layout blocked close or changed the actual execution owner", current)
	}
	if saved := service.Preferences.Snapshot(); saved.SidebarWidth != 364 || saved.SidebarCollapsed || store.writes.Load() != 2 {
		t.Fatal("later gesture did not receive its own committed receipt", saved)
	}
}

func TestWorkspaceLayoutPreferencesConcurrentAppearanceCASRetainsUserForm(t *testing.T) {
	service := preferenceFixture(t)
	port := service.Store.(hub.PreferencesStore)
	release := make(chan struct{})
	var once sync.Once
	defer once.Do(func() { close(release) })
	store := &preferenceFaultStore{PreferencesStore: port, entered: make(chan struct{}, 3), release: release}
	installPreferenceFaultStore(t, service, store)
	other, err := product.NewPreferencesService(t.Context(), port)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = other.Close(context.Background()) })
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	openPreferencePage(t, p, tester)
	choosePreference(t, tester, "Color theme", "dark")
	form := p.preferences
	p.changeWorkspaceSidebar(332, true)
	p.flushWorkspaceLayoutPreferences(true)
	select {
	case <-store.entered:
	case <-time.After(time.Second):
		t.Fatal("layout save did not enter storage")
	}
	theme, font, motion := "light", 16, true
	appearance, err := other.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: other.Snapshot().Revision, Theme: &theme, FontSize: &font, ReducedMotion: &motion})
	if err != nil {
		t.Fatal(err)
	}
	once.Do(func() { close(release) })
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving })
	saved := service.Preferences.Snapshot()
	if saved.Revision != appearance.Revision+1 || saved.Theme != theme || saved.FontSize != font || !saved.ReducedMotion || saved.SidebarWidth != 332 || !saved.SidebarCollapsed || p.preferences != form || store.writes.Load() != 2 {
		t.Fatal("layout CAS replaced concurrent appearance or an unsaved form", saved, p.preferences, store.writes.Load())
	}
}

func TestWorkspaceLayoutPreferencesConcurrentWidthAndCollapseMergePerField(t *testing.T) {
	for _, mode := range []string{"collapse", "width", "width-with-later-gesture"} {
		t.Run(mode, func(t *testing.T) {
			a := preferenceFixture(t)
			initialWidth := 220
			if _, err := a.Preferences.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: a.Preferences.Snapshot().Revision, SidebarWidth: &initialWidth}); err != nil {
				t.Fatal(err)
			}
			b, err := product.NewHubService(a.Store, a.Runtime, a.Repository, a.DefaultTarget, a.DataDirectory, a.Demo)
			if err != nil {
				t.Fatal(err)
			}
			release := make(chan struct{})
			var once sync.Once
			defer once.Do(func() { close(release) })
			blocked := &preferenceFaultStore{PreferencesStore: a.Store.(hub.PreferencesStore), entered: make(chan struct{}, 3), release: release}
			installPreferenceFaultStore(t, b, blocked)
			t.Cleanup(func() { _ = b.Preferences.Close(context.Background()) })
			pageA, pageB := NewNativePage(a), NewNativePage(b)
			testerA, testerB := ui.NewTester(workspaceStaticView(pageA), 1240, 850), ui.NewTester(workspaceStaticView(pageB), 1240, 850)
			if mode == "collapse" {
				connectionClick(t, testerB, "Collapse session navigation")
			} else {
				pageB.changeWorkspaceSidebar(348, false)
			}
			pageB.flushWorkspaceLayoutPreferences(true)
			select {
			case <-blocked.entered:
			case <-time.After(time.Second):
				t.Fatal("second window did not capture the old revision")
			}
			if mode == "collapse" {
				edge, _ := testerA.Find("Resize session navigation")
				testerA.Press(edge.X+4, edge.Y+300)
				testerA.Move(edge.X+132, edge.Y+300)
				testerA.Release(edge.X+132, edge.Y+300)
			} else {
				connectionClick(t, testerA, "Collapse session navigation")
			}
			pageA.flushWorkspaceLayoutPreferences(true)
			settleWorkspaceLayout(t, pageA, testerA, func() bool {
				return !pageA.layoutPersistence.saving && pageA.layoutPersistence.saved == pageA.layout.changed
			})
			if mode == "width-with-later-gesture" {
				pageB.changeWorkspaceSidebar(364, false)
			}
			once.Do(func() { close(release) })
			settleWorkspaceLayout(t, pageB, testerB, func() bool { return !pageB.layoutPersistence.saving })
			saved := b.Preferences.Snapshot()
			if saved.SidebarWidth != 348 || !saved.SidebarCollapsed || blocked.writes.Load() != 2 || !pageB.layout.sidebarCollapsed {
				t.Fatal("field-level CAS replaced another window's independent layout field", saved, pageB.layout, blocked.writes.Load())
			}
			if mode == "width-with-later-gesture" {
				if pageB.layout.sidebarWidth != 364 || pageB.layout.changed == pageB.layoutPersistence.saved {
					t.Fatal("receipt consumed the later width gesture", pageB.layout, pageB.layoutPersistence)
				}
				pageB.flushWorkspaceLayoutPreferences(true)
				settleWorkspaceLayout(t, pageB, testerB, func() bool {
					return !pageB.layoutPersistence.saving && pageB.layoutPersistence.saved == pageB.layout.changed
				})
				if saved := b.Preferences.Snapshot(); saved.SidebarWidth != 364 || !saved.SidebarCollapsed {
					t.Fatal("later width save replaced the adopted remote collapse", saved)
				}
			} else if pageB.layout.sidebarWidth != 348 {
				t.Fatal("receipt did not project the untouched remote width", pageB.layout)
			}
			if _, err := a.Preferences.Read(t.Context()); err != nil {
				t.Fatal(err)
			}
			testerA.Frame()
			if !pageA.layout.sidebarCollapsed || pageA.layout.sidebarWidth != pageB.layout.sidebarWidth {
				t.Fatal("clean first window did not adopt the merged layout", pageA.layout, pageB.layout)
			}
		})
	}
}

func TestWorkspaceLayoutPreferencesFailureRetryAndCanceledClose(t *testing.T) {
	service := preferenceFixture(t)
	store := &preferenceFaultStore{PreferencesStore: service.Store.(hub.PreferencesStore)}
	store.saveFailure.Store(1)
	installPreferenceFaultStore(t, service, store)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.changeWorkspaceSidebar(300, true)
	p.flushWorkspaceLayoutPreferences(true)
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving })
	event := new(mygo.CloseEvent)
	if p.layoutPersistence.failed != p.layout.changed || p.layoutPersistence.saved != 0 || p.problem == "" || p.guardWorkspaceLayoutClose(event) || event.DefaultPrevented() {
		t.Fatal("settled save failure hid the draft or trapped close", p.layoutPersistence)
	}
	store.saveFailure.Store(0)
	p.changeWorkspaceSidebar(316, true)
	p.flushWorkspaceLayoutPreferences(true)
	settleWorkspaceLayout(t, p, tester, func() bool { return !p.layoutPersistence.saving && p.layoutPersistence.saved == p.layout.changed })
	if saved := service.Preferences.Snapshot(); saved.SidebarWidth != 316 || !saved.SidebarCollapsed || p.layoutPersistence.failed != 0 || p.problem != "" {
		t.Fatal("a later adjustment could not retry the failed save", saved)
	}
	p.changeWorkspaceSidebar(332, false)
	ctx, cancel := context.WithCancel(context.Background())
	p.ctx = ctx
	cancel()
	writes := store.writes.Load()
	event = new(mygo.CloseEvent)
	if p.guardWorkspaceLayoutClose(event) || event.DefaultPrevented() || p.layoutPersistence.closeRequested || store.writes.Load() != writes {
		t.Fatal("canceled page context trapped close or admitted another write")
	}
}

func TestWorkspaceLayoutPreferencesCanceledPendingSaveDoesNotTrapObserverClose(t *testing.T) {
	service := preferenceFixture(t)
	release := make(chan struct{})
	defer close(release)
	store := &preferenceFaultStore{PreferencesStore: service.Store.(hub.PreferencesStore), entered: make(chan struct{}, 1), release: release}
	installPreferenceFaultStore(t, service, store)
	p := NewNativePage(service)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	p.ctx = ctx
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.changeWorkspaceSidebar(332, true)
	p.flushWorkspaceLayoutPreferences(true)
	select {
	case <-store.entered:
	case <-time.After(time.Second):
		t.Fatal("layout save did not enter storage")
	}
	cancel()
	event := new(mygo.CloseEvent)
	if p.guardWorkspaceLayoutClose(event) || event.DefaultPrevented() || p.layoutPersistence.closeRequested {
		t.Fatal("canceled observer was trapped waiting for a UI receipt")
	}
	joined, stop := context.WithTimeout(context.Background(), time.Second)
	defer stop()
	if err := service.Preferences.Close(joined); err != nil {
		t.Fatal("shared service failed to join canceled accepted storage", err)
	}
	tester.Frame()
	if saved := service.Preferences.Snapshot(); saved.SidebarWidth != 0 || saved.SidebarCollapsed || store.writes.Load() != 1 {
		t.Fatal("canceled pending layout was committed or replayed", saved)
	}
}
