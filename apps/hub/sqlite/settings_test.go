// INPUT: private product store and explicit MCP snapshots
// OUTPUT: Disabled imports, separate approval, scope, next-turn immutability and persistence regressions
// POS: [L7 Hub product] Product configuration does not import protocol/runtime process behavior
package sqlite

import (
	"context"
	"github.com/QuincySx/Alva/apps/hub"
	"path/filepath"
	"testing"
)

func TestMCPSettingsRequireSeparateApprovalAndPersistPrivately(t *testing.T) {
	ctx := context.Background()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	product, err := store.CreateSession(ctx, "settings", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	settings := hub.Settings{Version: 1, SessionID: product.ID, MCP: []hub.MCPServer{{ID: "explicit", Command: "does-not-exist-fixture", Enabled: true, HostAccessApproved: true, Env: map[string]string{"EXPLICIT": "fixture"}}}}
	if err = store.SaveSettings(ctx, settings); err != nil {
		t.Fatal(err)
	}
	if !settings.MCP[0].Enabled {
		t.Fatal("save mutated caller snapshot")
	}
	saved, err := store.Settings(ctx, product.ID)
	if err != nil || saved.MCP[0].Enabled || saved.MCP[0].HostAccessApproved {
		t.Fatal("edited import self-approved", err)
	}
	if err = store.SetMCPEnabled(ctx, product.ID, "explicit", true, false); err == nil {
		t.Fatal("enable skipped host approval")
	}
	if err = store.SetMCPEnabled(ctx, product.ID, "explicit", true, true); err != nil {
		t.Fatal(err)
	}
	if err = store.SetMCPEnabled(ctx, "foreign", "explicit", true, true); err == nil {
		t.Fatal("foreign product config accepted")
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	saved, err = store.Settings(ctx, product.ID)
	if err != nil || !saved.MCP[0].Enabled || saved.MCP[0].Env["EXPLICIT"] != "fixture" {
		t.Fatal("approved snapshot not durable", err)
	}
	if _, err = store.BeginTurn(ctx, product.ID, "pending"); err != nil {
		t.Fatal(err)
	}
	if err = store.SetMCPEnabled(ctx, product.ID, "explicit", false, false); err == nil {
		t.Fatal("running immutable snapshot was changed")
	}
}
