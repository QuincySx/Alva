// INPUT: Approved B3 shell geometry and native TitleBar facts used by focused desktop tests.
// OUTPUT: Shared expected insets without duplicating magic numbers across layout suites.
// POS: [L7 Hub UI testing] Test-only geometry helper; no product behavior.
//go:build desktop

package hubdesktop

import "github.com/egoist/mygo/ui"

func b3Top(bar ui.TitleBar) float32 {
	if bar.Height > 0 {
		return max(0, (max(bar.Height, workspaceHeaderHeight)-workspaceHeaderHeight)/2)
	}
	return workspaceWindowInset
}

func b3RightBandGuard(bar ui.TitleBar) float32 {
	return max(0, bar.Height-(b3Top(bar)+workspaceHeaderHeight+workspaceRegionGap))
}

func b3PaperY(bar ui.TitleBar) float32 {
	return b3Top(bar) + workspaceHeaderHeight + b3RightBandGuard(bar) + workspaceRegionGap
}

func b3RightSafe(bar ui.TitleBar) float32 {
	return max(float32(8), bar.Right)
}

func b3RightToggleX(width float32, bar ui.TitleBar) float32 {
	return width - workspaceIconControlSize - (workspaceWindowInset + b3RightSafe(bar))
}

func b3LeftSafe(bar ui.TitleBar, windowX float32) float32 {
	return max(float32(8), bar.Left+8-windowX)
}
