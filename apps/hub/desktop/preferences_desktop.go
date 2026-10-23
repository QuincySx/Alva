// INPUT: Shared committed preference owner, page-local drafts and MyGo native controls/themes.
// OUTPUT: Session-free General settings, explicit Save/Reload and application appearance projection.
// POS: [L7 Hub UI] Edits presentation preferences only; no SQL, validation policy or execution ownership.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"strconv"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type preferencesPane struct {
	base                 hub.Preferences
	theme, density, size string
	loading, saving      bool
	readFailed           bool
	epoch                uint64
	problem              string
}

func (p *preferencesPane) dirty() bool {
	return p.theme != p.base.Theme || p.density != p.base.Density || p.size != strconv.Itoa(p.base.FontSize)
}
func (p *preferencesPane) accept(value hub.Preferences) {
	p.base = value
	p.theme = value.Theme
	p.density = value.Density
	p.size = strconv.Itoa(value.FontSize)
}
func (p *NativePage) openPreferences() {
	p.setPane("preferences")
	if !p.preferences.dirty() || p.preferences.base.Revision == 0 {
		p.reloadPreferences()
	}
}
func (p *NativePage) reloadPreferences() {
	if p.Service.Preferences == nil || p.preferences.saving || p.preferences.loading {
		return
	}
	p.preferences.loading = true
	p.preferences.epoch++
	epoch := p.preferences.epoch
	go func() {
		value, err := p.Service.Preferences.Read(p.ctx)
		p.dispatch(func() {
			if p.preferences.epoch != epoch {
				return
			}
			p.preferences.loading = false
			if err != nil {
				p.preferences.readFailed = true
				p.preferences.problem = "Could not read saved preferences. Reload to retry."
				return
			}
			p.preferences.accept(value)
			p.preferences.readFailed = false
			p.preferences.problem = ""
		})
	}()
}
func (p *NativePage) savePreferences() {
	v := &p.preferences
	if p.Service.Preferences == nil || v.saving || v.loading || v.readFailed || v.base.Revision == 0 || !v.dirty() {
		return
	}
	size, err := strconv.Atoi(v.size)
	if err != nil {
		v.problem = "Choose a supported body size."
		return
	}
	theme, density := v.theme, v.density
	update := hub.PreferenceUpdate{ExpectedRevision: v.base.Revision, Theme: &theme, Density: &density, FontSize: &size}
	v.saving = true
	v.epoch++
	epoch := v.epoch
	go func() {
		value, err := p.Service.Preferences.Update(p.ctx, update)
		p.dispatch(func() {
			if v.epoch != epoch {
				return
			}
			v.saving = false
			if err != nil {
				switch {
				case errors.Is(err, hub.ErrPreferenceConflict):
					v.problem = "Preferences changed in another window or client. Your draft is retained; reload before saving."
				case errors.Is(err, context.Canceled):
					v.problem = "Save canceled. Your draft is retained."
				default:
					v.problem = "Could not save preferences. Your draft is retained; retry Save or reload."
				}
				return
			}
			v.accept(value)
			v.problem = ""
		})
	}()
}

// Keep the window's context and save owner alive until storage actually returns.
func (p *NativePage) guardPreferencesClose(event *mygo.CloseEvent) bool {
	if !p.preferences.saving {
		return false
	}
	event.PreventDefault()
	p.setPane("preferences")
	p.preferences.problem = "Preferences are still saving. Wait for storage to finish before closing this window."
	return true
}

func preferenceChoice(c *ui.Context, label string, value *string, options []string, disabled bool) {
	ui.Row(c).FillWidth().AlignItems(ui.Center).Padding(8, 0).Gap(12).Children(func() {
		ui.Text(c, label).Grow(1)
		choice := ui.Select(c, value, options).Label("Select "+label).MinWidth(0).Border(0, ui.Transparent).Background(ui.Transparent).Disabled(disabled)
		if choice.Hovered() {
			choice.Background(c.Theme().SurfaceHover)
		}
	})
}

func (p *NativePage) buildPreferences(c *ui.Context) {
	v := &p.preferences
	ui.Row(c).Gap(8).Children(func() {
		ui.Button(c, "Reload preferences").Disabled(v.loading || v.saving).OnClick(p.reloadPreferences)
		ui.Button(c, "Save preferences").Disabled(v.base.Revision == 0 || v.loading || v.saving || v.readFailed || !v.dirty()).OnClick(p.savePreferences)
	})
	ui.Text(c, "General").FontSize(24).Bold()
	if v.loading {
		ui.Text(c, "Loading preferences…").Role(ui.RoleStatus)
	}
	if v.saving {
		ui.Text(c, "Saving preferences…").Role(ui.RoleStatus)
	}
	if v.problem != "" {
		ui.Text(c, v.problem).Role(ui.RoleStatus)
	}
	if v.problem == "" && p.Service.Preferences != nil && v.base.Revision != 0 && v.base.Revision != p.Service.Preferences.Snapshot().Revision && v.dirty() {
		ui.Text(c, "Preferences changed in another window or client. Your draft is retained; reload before saving.").Role(ui.RoleStatus)
	}
	ui.Scroll(c).Grow(1).Children(func() {
		ui.Column(c).FillWidth().Gap(0).Children(func() {
			ui.Text(c, "Appearance").Bold()
			options := hub.PreferenceChoices()
			sizes := make([]string, len(options.FontSizes))
			for i, n := range options.FontSizes {
				sizes[i] = strconv.Itoa(n)
			}
			disabled := v.base.Revision == 0 || v.loading || v.saving
			preferenceChoice(c, "Color theme", &v.theme, options.Themes, disabled)
			preferenceChoice(c, "Information density", &v.density, options.Densities, disabled)
			preferenceChoice(c, "Body size", &v.size, sizes, disabled)
			motion := "Off"
			if c.Preferences().ReduceMotion {
				motion = "On"
			}
			ui.Text(c, "Reduced motion: "+motion+" · follows system settings").TextColor(c.Theme().TextMuted)
		})
	})
}

func (p *NativePage) observePreferences() {
	if p.Service.Preferences == nil {
		return
	}
	if !p.preferences.loading && !p.preferences.saving && (p.preferences.base.Revision == 0 || !p.preferences.dirty()) {
		p.preferences.accept(p.Service.Preferences.Snapshot())
	}
}

func (p *NativePage) applyPreferences(c *ui.Context) {
	value := hub.DefaultPreferences()
	if p.Service.Preferences != nil {
		value = p.Service.Preferences.Snapshot()
	}
	dark := c.Theme().Dark
	if value.Theme != "system" {
		dark = value.Theme == "dark"
	}
	t := ui.LightTheme()
	if dark {
		t = ui.DarkTheme()
	}
	if dark {
		t.Background = ui.Hex("#171C1A")
		t.Surface = ui.Hex("#212825")
		t.SurfaceHover = ui.Hex("#2B342F")
		t.SurfacePressed = ui.Hex("#36423C")
		t.Text = ui.Hex("#EEF3F0")
		t.TextMuted = ui.Hex("#B0BEB6")
		t.Border = ui.Hex("#3D4A43")
		t.Accent = ui.Hex("#91CFC3")
		t.AccentText = ui.Hex("#132B25")
	} else {
		t.Background = ui.Hex("#FAFBFA")
		t.Surface = ui.Hex("#FFFFFF")
		t.SurfaceHover = ui.Hex("#F3F5F4")
		t.SurfacePressed = ui.Hex("#E9EEEC")
		t.Text = ui.Hex("#29332F")
		t.TextMuted = ui.Hex("#5E6B64")
		t.Border = ui.Hex("#DDE5E0")
		t.Accent = ui.Hex("#246B65")
		t.AccentText = ui.Hex("#FFFFFF")
	}
	t.Font = "Alva UI Sans SC"
	t.AccentHover = t.Accent.Mix(t.Text, .08)
	t.AccentPressed = t.Accent.Mix(t.Text, .16)
	t.Focus = t.Accent
	t.Selection = t.Accent.Alpha(.2)
	t.Radius = 8
	t.Spacing = 4
	if value.Density == "compact" {
		t.Spacing = 3
	}
	system := c.Preferences()
	scale := system.TextScale
	if scale <= 0 {
		scale = 1
	}
	t.FontSize = float32(value.FontSize) * scale
	if system.HighContrast {
		t.Border = t.Border.Mix(t.Text, .45)
		t.TextMuted = t.TextMuted.Mix(t.Text, .4)
	}
	c.SetTheme(t)
}
