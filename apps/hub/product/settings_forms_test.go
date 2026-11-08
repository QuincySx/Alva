// INPUT: Real private product SQLite through the shared product fixture
// OUTPUT: Scope resolution, workspace approval, redaction, CAS and override regressions for SettingsService
// POS: [L7 Hub product] Settings edits never grant host access implicitly or leak saved secrets to the form
package product

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func settingsSession(t *testing.T, s *HubService, title string) hub.Session {
	t.Helper()
	session, err := s.Create(context.Background(), title, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return session
}

func appScope(session string) SettingsScope { return SettingsScope{App: true, SessionID: session} }

func saveAppMCP(t *testing.T, s *HubService, scope SettingsScope, servers ...hub.MCPServer) SettingsView {
	t.Helper()
	view, err := s.Settings.Load(context.Background(), scope)
	if err != nil {
		t.Fatal(err)
	}
	desired := view.Capabilities
	desired.MCP = servers
	view, err = s.Settings.Save(context.Background(), scope, SectionMCP, view.Fingerprints[SectionMCP], desired)
	if err != nil {
		t.Fatal(err)
	}
	return view
}

func effectiveMCP(t *testing.T, s *HubService, id string) []hub.MCPServer {
	t.Helper()
	settings, err := s.EffectiveSettings(context.Background(), id)
	if err != nil {
		t.Fatal(err)
	}
	return settings.MCP
}

func TestApplicationSettingsReachNewSessionsOnlyWithExactWorkspaceApproval(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	first, second := settingsSession(t, s, "first"), settingsSession(t, s, "second")
	server := hub.MCPServer{ID: "files", Name: "文件", Command: "/bin/echo", Env: map[string]string{"TOKEN": "s3cret"}}
	saveAppMCP(t, s, appScope(""), server)
	if got := effectiveMCP(t, s, first.ID); len(got) != 1 || got[0].Enabled || got[0].HostAccessApproved {
		t.Fatalf("saved application server must reach new sessions disabled: %+v", got)
	}
	// Enabling with the first task selected approves only its workspace.
	view, err := s.Settings.SetEnabled(ctx, appScope(first.ID), hub.CapabilityMCP, "files", true)
	if err != nil {
		t.Fatal(err)
	}
	if item, _ := view.Item(hub.CapabilityMCP, "files"); !item.Enabled || !item.Approved || item.NeedsApproval {
		t.Fatalf("first workspace not approved: %+v", item)
	}
	if got := effectiveMCP(t, s, first.ID); len(got) != 1 || !got[0].Enabled || !got[0].HostAccessApproved || got[0].Env["TOKEN"] != "s3cret" {
		t.Fatalf("approved workspace does not run the exact snapshot: %+v", got)
	}
	if got := effectiveMCP(t, s, second.ID); got[0].Enabled || got[0].HostAccessApproved {
		t.Fatal("approval leaked to another workspace")
	}
	other, err := s.Settings.Load(ctx, appScope(second.ID))
	if err != nil {
		t.Fatal(err)
	}
	if other.PendingApprovals() != 1 {
		t.Fatal("second workspace must show a pending approval", other.Items)
	}
	// A configuration change invalidates the approval; the intent stays.
	server.Args = []string{"--changed"}
	server.Env = map[string]string{"TOKEN": SecretKept}
	view = saveAppMCP(t, s, appScope(first.ID), server)
	if item, _ := view.Item(hub.CapabilityMCP, "files"); !item.Enabled || item.Approved || !item.NeedsApproval {
		t.Fatalf("changed configuration kept its approval: %+v", item)
	}
	if got := effectiveMCP(t, s, first.ID); got[0].Enabled || got[0].HostAccessApproved || got[0].Env["TOKEN"] != "s3cret" {
		t.Fatalf("changed configuration still runs or lost its secret: %+v", got)
	}
	if _, err = s.Settings.Approve(ctx, first.ID, hub.CapabilityMCP, "files", true); err != nil {
		t.Fatal(err)
	}
	if got := effectiveMCP(t, s, first.ID); !got[0].Enabled || !got[0].HostAccessApproved {
		t.Fatal("explicit re-approval did not apply")
	}
	// Renaming is presentation only and keeps the approval.
	server.Name = "项目文件"
	saveAppMCP(t, s, appScope(first.ID), server)
	if got := effectiveMCP(t, s, first.ID); !got[0].Enabled {
		t.Fatal("display rename revoked the approval")
	}
	if _, err = s.Settings.Approve(ctx, first.ID, hub.CapabilityMCP, "files", false); err != nil {
		t.Fatal(err)
	}
	if got := effectiveMCP(t, s, first.ID); got[0].Enabled {
		t.Fatal("withdrawn approval still runs")
	}
}

func TestApplicationSettingsNeverStoreApprovalFlags(t *testing.T) {
	s, _ := fixture(t)
	store := s.Store.(hub.AppSettingsStore)
	value, err := store.AppSettings(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	approved := hub.Capabilities{MCP: []hub.MCPServer{{ID: "x", Command: "/bin/echo", Enabled: true, HostAccessApproved: true}}}
	saved, err := store.SaveAppSettings(context.Background(), value.Revision, approved)
	if err != nil {
		t.Fatal(err)
	}
	if !saved.Capabilities.MCP[0].Enabled || saved.Capabilities.MCP[0].HostAccessApproved {
		t.Fatal("application settings stored a host approval", saved.Capabilities.MCP[0])
	}
	if _, err = store.SaveAppSettings(context.Background(), value.Revision, approved); !errors.Is(err, hub.ErrAppSettingsConflict) {
		t.Fatal("stale application revision was accepted", err)
	}
	session := settingsSession(t, s, "workspace")
	if got := effectiveMCP(t, s, session.ID); got[0].Enabled {
		t.Fatal("application intent ran without a workspace approval")
	}
}

func TestLegacySessionSettingsStayAnOverride(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	legacy := settingsSession(t, s, "legacy")
	if err := s.Store.SetSubagentsEnabled(ctx, legacy.ID, true); err != nil {
		t.Fatal(err)
	}
	saveAppMCP(t, s, appScope(""), hub.MCPServer{ID: "app", Command: "/bin/echo"})
	settings, err := s.EffectiveSettings(ctx, legacy.ID)
	if err != nil {
		t.Fatal(err)
	}
	if !settings.SubagentsEnabled || len(settings.MCP) != 0 {
		t.Fatalf("legacy session lost its own snapshot: %+v", settings)
	}
	view, err := s.Settings.Load(ctx, appScope(legacy.ID))
	if err != nil || !view.Override {
		t.Fatal("legacy session not reported as an override", err)
	}
	if _, err = s.Settings.SetOverride(ctx, legacy.ID, false); err != nil {
		t.Fatal(err)
	}
	if got := effectiveMCP(t, s, legacy.ID); len(got) != 1 {
		t.Fatal("returning to application settings did not apply them", got)
	}
}

func TestSettingsFormNeverReturnsSavedSecrets(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	view := saveAppMCP(t, s, appScope(""), hub.MCPServer{ID: "remote", Transport: hub.MCPTransportHTTP, URL: "https://mcp.example.test/mcp", Headers: map[string]string{"X-Api-Key": "header-secret"}},
		hub.MCPServer{ID: "local", Command: "/bin/echo", Env: map[string]string{"TOKEN": "env-secret"}})
	raw := strings.Join([]string{view.Capabilities.MCP[0].Headers["X-Api-Key"], view.Capabilities.MCP[1].Env["TOKEN"]}, "|")
	if strings.Contains(raw, "header-secret") || strings.Contains(raw, "env-secret") || view.Capabilities.MCP[0].Headers["X-Api-Key"] != SecretKept {
		t.Fatal("form received a saved secret", raw)
	}
	// A placeholder for a key that was never saved must be re-entered.
	desired := view.Capabilities
	desired.MCP[1].Env = map[string]string{"TOKEN": SecretKept, "NEW": SecretKept}
	_, err := s.Settings.Save(ctx, appScope(""), SectionMCP, view.Fingerprints[SectionMCP], desired)
	var problems FieldErrors
	if !errors.As(err, &problems) || problems.For("mcp.local.env") == "" {
		t.Fatal("unknown placeholder was accepted", err)
	}
	// Replacing a value keeps the others.
	desired.MCP[1].Env = map[string]string{"TOKEN": SecretKept, "NEW": "plain"}
	if _, err = s.Settings.Save(ctx, appScope(""), SectionMCP, view.Fingerprints[SectionMCP], desired); err != nil {
		t.Fatal(err)
	}
	value, err := s.Store.(hub.AppSettingsStore).AppSettings(ctx)
	if err != nil || value.Capabilities.MCP[1].Env["TOKEN"] != "env-secret" || value.Capabilities.MCP[1].Env["NEW"] != "plain" || value.Capabilities.MCP[0].Headers["X-Api-Key"] != "header-secret" {
		t.Fatal("kept or replaced secrets were not saved exactly", err)
	}
}

func TestSettingsSaveDetectsConcurrentSectionEdits(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	view, err := s.Settings.Load(ctx, appScope(""))
	if err != nil {
		t.Fatal(err)
	}
	stale := view.Fingerprints[SectionBrowser]
	desired := view.Capabilities
	desired.Browser = hub.BrowserSettings{Executable: "/Applications/Browser", Headless: true}
	if _, err = s.Settings.Save(ctx, appScope(""), SectionBrowser, stale, desired); err != nil {
		t.Fatal(err)
	}
	// Another section saved meanwhile does not conflict.
	desired.MediaDecoder = hub.MediaDecoder{FFmpeg: "/opt/ffmpeg", FFprobe: "/opt/ffprobe"}
	if _, err = s.Settings.Save(ctx, appScope(""), SectionMedia, view.Fingerprints[SectionMedia], desired); err != nil {
		t.Fatal("independent section conflicted", err)
	}
	desired.Browser.Headless = false
	if _, err = s.Settings.Save(ctx, appScope(""), SectionBrowser, stale, desired); !errors.Is(err, ErrSettingsConflict) {
		t.Fatal("stale section edit overwrote a newer save", err)
	}
	// Toggling a switch does not invalidate an open configuration form.
	fresh, err := s.Settings.Load(ctx, appScope(""))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Settings.SetEnabled(ctx, appScope(""), hub.CapabilityPlanning, "", true); err != nil {
		t.Fatal(err)
	}
	desired = fresh.Capabilities
	desired.Tools.Web.TimeoutMS = 1000
	saved, err := s.Settings.Save(ctx, appScope(""), SectionTools, fresh.Fingerprints[SectionTools], desired)
	if err != nil {
		t.Fatal("switch change conflicted with a configuration form", err)
	}
	if item, _ := saved.Item(hub.CapabilityPlanning, ""); !item.Enabled {
		t.Fatal("configuration save reverted a switch")
	}
}

func TestSettingsFieldErrorsAreChinese(t *testing.T) {
	s, _ := fixture(t)
	view, err := s.Settings.Load(context.Background(), appScope(""))
	if err != nil {
		t.Fatal(err)
	}
	desired := view.Capabilities
	desired.MCP = []hub.MCPServer{{ID: "bad id", Command: "relative/tool"}, {ID: "oauth", Transport: hub.MCPTransportHTTP, URL: "http://mcp.example.test/mcp", Auth: hub.MCPAuthOAuth}}
	_, err = s.Settings.Save(context.Background(), appScope(""), SectionMCP, view.Fingerprints[SectionMCP], desired)
	var problems FieldErrors
	if !errors.As(err, &problems) || problems.For("mcp.bad id.id") == "" || problems.For("mcp.bad id.command") == "" || !strings.Contains(problems.For("mcp.oauth.auth"), "https") {
		t.Fatal("invalid fields were not reported", err)
	}
	// Plain HTTP without OAuth is allowed on any host (product decision).
	desired.MCP = []hub.MCPServer{{ID: "lan", Transport: hub.MCPTransportHTTP, URL: "http://192.168.1.20:8080/mcp"}}
	if _, err = s.Settings.Save(context.Background(), appScope(""), SectionMCP, view.Fingerprints[SectionMCP], desired); err != nil {
		t.Fatal("plain HTTP MCP address was rejected", err)
	}
}

func TestSessionOverrideStartsFromWhatWouldRunAndRevokesChangedItems(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	session := settingsSession(t, s, "override")
	saveAppMCP(t, s, appScope(session.ID), hub.MCPServer{ID: "a", Command: "/bin/echo"}, hub.MCPServer{ID: "b", Command: "/bin/cat"})
	if _, err := s.Settings.SetEnabled(ctx, appScope(session.ID), hub.CapabilityMCP, "a", true); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Settings.SetEnabled(ctx, appScope(""), hub.CapabilityMCP, "b", true); err != nil {
		t.Fatal(err)
	}
	view, err := s.Settings.SetOverride(ctx, session.ID, true)
	if err != nil {
		t.Fatal(err)
	}
	a, _ := view.Item(hub.CapabilityMCP, "a")
	b, _ := view.Item(hub.CapabilityMCP, "b")
	if view.App || !view.Override || !a.Enabled || !a.Approved || b.Enabled {
		t.Fatalf("override did not copy exactly what would run: %+v %+v", a, b)
	}
	scope := SettingsScope{SessionID: session.ID}
	desired := view.Capabilities
	desired.MCP[1].Args = []string{"changed"}
	view, err = s.Settings.Save(ctx, scope, SectionMCP, view.Fingerprints[SectionMCP], desired)
	if err != nil {
		t.Fatal(err)
	}
	if a, _ = view.Item(hub.CapabilityMCP, "a"); !a.Enabled {
		t.Fatal("unchanged approved server was switched off")
	}
	desired = view.Capabilities
	desired.MCP[0].Args = []string{"changed"}
	view, err = s.Settings.Save(ctx, scope, SectionMCP, view.Fingerprints[SectionMCP], desired)
	if err != nil {
		t.Fatal(err)
	}
	if a, _ = view.Item(hub.CapabilityMCP, "a"); a.Enabled || a.Approved {
		t.Fatal("changed session snapshot kept its approval")
	}
	// Application edits no longer reach the overriding session.
	saveAppMCP(t, s, appScope(""))
	if got := effectiveMCP(t, s, session.ID); len(got) != 2 {
		t.Fatal("application edit changed an overriding session", got)
	}
}

func TestSettingsResetSwitchesListsOffAndClearsScalars(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	session := settingsSession(t, s, "reset")
	saveAppMCP(t, s, appScope(session.ID), hub.MCPServer{ID: "a", Command: "/bin/echo"})
	if _, err := s.Settings.SetEnabled(ctx, appScope(session.ID), hub.CapabilityMCP, "a", true); err != nil {
		t.Fatal(err)
	}
	view, err := s.Settings.Reset(ctx, appScope(session.ID), SectionMCP)
	if err != nil {
		t.Fatal(err)
	}
	if a, _ := view.Item(hub.CapabilityMCP, "a"); a.Enabled || len(view.Capabilities.MCP) != 1 {
		t.Fatal("reset deleted servers or kept them on", view.Capabilities.MCP)
	}
	desired := view.Capabilities
	desired.Browser = hub.BrowserSettings{Executable: "/Applications/Browser"}
	if view, err = s.Settings.Save(ctx, appScope(""), SectionBrowser, view.Fingerprints[SectionBrowser], desired); err != nil {
		t.Fatal(err)
	}
	if view, err = s.Settings.Reset(ctx, appScope(""), SectionBrowser); err != nil || view.Capabilities.Browser.Executable != "" {
		t.Fatal("browser reset did not restore defaults", err)
	}
}
