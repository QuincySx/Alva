// INPUT: Native MyGo title-bar facts and the shared Alva window dimensions.
// OUTPUT: System-owned window controls and safe, aligned workspace header regions.
// POS: [L7 Hub UI] Presentation only; native Close retains the existing save and cleanup guards.
//go:build desktop

package hubdesktop

import (
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

const workspaceHeaderHeight float32 = 36

func workspaceWindowOptions(platform string) mygo.WindowOptions {
	style := mygo.TitleBarHidden
	var lights *mygo.Point
	if platform == "darwin" {
		style = mygo.TitleBarHiddenInset
		// AppKit's default toolbar band need not share the controls' center.
		// An explicit native position makes TitleBar report their symmetric band.
		lights = &mygo.Point{X: 16, Y: 19}
	}
	return mygo.WindowOptions{Title: "Alva", Width: 1240, Height: 850, MinWidth: 960, MinHeight: 680,
		TitleBarStyle: style, TitleBarHeight: 52, TrafficLightPosition: lights}
}

func workspaceTopInset(c *ui.Context) float32 {
	if height := c.TitleBar().Height; height > 0 {
		return max(0, (max(height, workspaceHeaderHeight)-workspaceHeaderHeight)/2)
	}
	return workspaceWindowInset
}

// Windows/Linux controls can cover the entire native band on the right. The
// header owns its remaining safe height, so its body never enters that band.
// macOS keeps the formal 36px header beside its left-side traffic lights.
func workspaceHeaderBottomInset(c *ui.Context) float32 {
	if bar := c.TitleBar(); bar.Right > 0 {
		return max(0, bar.Height-workspaceTopInset(c)-workspaceHeaderHeight)
	}
	return 0
}

func workspaceHeaderRegion(c *ui.Context, key, label string, windowX float32, content func()) {
	left := workspaceLeftSafeInset(c, windowX)
	right := workspaceRightSafeInset(c)
	ui.Column(c.Key(key)).Label(label+" region").FillWidth().Shrink(0).Padding(0, 0, workspaceHeaderBottomInset(c), 0).Children(func() {
		ui.Row(c).Label(label).FillWidth().Height(workspaceHeaderHeight).Shrink(0).
			Padding(0, right, 0, left).Gap(4).AlignItems(ui.Center).DragWindow().Children(content)
	})
}

// B3 places each toolbar and its content surface in the same gapped column.
// The 8px column gap is therefore the complete space below the header; the
// native band is already represented by workspaceTopInset, not added twice.
func workspaceColumnToolbar(c *ui.Context, key, label string, left, right float32, content func()) {
	ui.Column(c).Label(label+" region").FillWidth().Shrink(0).
		Padding(0, 0, workspaceNativeRightBandGuard(c), 0).Children(func() {
		ui.Row(c.Key(key)).Label(label).FillWidth().Height(workspaceHeaderHeight).Shrink(0).
			Padding(0, right, 0, left).Gap(4).AlignItems(ui.Center).DragWindow().Children(content)
	})
}

// A right-side native control band can be taller than the 36px toolbar plus
// the shell inset and gap. Reserve only that remainder in every column so the
// first content surface never starts under the system controls twice.
func workspaceNativeRightBandGuard(c *ui.Context) float32 {
	return max(0, c.TitleBar().Height-(workspaceTopInset(c)+workspaceHeaderHeight+workspaceRegionGap))
}

func workspaceLeftSafeInset(c *ui.Context, windowX float32) float32 {
	return max(float32(8), c.TitleBar().Left+8-windowX)
}

func workspaceRightSafeInset(c *ui.Context) float32 {
	return max(float32(8), c.TitleBar().Right+8-workspaceWindowInset)
}

func workspaceNavigationHeader(c *ui.Context, label string, minimumLeft float32, content func()) {
	// The row starts at the shell inset. Keep the native controls' actual band
	// plus the shared 8px gap clear without assuming a fixed guard width.
	left := max(minimumLeft-workspaceWindowInset, c.TitleBar().Left+8-workspaceWindowInset)
	ui.Column(c).Label(label+" region").FillWidth().Shrink(0).Padding(0, 0, workspaceHeaderBottomInset(c), 0).Children(func() {
		ui.Row(c).Label(label).FillWidth().Height(workspaceHeaderHeight).Padding(0, 0, 0, left).
			Gap(4).AlignItems(ui.Center).DragWindow().Children(content)
	})
}
