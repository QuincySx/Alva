// INPUT: Private product SQLite, v8 databases and application/workspace settings
// OUTPUT: v9 migration, revision CAS, approval persistence, override transitions and reopen regressions
// POS: [L7 SQLite adapter] Application intent and approvals survive restarts; approvals never ride in intent
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestApplicationSettingsMigrationCASAndApprovalsPersist(t *testing.T) {
	ctx := context.Background()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	// Simulate a v8 database: the v9 tables are created on reopen.
	for _, statement := range []string{`DROP TABLE hub_workspace_approvals`, `DROP TABLE hub_app_settings`, `PRAGMA user_version=8`} {
		if _, err = store.db.ExecContext(ctx, statement); err != nil {
			t.Fatal(err)
		}
	}
	store.Close()
	if store, err = Open(ctx, directory); err != nil {
		t.Fatal("v8 database did not migrate", err)
	}
	var version int
	if err = store.db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil || version != 11 {
		t.Fatal("schema not at v11", version, err)
	}
	value, err := store.AppSettings(ctx)
	if err != nil || value.Revision != 1 || !value.Capabilities.Empty() {
		t.Fatal("migration did not seed empty application settings", value, err)
	}
	next := hub.Capabilities{MCP: []hub.MCPServer{{ID: "a", Command: "/bin/echo", Enabled: true}}, SubagentsEnabled: true}
	saved, err := store.SaveAppSettings(ctx, value.Revision, next)
	if err != nil || saved.Revision != 2 {
		t.Fatal(err)
	}
	if _, err = store.SaveAppSettings(ctx, value.Revision, next); !errors.Is(err, hub.ErrAppSettingsConflict) {
		t.Fatal("stale revision accepted", err)
	}
	workspace := t.TempDir()
	approval := hub.WorkspaceApproval{Workspace: workspace, Kind: hub.CapabilityMCP, Item: "a", Fingerprint: saved.Capabilities.Items()[0].Fingerprint}
	if err = store.SaveWorkspaceApproval(ctx, approval); err != nil {
		t.Fatal(err)
	}
	if err = store.SaveWorkspaceApproval(ctx, hub.WorkspaceApproval{Workspace: "relative", Kind: hub.CapabilityMCP, Item: "a", Fingerprint: approval.Fingerprint}); err == nil {
		t.Fatal("relative workspace approval accepted")
	}
	store.Close()
	if store, err = Open(ctx, directory); err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	reopened, err := store.AppSettings(ctx)
	approvals, approvalsErr := store.WorkspaceApprovals(ctx, workspace)
	if err != nil || approvalsErr != nil || reopened.Revision != 2 || !reopened.Capabilities.SubagentsEnabled || len(approvals) != 1 || approvals[0].Fingerprint != approval.Fingerprint {
		t.Fatal("application settings or approvals lost on reopen", err, approvalsErr)
	}
	if err = store.DeleteWorkspaceApproval(ctx, workspace, hub.CapabilityMCP, "a"); err != nil {
		t.Fatal(err)
	}
	if approvals, _ = store.WorkspaceApprovals(ctx, workspace); len(approvals) != 0 {
		t.Fatal("withdrawn approval kept")
	}
}

func TestSessionOverrideTransitions(t *testing.T) {
	ctx := context.Background()
	store, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	session, err := store.CreateSession(ctx, "override", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err = store.ReplaceSessionCapabilities(ctx, session.ID, hub.Capabilities{}); err == nil {
		t.Fatal("session edit accepted without an override")
	}
	initial := hub.Capabilities{MCP: []hub.MCPServer{{ID: "a", Command: "/bin/echo", Enabled: true, HostAccessApproved: true}}}
	if err = store.SetCapabilityOverride(ctx, session.ID, true, initial); err != nil {
		t.Fatal(err)
	}
	if err = store.SetCapabilityOverride(ctx, session.ID, true, initial); err == nil {
		t.Fatal("second override replaced the session's own settings")
	}
	if err = store.SetSessionCapabilityEnabled(ctx, session.ID, hub.CapabilityMCP, "a", false); err != nil {
		t.Fatal(err)
	}
	if err = store.SetSessionCapabilityEnabled(ctx, session.ID, hub.CapabilityMCP, "a", true); err != nil {
		t.Fatal(err)
	}
	value, err := store.Settings(ctx, session.ID)
	if err != nil || !value.CapabilityOverride || !value.MCP[0].Enabled || !value.MCP[0].HostAccessApproved {
		t.Fatal("explicit session enable did not approve", value.MCP, err)
	}
	if err = store.SetCapabilityOverride(ctx, session.ID, false, hub.Capabilities{}); err != nil {
		t.Fatal(err)
	}
	if value, err = store.Settings(ctx, session.ID); err != nil || value.OverridesCapabilities() {
		t.Fatal("returning to application settings kept the session copy", value.MCP, err)
	}
}
