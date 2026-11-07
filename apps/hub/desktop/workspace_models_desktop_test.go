// INPUT: Committed static/dynamic Hub targets, connection metadata and headless MyGo input.
// OUTPUT: Model identity, full-row paint/action geometry, virtual navigation and independent favorites/options.
// POS: [L7 Hub UI testing] Exercises real selection without OS windows, network, model execution or empty sessions.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"math"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceModelRowWholeSurfaceAndIndependentActions(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run(map[bool]string{false: "light", true: "dark"}[dark], func(t *testing.T) {
			service, _ := connectionsFixture(t)
			other := service.DefaultTarget
			other.ModelID = "second-model"
			service.Targets = append(service.Targets, other)
			p := NewNativePage(service)
			p.model.Draft = "preserve the prompt"
			var hover, selected ui.Color
			tester := ui.NewTester(func(c *ui.Context) {
				c.DisableAnimations()
				p.Build(c)
				hover, selected = workspacePalette(c).hover, workspacePalette(c).subtle
			}, 1240, 850)
			tester.SetDark(dark)
			connectionClick(t, tester, "Agent and model")
			rect := func(label string) ui.Rect {
				t.Helper()
				r, ok := tester.Find(label)
				if !ok {
					t.Fatal("missing model row control", label)
				}
				return r
			}
			row := rect("Use alva · " + other.ModelID)
			star := rect("Favorite model: " + other.ModelID)
			options := rect("Model options: " + other.ModelID)
			close := func(a, b float32) bool { return math.Abs(float64(a-b)) <= .5 }
			if !close(row.H, 56) || star.W != 28 || star.H != 28 || options.W != 28 || options.H != 28 ||
				!close(star.Y+14, row.Y+row.H/2) || !close(options.Y+14, row.Y+row.H/2) ||
				!close(options.X-star.X-star.W, 8) || !close(row.X+row.W-options.X-options.W, 12) {
				t.Fatal("row actions do not follow centered32px slots, gap8 and right inset12", row, star, options)
			}
			assertSurface := func(box ui.Rect, want ui.Color) {
				t.Helper()
				for _, x := range []float32{box.X + 3, box.X + box.W - 3} {
					pixel := tester.Image().RGBAAt(int(x), int(box.Y+box.H/2))
					if pixel.R != want.R || pixel.G != want.G || pixel.B != want.B || pixel.A == 0 {
						t.Fatalf("model row background stops before its actions: pixel=%v want=%v at%v", pixel, want, x)
					}
				}
			}
			// Hovering either child keeps a single surface all the way to the
			// row's right edge, rather than highlighting just a title button.
			for _, point := range []ui.Rect{row, star, options} {
				tester.Move(point.X+point.W/2, point.Y+point.H/2)
				assertSurface(row, hover)
			}
			connectionClick(t, tester, "Favorite model: "+other.ModelID)
			waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
			if !service.Connections.ModelPreference(other).Favorite || !p.composer.targetOpen ||
				hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(service.DefaultTarget) {
				t.Fatal("favorite action selected the row, closed the picker or failed to save")
			}
			connectionClick(t, tester, "Model options: "+other.ModelID)
			if p.composer.modelPicker.config == "" || !p.composer.targetOpen ||
				hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(service.DefaultTarget) {
				t.Fatal("options action also selected the row")
			}
			tester.Key(0, ui.KeyEscape)
			// The right padding is also part of the row's selection target.
			tester.ClickAt(row.X+row.W-3, row.Y+row.H/2)
			if p.composer.targetOpen || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(other) {
				t.Fatal("whole-row pointer selection did not choose the exact model")
			}
			connectionClick(t, tester, "Agent and model")
			row = rect("Use alva · " + other.ModelID)
			tester.Move(0, 0)
			assertSurface(row, selected)
			tester.Key(0, ui.KeyEscape)
			if !tester.Focused("Agent and model") || p.model.Draft != "preserve the prompt" || service.Runtime.HasActive() {
				t.Fatal("browsing changed the draft, started execution or lost trigger focus")
			}
			sessions, err := service.Store.ListSessions(t.Context(), 8)
			if err != nil || len(sessions) != 0 {
				t.Fatal("model actions created a session", err)
			}
		})
	}
}

func TestWorkspaceModelRowKeepsFavoriteSlotWithoutPreferenceStore(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := composerTester(p, 900)
	connectionClick(t, tester, "Agent and model")
	label := "Favorite model: " + service.DefaultTarget.ModelID
	star, exists := tester.Find(label)
	if !exists || star.W != 28 || star.H != 28 {
		t.Fatal("missing service removed the canonical favorite slot", star)
	}
	connectionClick(t, tester, label)
	// The owning row may take focus; Tester.Focused also matches a label's
	// focused ancestor. The disabled child must neither save nor select.
	if p.composer.modelPicker.saving || !p.composer.targetOpen || service.Runtime.HasActive() ||
		hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(service.DefaultTarget) {
		t.Fatal("unavailable preference store accepted a favorite action or selected the model")
	}
}

func TestWorkspaceModelRowsGroupOnlyExactConnectionAndModel(t *testing.T) {
	base := hub.Target{EngineID: "alva", ModelID: "shared", ProviderID: "p", AccountSource: "a", BillingRoute: "first"}
	otherAgent, otherAccount := base, base
	otherAgent.EngineID = "pi"
	otherAccount.AccountSource = "b"
	rows, sources := workspaceModelRows([]hub.Target{base, otherAgent, base, otherAccount}, nil, otherAgent, base)
	if len(rows) != 2 || len(sources) != 3 || len(rows[0].routes) != 2 || rows[0].routes[rows[0].chosen].EngineID != "pi" {
		t.Fatalf("wrong model identity, duplicate handling or compatible Agent: rows=%+v sources=%+v", rows, sources)
	}
	entry := hub.ModelConnection{ID: "stable", Revision: 8, Name: "Work account", Models: []hub.ConnectionModel{{ID: "shared", Name: "Friendly name", ImageCapable: true}}}
	route := entry.Target(entry.Models[0])
	rows, _ = workspaceModelRows([]hub.Target{route}, []hub.ModelConnection{entry}, base, base)
	if rows[0].name != "Friendly name" || rows[0].source != "Work account" || rows[0].capabilities != "视觉 · 已声明" {
		t.Fatalf("committed metadata was ignored: %+v", rows[0])
	}
	newRevision := route
	newRevision.AccountSource = "connection:stable@9"
	if workspaceModelIdentity(route) != workspaceModelIdentity(newRevision) || hub.RouteKey(route) == hub.RouteKey(newRevision) {
		t.Fatal("presentation identity or exact execution revision was lost")
	}
}

func TestWorkspaceModelPickerSearchProviderAndAgentDraft(t *testing.T) {
	service, requests := fixture(t)
	base := service.DefaultTarget
	alt := base
	alt.EngineID = "pi"
	other := base
	other.AccountSource, other.ModelID = "different-account", "second-model"
	service.Targets = []hub.Target{base, alt, other}
	p := NewNativePage(service)
	p.model.Draft = "保留这条草稿"
	tester := composerTester(p, 900)
	connectionClick(t, tester, "Agent and model")
	if len(p.composer.modelPicker.rows) != 2 || len(p.composer.modelPicker.visible) != 2 {
		t.Fatal("Agent compatibility duplicated a model row")
	}
	connectionClick(t, tester, "Model options: "+base.ModelID)
	connectionClick(t, tester, "Model Agent")
	connectionClick(t, tester, "Agent option: pi · "+base.ModelID)
	if !p.composer.targetOpen || p.composer.modelPicker.rows[0].chosen != 1 || p.workspaceComposerTarget().EngineID != "alva" {
		t.Fatal("changing Agent in options selected or closed the model")
	}
	tester.Key(0, ui.KeyEscape)
	tester.Key(0, ui.KeyEscape)
	if p.workspaceComposerTarget().EngineID != "alva" {
		t.Fatal("closing the picker committed an Agent draft")
	}
	connectionClick(t, tester, "Agent and model")
	if p.composer.modelPicker.rows[0].routes[p.composer.modelPicker.rows[0].chosen].EngineID != "alva" {
		t.Fatal("reopening retained an uncommitted Agent draft")
	}
	connectionClick(t, tester, "Filter models: local · different-account")
	if len(p.composer.modelPicker.visible) != 1 || p.composer.modelPicker.rows[p.composer.modelPicker.visible[0]].model != other.ModelID {
		t.Fatal("Provider filter leaked another account")
	}
	connectionClick(t, tester, "Filter models: 全部模型")
	connectionType(t, tester, "Search models", "SECOND different-ACCOUNT")
	if len(p.composer.modelPicker.visible) != 1 {
		t.Fatal("case-insensitive model/source search did not match")
	}
	tester.Key(0, ui.KeyEnter)
	if p.composer.targetOpen || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(other) || p.model.Draft != "保留这条草稿" {
		t.Fatal("single Enter did not choose the exact searched connection")
	}
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("model browsing/selection created a session or sent a request", err)
	}
}

func TestWorkspaceModelPickerRejectsChangedExactRoute(t *testing.T) {
	service, _ := connectionsFixture(t)
	entry, err := service.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "selected account", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/v1/chat/completions", Auth: "none", Enabled: true, Models: []hub.ConnectionModel{{ID: "model", Enabled: true}}})
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.model.Draft = "保留"
	tester := composerTester(p, 900)
	connectionClick(t, tester, "Agent and model")
	index := -1
	for i, row := range p.composer.modelPicker.rows {
		if row.model == "model" {
			index = i
		}
	}
	if index < 0 {
		t.Fatal("enabled committed route missing")
	}
	if _, err = service.Connections.SetEnabled(t.Context(), entry.ID, entry.Revision, false); err != nil {
		t.Fatal(err)
	}
	p.chooseWorkspaceModel(index)
	tester.Frame()
	if !p.composer.targetOpen || !strings.Contains(p.composer.modelPicker.problem, "停用") || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(service.DefaultTarget) {
		t.Fatal("stale model silently selected another route")
	}
	if p.model.Draft != "保留" || p.selected != "" || service.Runtime.HasActive() {
		t.Fatal("stale route changed the draft or admitted a session")
	}
}

func TestWorkspaceModelPickerVirtualizesLargeCatalogAndHasEmptyStates(t *testing.T) {
	service, _ := fixture(t)
	service.Targets = nil
	p := NewNativePage(service)
	tester := composerTester(p, 900)
	connectionClick(t, tester, "Agent and model")
	if !tester.HasText("还没有可用模型，请先配置连接。") {
		t.Fatal("empty catalog has no explicit configuration path")
	}
	tester.Key(0, ui.KeyEscape)
	for i := range 16384 {
		target := service.DefaultTarget
		target.ModelID = fmt.Sprintf("catalog-%05d", i)
		service.Targets = append(service.Targets, target)
	}
	connectionClick(t, tester, "Agent and model")
	if len(p.composer.modelPicker.rows) != 16384 {
		t.Fatal("catalog was truncated")
	}
	// Native List builds only the viewport and overscan. Each rendered model
	// contributes two texts; 16,384 rows must not become 16,384 controls.
	if len(tester.Texts()) > 150 {
		t.Fatalf("large catalog built all controls: %d texts", len(tester.Texts()))
	}
	tester.Key(0, ui.KeyEnd)
	lastLabel := "Use alva · catalog-16383"
	if !tester.Focused(lastLabel) {
		t.Fatal("End did not materialize and focus the final virtual row")
	}
	tester.Key(0, ui.KeyUp)
	if !tester.Focused("Use alva · catalog-16382") {
		t.Fatal("Up did not navigate the virtual catalog")
	}
	connectionType(t, tester, "Search models", "nothing matches this")
	if !tester.HasText("没有匹配的模型") || len(p.composer.modelPicker.visible) != 0 {
		t.Fatal("search miss did not expose an empty result")
	}
	tester.Key(0, ui.KeyEnter)
	if !p.composer.targetOpen || p.selected != "" {
		t.Fatal("Enter on no results selected or created a session")
	}
	tester.Key(0, ui.KeyEscape)
	if p.composer.targetOpen || !tester.Focused("Agent and model") {
		t.Fatal("Escape did not restore the model trigger")
	}
}
