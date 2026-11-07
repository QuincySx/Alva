// INPUT: Window-local sidebar intent and the shared committed PreferencesService sparse CAS.
// OUTPUT: Debounced layout persistence, safe close settlement and cold restore without changing user forms.
// POS: [L7 Hub UI] Saves presentation fields only; execution, account routing and database policy stay with their owners.
//go:build desktop

package hubdesktop

import (
	"errors"
	"math"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo"
)

const workspaceLayoutSaveProblem = "导航布局未保存；再次调整可重试。"

type workspaceLayoutPersistence struct {
	loaded, saving, closeRequested    bool
	revision, saved, observed, failed uint64
	due                               time.Time
}

// Render observes committed state only. Pending local gestures retain ownership
// until their own receipt; another window's theme save never overwrites them.
func (p *NativePage) observeWorkspaceLayoutPreferences() {
	if p.Service.Preferences == nil {
		return
	}
	s := &p.layoutPersistence
	v := p.Service.Preferences.Snapshot()
	if !s.loaded || v.Revision != s.revision {
		s.loaded, s.revision = true, v.Revision
		p.projectWorkspaceLayoutPreferences(v, s.saved)
	}
}

// Each field keeps its own gesture version: a pending collapse does not own a
// remotely edited width, and a receipt cannot consume a later width gesture.
func (p *NativePage) projectWorkspaceLayoutPreferences(value hub.Preferences, version uint64) {
	if p.layout.widthChanged <= version {
		p.layout.sidebarWidth = float32(value.SidebarWidth)
	}
	if p.layout.collapsedChanged <= version {
		p.layout.sidebarCollapsed = value.SidebarCollapsed
	}
}

// The existing independent observer calls this; Build never starts storage I/O.
// Force is an explicit close/flush gesture. Drag frames remain memory-only.
func (p *NativePage) flushWorkspaceLayoutPreferences(force bool) {
	p.observeWorkspaceLayoutPreferences()
	s := &p.layoutPersistence
	if p.Service.Preferences == nil || p.ctx.Err() != nil || s.saving || p.layout.changed == s.saved {
		return
	}
	if p.layout.changed != s.observed {
		s.observed, s.due = p.layout.changed, time.Now().Add(250*time.Millisecond)
	}
	if !force && (time.Now().Before(s.due) || p.layout.sidebarResize.dragging || s.failed == p.layout.changed) {
		return
	}
	version := p.layout.changed
	width, collapsed := int(math.Round(float64(p.layout.sidebarWidth))), p.layout.sidebarCollapsed
	service := p.Service.Preferences
	base := service.Snapshot()
	update := hub.PreferenceUpdate{ExpectedRevision: base.Revision}
	if p.layout.widthChanged > s.saved {
		update.SidebarWidth = &width
	}
	if p.layout.collapsedChanged > s.saved {
		update.SidebarCollapsed = &collapsed
	}
	s.saving = true
	go func() {
		value, err := service.Update(p.ctx, update)
		// A known CAS rejection committed nothing. Refresh and merge these two
		// optional fields once, preserving all independently edited fields.
		if errors.Is(err, hub.ErrPreferenceConflict) {
			value, err = service.Read(p.ctx)
			if err == nil && (update.SidebarWidth != nil && value.SidebarWidth != width || update.SidebarCollapsed != nil && value.SidebarCollapsed != collapsed) {
				update.ExpectedRevision = value.Revision
				value, err = service.Update(p.ctx, update)
			}
		}
		p.dispatch(func() {
			s.saving = false
			if err != nil {
				s.failed, s.closeRequested = version, false
				p.problem = workspaceLayoutSaveProblem
				return
			}
			s.saved, s.revision, s.failed = version, value.Revision, 0
			p.projectWorkspaceLayoutPreferences(value, version)
			if p.problem == workspaceLayoutSaveProblem {
				p.problem = ""
			}
			closeRequested := s.closeRequested && p.layout.changed == version
			s.closeRequested = false
			if closeRequested && p.window != nil {
				p.window.Close()
			}
		})
	}()
}

func (p *NativePage) guardWorkspaceLayoutClose(event *mygo.CloseEvent) bool {
	s := &p.layoutPersistence
	// A canceled observer cannot admit a flush or reliably receive its UI
	// receipt. The shared service still owns and joins admitted storage calls.
	if p.ctx.Err() != nil {
		s.closeRequested = false
		return false
	}
	if p.Service.Preferences == nil || p.layout.changed == s.saved && !s.saving {
		return false
	}
	// A settled failure is visible and retains the local preference. It must
	// not trap the window forever; a later explicit close may discard it.
	if !s.saving && s.failed == p.layout.changed {
		return false
	}
	s.closeRequested = true
	p.flushWorkspaceLayoutPreferences(true)
	event.PreventDefault()
	return true
}
