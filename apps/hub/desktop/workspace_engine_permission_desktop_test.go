// INPUT: Two registered engines that differ only in their declared PermissionCallback capability
// OUTPUT: The composer permission chip says an engine does not ask exactly when its capability is absent, never by engine name
// POS: [L7 Hub UI tests] Capability-derived "此引擎不会询问" marker; no real engine or account
//go:build desktop

package hubdesktop

import (
	"context"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type capabilityFixtureEngine struct{ caps engine.Capabilities }

func (capabilityFixtureEngine) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	return nil
}
func (capabilityFixtureEngine) Cancel(context.Context, string) error { return nil }
func (capabilityFixtureEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return nil
}
func (e capabilityFixtureEngine) Capabilities() engine.Capabilities { return e.caps }

func TestWorkspaceEnginePermissionMarkerFollowsDeclaredCapability(t *testing.T) {
	s, _ := connectionsFixture(t)
	for id, caps := range map[string]engine.Capabilities{
		"asks-fixture":   {Streaming: true, PermissionCallback: true},
		"silent-fixture": {Streaming: true},
	} {
		if err := s.Runtime.RegisterDescriptor(id, engine.Descriptor{Runtime: capabilityFixtureEngine{caps}, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
			t.Fatal(err)
		}
	}
	p := NewNativePage(s)
	label, description := p.workspaceEnginePermission("silent-fixture")
	if label != "此引擎不会询问" || !strings.Contains(description, "无法在执行前拦截") {
		t.Fatalf("an engine without a permission callback must be marked as not asking: %q %q", label, description)
	}
	label, description = p.workspaceEnginePermission("asks-fixture")
	if label == "此引擎不会询问" || !strings.Contains(description, "审批卡") {
		t.Fatalf("an engine that forwards permission requests must not carry the marker: %q %q", label, description)
	}
	// The permission description path used by the composer chip agrees.
	if got, _ := p.workspacePermissionDescription("silent-fixture"); got != "此引擎不会询问" {
		t.Fatalf("composer chip does not use the capability marker: %q", got)
	}
}

func TestWorkspaceComposerChipShowsTheMarkerForANonAskingEngine(t *testing.T) {
	s, _ := connectionsFixture(t)
	for id, caps := range map[string]engine.Capabilities{"asks-fixture": {Streaming: true, PermissionCallback: true}, "silent-fixture": {Streaming: true}} {
		if err := s.Runtime.RegisterDescriptor(id, engine.Descriptor{Runtime: capabilityFixtureEngine{caps}, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
			t.Fatal(err)
		}
	}
	for id, want := range map[string]bool{"silent-fixture": true, "asks-fixture": false} {
		p := NewNativePage(s)
		p.composer.target = hub.Target{EngineID: id, ModelID: "engine-default", ProviderID: "engine-default", AccountSource: "fixture account", BillingRoute: "fixture"}
		tester := ui.NewTester(workspaceStaticView(p), 960, 680)
		if _, ok := tester.Find("Permission policy"); !ok {
			t.Fatal("composer has no permission chip", tester.Texts())
		}
		if got := tester.HasText("此引擎不会询问"); got != want {
			t.Fatalf("%s: marker visible=%v, want %v; texts=%q", id, got, want, tester.Texts())
		}
		saveChartScreenshot(t, tester, "engine-permission-"+id+".png")
	}
}
