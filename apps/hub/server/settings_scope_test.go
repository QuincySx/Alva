// INPUT: Real loopback HTTP settings switches against isolated stores
// OUTPUT: Application-scope default, explicit session scope, per-request host approval and Chinese refusal reasons
// POS: [L7 Hub product] HTTP settings writes share the product SettingsService semantics with the desktop; no execution is started
package server

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
)

func refusal(t *testing.T, data []byte) string {
	t.Helper()
	var value struct{ Error, Message string }
	if json.Unmarshal(data, &value) != nil || value.Error != "Conflict" {
		t.Fatalf("not a conflict body: %s", data)
	}
	return value.Message
}

func TestSettingsSwitchesWriteApplicationScopeByDefaultAndSessionScopeOnlyWhenAsked(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	a, b := create(t, f, "first"), create(t, f, "second")
	prefix := "/v1/sessions/" + a.ID + "/settings/"
	settings := f.app.Service.Settings

	// Default scope is the application: it applies to every task and does not
	// turn this task into a "session only" one.
	body(t, f.request(t, "PUT", prefix+"subagents", []byte(`{"enabled":true}`), nil), 200)
	view, err := settings.Load(t.Context(), product.SettingsScope{App: true, SessionID: a.ID})
	if err != nil || !view.Capabilities.SubagentsEnabled {
		t.Fatalf("default write must land in application settings: %+v %v", view.Capabilities.SubagentsEnabled, err)
	}
	snapshot, err := f.app.Service.SessionSettings(t.Context(), a.ID)
	if err != nil || snapshot.OverridesCapabilities() || snapshot.SubagentsEnabled {
		t.Fatal("an application-scope write must not create a session override", snapshot, err)
	}
	other, err := f.app.Service.EffectiveSettings(t.Context(), b.ID)
	if err != nil || !other.SubagentsEnabled {
		t.Fatal("application scope must reach the other task", err)
	}
	body(t, f.request(t, "PUT", prefix+"memory", []byte(`{"enabled":true,"scope":"app"}`), nil), 200)
	view, _ = settings.Load(t.Context(), product.SettingsScope{App: true, SessionID: a.ID})
	if !view.Capabilities.PersistentMemory {
		t.Fatal("explicit app scope must behave as the default")
	}

	// Session scope is refused, with the reason, until the task keeps its own settings.
	reason := refusal(t, body(t, f.request(t, "PUT", prefix+"media", []byte(`{"enabled":true,"scope":"session"}`), nil), 409))
	if !strings.Contains(reason, "仅本会话") {
		t.Fatalf("reason does not explain how to proceed: %q", reason)
	}
	if _, err = settings.SetOverride(t.Context(), a.ID, true); err != nil {
		t.Fatal(err)
	}
	body(t, f.request(t, "PUT", prefix+"subagents", []byte(`{"enabled":false,"scope":"session"}`), nil), 200)
	own, err := f.app.Service.SessionSettings(t.Context(), a.ID)
	if err != nil || own.SubagentsEnabled || !own.OverridesCapabilities() {
		t.Fatal("explicit session scope must write the task's own snapshot", own, err)
	}
	view, _ = settings.Load(t.Context(), product.SettingsScope{App: true, SessionID: a.ID})
	if !view.Capabilities.SubagentsEnabled {
		t.Fatal("a session-scope write must leave application settings alone")
	}
}

func TestSettingsSwitchRefusalsAreExplicitAndChangeNothing(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	a := create(t, f, "refusals")
	prefix := "/v1/sessions/" + a.ID + "/settings/"
	// A host item cannot be switched on without approving host access in the same request.
	reason := refusal(t, body(t, f.request(t, "PUT", prefix+"browser", []byte(`{"enabled":true}`), nil), 409))
	if !strings.Contains(reason, "host_access_approved") {
		t.Fatalf("approval refusal must name the field: %q", reason)
	}
	view, err := f.app.Service.Settings.Load(t.Context(), product.SettingsScope{App: true, SessionID: a.ID})
	if err != nil || view.Capabilities.Browser.Enabled {
		t.Fatal("a refused enable must change nothing", err)
	}
	// An unknown scope is a malformed request, not a silent fallback.
	body(t, f.request(t, "PUT", prefix+"memory", []byte(`{"enabled":true,"scope":"everything"}`), nil), 400)
	// A server that only exists in a session draft is not an application switch.
	body(t, f.request(t, "PUT", prefix+"mcp", []byte(`{"servers":[{"id":"draft","command":"never-run-fixture"}]}`), nil), 200)
	reason = refusal(t, body(t, f.request(t, "PUT", prefix+"mcp/draft", []byte(`{"enabled":true,"host_access_approved":true}`), nil), 409))
	if !strings.Contains(reason, "session") {
		t.Fatalf("unknown-switch refusal must point at session scope: %q", reason)
	}
	// Skills stay session-scoped whatever the body says elsewhere.
	body(t, f.request(t, "PUT", prefix+"skills", []byte(`{"enabled":true}`), nil), 200)
	snapshot, _ := f.app.Service.SessionSettings(t.Context(), a.ID)
	if !snapshot.SkillsEnabled {
		t.Fatal("skills switch did not reach the session", snapshot)
	}
	var _ hub.Settings = snapshot
}

func TestHostItemApprovalIsBoundToTheFingerprintTheCallerRead(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	ctx := t.Context()
	s, err := f.app.Service.Create(ctx, "approvals", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	scope := product.SettingsScope{App: true, SessionID: s.ID}
	view, err := f.app.Service.Settings.Load(ctx, scope)
	if err != nil {
		t.Fatal(err)
	}
	desired := view.Capabilities
	desired.MCP = []hub.MCPServer{{ID: "local", Command: "/usr/bin/true", Env: map[string]string{"TOKEN": "env-secret-value"}}}
	if _, err = f.app.Service.Settings.Save(ctx, scope, product.SectionMCP, view.Fingerprints[product.SectionMCP], desired); err != nil {
		t.Fatal(err)
	}
	prefix := "/v1/sessions/" + s.ID + "/settings/"

	// The read-only application view hands out the fingerprint, never a saved secret.
	raw := body(t, f.request(t, "GET", prefix+"app", nil, nil), 200)
	if strings.Contains(string(raw), "env-secret-value") {
		t.Fatalf("application view leaked a saved secret: %s", raw)
	}
	var app product.SettingsView
	if json.Unmarshal(raw, &app) != nil {
		t.Fatal(string(raw))
	}
	item, ok := app.Item(hub.CapabilityMCP, "local")
	if !ok || item.Fingerprint == "" {
		t.Fatalf("view does not carry the item's fingerprint: %s", raw)
	}
	enabled := func() bool {
		view, _ := f.app.Service.Settings.Load(ctx, scope)
		got, _ := view.Item(hub.CapabilityMCP, "local")
		return got.Enabled
	}
	// No fingerprint, or a stale one, approves nothing and changes nothing.
	reason := refusal(t, body(t, f.request(t, "PUT", prefix+"mcp/local", []byte(`{"enabled":true,"host_access_approved":true}`), nil), 409))
	if !strings.Contains(reason, "fingerprint") {
		t.Fatalf("missing fingerprint must be explained: %q", reason)
	}
	reason = refusal(t, body(t, f.request(t, "PUT", prefix+"mcp/local", []byte(`{"enabled":true,"host_access_approved":true,"fingerprint":"stale"}`), nil), 409))
	if !strings.Contains(reason, "重新确认") || enabled() {
		t.Fatalf("a stale fingerprint must be refused before anything is written: %q enabled=%v", reason, enabled())
	}
	body(t, f.request(t, "PUT", prefix+"mcp/local", []byte(`{"enabled":true,"host_access_approved":true,"fingerprint":"`+item.Fingerprint+`"}`), nil), 200)
	if !enabled() {
		t.Fatal("the exact fingerprint must enable and approve")
	}
}
