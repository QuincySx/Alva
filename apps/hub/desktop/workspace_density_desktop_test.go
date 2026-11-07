// INPUT: One normal Foundation scale and dense native task projections at fixed window sizes.
// OUTPUT: Measured list capacity, compact control typography and bounded scope selection regressions.
// POS: [L7 Hub UI tests] Uses deterministic temporary fixtures; never sends or reads user files.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceNormalDensityFitsTaskNavigationAndScope(t *testing.T) {
	for _, size := range []struct{ w, h int }{{1240, 850}, {1547, 932}} {
		t.Run(fmt.Sprintf("%dx%d", size.w, size.h), func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			titles := []string{"整理项目资料与设计决策", "检查模型连接与工具结果", "审查工作区变更", "继续上次的研究任务"}
			for i := range 40 {
				p.sessions = append(p.sessions, hub.Session{ID: fmt.Sprintf("density-%02d", i), Title: fmt.Sprintf("%s %02d", titles[i%len(titles)], i+1), Status: engine.Outcome("completed")})
			}
			tester := ui.NewTester(workspaceStaticView(p), size.w, size.h)
			tester.SetTitleBar(ui.TitleBar{Height: 52, Left: 86})
			tester.SetDark(true)
			get := func(label string) ui.Rect {
				t.Helper()
				r, ok := tester.Find(label)
				if !ok {
					t.Fatal("missing density control", label)
				}
				return r
			}
			task, settings, caption := get("New session"), get("General settings"), get("最近")
			if task.H != 28 || settings.W != 28 || settings.H != 28 || caption.H != 20.5 {
				t.Fatal("normal control or caption scale drifted", task, settings, caption)
			}
			visible := 0
			for _, session := range p.sessions {
				r, ok := tester.Find(p.sessionLabel(session))
				if ok && r.H > 28 {
					t.Fatal("navigation row is oversized", r)
				}
				if ok && r.W > 0 && r.H == 28 && r.Y+r.H <= settings.Y-4 && r.Y >= 48 {
					visible++
				}
			}
			if visible < 16 {
				t.Fatal("normal viewport still wastes the task list", visible)
			}
			t.Logf("%dx%d: %d fully visible task rows; normal text13, row28, control32, action28", size.w, size.h, visible)
			renderDensityReview(t, tester, fmt.Sprintf("normal-home-%dx%d.png", size.w, size.h))
			p.settingsScope.destination = "resources"
			p.settingsScope.page.Sessions = append([]hub.Session(nil), p.sessions[:16]...)
			p.pane = "workspace-scope"
			tester.Frame()
			search, back, header := get("Find scope session"), get("Cancel scope selection"), get("Workspace header")
			if search.H != 32 || search.W > 760 || back.H != 28 || back.Y+back.H/2 != header.Y+header.H/2 {
				t.Fatal("scope chooser lost bounded normal-density controls", search, back, header)
			}
			if tester.HasText("选择已保存的任务后，打开它的文件与任务。") {
				t.Fatal("redundant chooser description returned")
			}
			if len(requests) != 0 || service.Runtime.HasActive() {
				t.Fatal("density rendering executed a model request")
			}
			renderDensityReview(t, tester, fmt.Sprintf("normal-scope-%dx%d.png", size.w, size.h))
		})
	}
}

func renderDensityReview(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_DENSITY_REVIEW_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	if err := png.Encode(f, tester.Image()); err != nil {
		f.Close()
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
}
