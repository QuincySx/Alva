// INPUT: Canonical Alva Switch geometry and native checked/keyboard semantics under both committed themes.
// OUTPUT: Visible track/thumb paint, exact38x24 bounds and real input-state regressions.
// POS: [L7 UI testing] Prevents an interactive but invisible bare SwitchBase from reaching product pages.
//go:build desktop

package hubdesktop

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestWorkspaceSwitchPaintsFoundationThumbAndKeepsNativeSemantics(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run(map[bool]string{false: "light", true: "dark"}[dark], func(t *testing.T) {
			value := false
			service, _ := fixture(t)
			page := NewNativePage(service)
			var off, on ui.Color
			tester := ui.NewTester(func(c *ui.Context) {
				page.applyPreferences(c)
				off, on = c.Theme().TextMuted, c.Theme().AccentText
				ui.Box(c).Padding(8).Children(func() { workspaceControlSwitch(c, "switch", &value, false).Label("Motion switch") })
			}, 80, 50)
			tester.SetDark(dark)
			r, ok := tester.Find("Motion switch")
			if !ok || r.W != 38 || r.H != 24 {
				t.Fatal("switch lost Foundation bounds", r)
			}
			pixel := tester.Image().RGBAAt(int(r.X+12), int(r.Y+12))
			if pixel.R != off.R || pixel.G != off.G || pixel.B != off.B || pixel.A == 0 {
				t.Fatal("off thumb is not visibly painted", pixel, off)
			}
			if err := tester.Click("Motion switch"); err != nil {
				t.Fatal(err)
			}
			if !value {
				t.Fatal("painted switch lost native pointer toggling")
			}
			pixel = tester.Image().RGBAAt(int(r.X+26), int(r.Y+12))
			if pixel.R != on.R || pixel.G != on.G || pixel.B != on.B || pixel.A == 0 {
				t.Fatal("on thumb is not visibly painted", pixel, on)
			}
			tester.Key(0, ui.KeySpace)
			if value {
				t.Fatal("painted switch lost native keyboard toggling")
			}
		})
	}
}
