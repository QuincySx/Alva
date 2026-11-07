// INPUT: Canonical row-option geometry, real connection facts and headless native controls.
// OUTPUT: Side placement, 16px insets, truthful disabled capability fields and explicit-save regressions.
// POS: [L7 Hub UI testing] No GUI process, network, model request or implicit session creation.
//go:build desktop

package hubdesktop

import (
	"math"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func workspaceOptionsFixture(t *testing.T, width, height int) (*NativePage, *ui.Tester, hub.Target) {
	return workspaceOptionsFixtureWithDescriptor(t, width, height, true)
}

func workspaceOptionsFixtureWithDescriptor(t *testing.T, width, height int, register bool) (*NativePage, *ui.Tester, hub.Target) {
	t.Helper()
	service, _ := connectionsFixture(t)
	if register {
		if err := service.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: true, SupportsSystemPrompt: true, SupportsMaxTurns: true}}); err != nil {
			t.Fatal(err)
		}
	}
	connection, err := service.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "Declared connection", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/chat", Auth: "none", Enabled: true,
		Models: []hub.ConnectionModel{{ID: "declared-model", Name: "Declared model", Enabled: true, ImageCapable: true}}})
	if err != nil {
		t.Fatal(err)
	}
	route := connection.Target(connection.Models[0])
	other := route
	other.EngineID = "pi"
	service.Targets = append(service.Targets, other)
	p := NewNativePage(service)
	p.composer.target, p.model.Draft = route, "preserve the draft"
	tester := ui.NewTester(workspaceStaticView(p), width, height)
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Model options: "+route.ModelID)
	return p, tester, route
}

func TestWorkspaceModelOptionsMatchCanonicalFieldsAndOutsidePlacement(t *testing.T) {
	p, tester, target := workspaceOptionsFixture(t, 1440, 960)
	rect := func(label string) ui.Rect {
		t.Helper()
		box, ok := tester.Find(label)
		if !ok {
			t.Fatal("missing option component", label)
		}
		return box
	}
	close := func(a, b float32) bool { return math.Abs(float64(a-b)) <= .5 }
	panel, picker := rect("Model options"), rect("Composer menu")
	if !close(panel.W, 300) || !close(picker.X-panel.X-panel.W, 12) || panel.X < 4 || panel.Y < 4 || panel.Y+panel.H > 956 {
		t.Fatal("options are not 300px wide outside the picker with a 12px gap", panel, picker)
	}
	if !close(panel.H, 471.5) {
		t.Fatal("canonical option content is not 495px high", panel)
	}
	components := []struct {
		label  string
		height float32
	}{
		{"Model options header", 32}, {"Model options source", 16.5}, {"Model Agent field", 59.5},
		{"Model options effort title", 19.5}, {"Model effort segments", 36}, {"Model context field", 59.5},
		{"Model vision setting", 40}, {"Edit model defaults: " + target.ModelID, 32}, {"Model options scope", 16.5},
	}
	previous := ui.Rect{}
	for i, component := range components {
		box := rect(component.label)
		if !close(box.X-panel.X, 16) || !close(box.H, component.height) {
			t.Fatal("canonical field inset or height changed", component.label, box, panel)
		}
		if i == 0 && !close(box.Y-panel.Y, 16) || i > 0 && !close(box.Y-previous.Y-previous.H, 16) {
			t.Fatal("canonical gap is not 16px", component.label, box, previous)
		}
		previous = box
	}
	if !close(panel.Y+panel.H-previous.Y-previous.H, 16) {
		t.Fatal("border consumed bottom padding", panel, previous)
	}
	star := rect("Favorite model in options: " + target.ModelID)
	if !close(star.W, 28) || !close(star.H, 28) || !close(panel.X+panel.W-star.X-star.W, 16) {
		t.Fatal("header star is not centered in the canonical 32px slot", star)
	}
	if tester.HasText("128000") || !p.composer.modelPicker.inlineOptions.vision {
		t.Fatal("capability or scope presentation invented unsupported facts")
	}
	connectionClick(t, tester, "Model context limit")
	if tester.Focused("Model context limit") {
		t.Fatal("undeclared Context limit became editable")
	}
	connectionClick(t, tester, "Model vision input")
	if p.Service.Connections.ModelPreference(target).Revision != 0 || !p.composer.modelPicker.inlineOptions.vision {
		t.Fatal("disabled capability controls changed facts")
	}
}

func TestWorkspaceModelOptionsAgentKeyboardOnlyChangesDraft(t *testing.T) {
	p, tester, target := workspaceOptionsFixture(t, 1440, 960)
	connectionClick(t, tester, "Model effort: 高")
	connectionClick(t, tester, "Model Agent")
	tester.Key(0, ui.KeyHome)
	tester.Key(0, ui.KeyEnter)
	if p.composer.modelPicker.rows[p.composer.modelPicker.visible[p.composer.modelPicker.active]].routes[0].EngineID != "pi" {
		t.Fatal("fixture route order changed")
	}
	var chosen string
	for _, row := range p.composer.modelPicker.rows {
		if row.model == target.ModelID {
			chosen = row.routes[row.chosen].EngineID
		}
	}
	if chosen != "pi" || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(target) || p.Service.Connections.ModelPreference(target).Revision != 0 {
		t.Fatal("Agent dropdown selected a model or changed committed defaults", chosen)
	}
	tester.Key(0, ui.KeyEscape)
	if p.composer.modelPicker.config != "" || !p.composer.targetOpen || !tester.Focused("Model options: "+target.ModelID) {
		t.Fatal("Escape did not restore row-options trigger")
	}
	tester.Key(0, ui.KeyEscape)
	if p.composer.targetOpen || !tester.Focused("Agent and model") {
		t.Fatal("Escape did not restore the model trigger")
	}
	connectionClick(t, tester, "Agent and model")
	for _, row := range p.composer.modelPicker.rows {
		if row.model == target.ModelID && row.routes[row.chosen].EngineID != "alva" {
			t.Fatal("closing committed the unselected Agent draft")
		}
	}
	connectionClick(t, tester, "Model options: "+target.ModelID)
	if p.composer.modelPicker.inlineOptions.effortDirty || p.composer.modelPicker.inlineOptions.effort != "" {
		t.Fatal("closing the picker retained an uncommitted effort draft")
	}
}

func TestWorkspaceModelOptionsEffortAndStarUseExplicitSharedOwner(t *testing.T) {
	p, tester, target := workspaceOptionsFixture(t, 1440, 960)
	connectionClick(t, tester, "Model effort: 高")
	if p.Service.Connections.ModelPreference(target).Options.Generation.ReasoningEffort != "" || !p.composer.modelPicker.inlineOptions.effortDirty {
		t.Fatal("segment browsing wrote defaults")
	}
	connectionClick(t, tester, "Favorite model in options: "+target.ModelID)
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if pref := p.Service.Connections.ModelPreference(target); !pref.Favorite || pref.Options.Generation.ReasoningEffort != "" || !p.composer.targetOpen {
		t.Fatal("header star selected a model or committed an effort draft", pref)
	}
	connectionClick(t, tester, "Edit model defaults: "+target.ModelID)
	if !p.composer.modelPicker.defaults.open || p.composer.modelPicker.defaults.effort != "high" {
		t.Fatal("model settings did not receive the explicit effort draft")
	}
	connectionClick(t, tester, "Save model defaults")
	waitWorkspace(t, p, tester, func() bool { return !p.composer.modelPicker.saving })
	if pref := p.Service.Connections.ModelPreference(target); !pref.Favorite || pref.Options.Generation.ReasoningEffort != "high" {
		t.Fatal("explicit save did not persist actual model defaults", pref)
	}
	sessions, err := p.Service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || p.model.Draft != "preserve the draft" || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(target) || p.Service.Runtime.HasActive() {
		t.Fatal("row option actions sent or created a session", err)
	}
}

func TestWorkspaceModelOptionsPlacementFlipsAndUsesOutsideSpace(t *testing.T) {
	for _, test := range []struct {
		name     string
		viewport float32
		picker   ui.Rect
		width    float32
		left     bool
	}{
		{"left", 1440, ui.Rect{X: 600, W: 640}, 300, true},
		{"flip-right", 1440, ui.Rect{X: 40, W: 640}, 300, false},
		{"narrow-left", 960, ui.Rect{X: 240, W: 640}, 224, true},
		{"narrow-right", 960, ui.Rect{X: 40, W: 640}, 264, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			width, left := workspaceModelOptionsWidth(test.viewport, test.picker)
			if width != test.width || left != test.left {
				t.Fatal("outside placement changed", width, left)
			}
		})
	}
	_, tester, _ := workspaceOptionsFixture(t, 960, 400)
	panel, ok := tester.Find("Model options")
	picker, found := tester.Find("Composer menu")
	if !ok || !found || panel.X < 4 || panel.Y < 4 || panel.X+panel.W > 956 || panel.Y+panel.H > 396 || panel.X+panel.W > picker.X-11.5 && panel.X < picker.X+picker.W+11.5 {
		t.Fatal("short viewport options overlap the picker or escape the viewport", panel, picker)
	}
}

func TestWorkspaceModelOptionsUseSelectedAgentGenerationDeclaration(t *testing.T) {
	for _, tc := range []struct {
		name, agent string
		supported   bool
	}{
		{"extension-enabled", "extension-agent", true},
		{"extension-disabled", "extension-agent", false},
		{"builtin-explicitly-disabled", "alva", false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			p, tester, target := workspaceOptionsFixtureWithDescriptor(t, 1440, 960, false)
			if err := p.Service.Runtime.RegisterDescriptor(tc.agent, engine.Descriptor{
				Runtime: &settingsRunningEngine{}, Config: engine.ConfigSurface{ProviderProtocol: "native", SupportsGeneration: tc.supported},
			}); err != nil {
				t.Fatal(err)
			}
			for i := range p.composer.modelPicker.rows {
				row := &p.composer.modelPicker.rows[i]
				if row.model == target.ModelID {
					route := target
					route.EngineID = tc.agent
					row.routes = append(row.routes, route)
					row.chosen = len(row.routes) - 1
				}
			}
			tester.Frame()
			connectionClick(t, tester, "Model effort: 高")
			if p.composer.modelPicker.inlineOptions.effortDirty != tc.supported {
				t.Fatal("effort control ignored selected Agent declaration")
			}
			connectionClick(t, tester, "Edit model defaults: "+target.ModelID)
			connectionClick(t, tester, "Model default temperature")
			if tester.Focused("Model default temperature") != tc.supported {
				t.Fatal("options and defaults disagree about selected Agent capability")
			}
			if p.Service.Runtime.HasActive() || p.Service.Connections.ModelPreference(target).Revision != 0 {
				t.Fatal("checking capabilities executed or saved a model")
			}
		})
	}
}
