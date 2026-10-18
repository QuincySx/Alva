// INPUT: Private product settings transactions, real Turn claims and reopened SQLite
// OUTPUT: Optional capability default-off, import grant clearing, separate consent and immutability regressions
// POS: [L7 Hub product] Tool settings preserve unrelated approved configuration and survive restart without execution.
package sqlite

import (
	"context"
	"github.com/QuincySx/Alva/apps/hub"
	"path/filepath"
	"testing"
)

func TestOptionalToolSettingsAreIndependentDurableAndIdleOnly(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if s != nil {
			s.Close()
		}
	}()
	p, err := s.CreateSession(ctx, "tools", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	value, err := s.Settings(ctx, p.ID)
	if err != nil || value.Tools.Planning || value.Tools.Shell.Enabled || value.Tools.Web.Enabled {
		t.Fatal("tools were not default off", err)
	}
	draft := hub.ToolSettings{Planning: true, Notebook: true, Interaction: true, Utility: true, Shell: hub.ShellSettings{Executable: "/bin/sh", Enabled: true, HostAccessApproved: true}, Web: hub.WebSettings{ReadURLs: true, Enabled: true, NetworkAccessApproved: true}}
	if err = s.ReplaceTools(ctx, p.ID, draft); err != nil {
		t.Fatal(err)
	}
	value, _ = s.Settings(ctx, p.ID)
	if value.Tools.Planning || value.Tools.Notebook || value.Tools.Interaction || value.Tools.Utility || value.Tools.Shell.Enabled || value.Tools.Shell.HostAccessApproved || value.Tools.Web.NetworkAccessApproved {
		t.Fatal("configuration imported tool authority")
	}
	for _, kind := range []string{"shell", "web"} {
		if err = s.SetToolEnabled(ctx, p.ID, kind, true, false); err == nil {
			t.Fatal("access consent bypass", kind)
		}
	}
	for _, kind := range []string{"planning", "notebook", "interaction", "utility", "shell", "web"} {
		if err = s.SetToolEnabled(ctx, p.ID, kind, true, true); err != nil {
			t.Fatal(kind, err)
		}
	}
	if err = s.SetSkillsEnabled(ctx, p.ID, true); err != nil {
		t.Fatal(err)
	}
	value, _ = s.Settings(ctx, p.ID)
	if !value.Tools.Shell.Enabled || !value.Tools.Planning {
		t.Fatal("unrelated settings cleared approvals")
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s = nil
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	value, err = s.Settings(ctx, p.ID)
	if err != nil || !value.Tools.Shell.Enabled || !value.Tools.Web.NetworkAccessApproved || !value.Tools.Utility {
		t.Fatal("tool authority lost on reopen", err)
	}
	if _, err = s.BeginTurn(ctx, p.ID, "real durable claim"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetToolEnabled(ctx, p.ID, "planning", false, false); err == nil {
		t.Fatal("active recipe changed")
	}
	if err = s.ReplaceTools(ctx, p.ID, hub.ToolSettings{}); err == nil {
		t.Fatal("active config replaced")
	}
}
