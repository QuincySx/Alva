// INPUT: Explicit AEP pins and product-private SQLite with no interpreter execution
// OUTPUT: Separate approval, edit invalidation, active-turn rejection and durable next-turn regressions
// POS: [L7 Hub product] Product snapshot authority tests independent of native Plugin runtime
package sqlite

import (
	"context"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func TestAEPApprovalsAreIndependentDurableAndClearedOnEdit(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	session, err := s.CreateSession(ctx, "AEP settings", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	entry := hub.AEPBundle{ID: "fixture", SourceDirectory: filepath.Join(t.TempDir(), "source"), Interpreter: filepath.Join(t.TempDir(), "interpreter"), Name: "fixture", Version: "1", SHA256: strings.Repeat("a", 64), Enabled: true, HostAccessApproved: true}
	if err = s.ReplaceAEP(ctx, session.ID, []hub.AEPBundle{entry}); err != nil {
		t.Fatal(err)
	}
	settings, err := s.Settings(ctx, session.ID)
	if err != nil || settings.AEP[0].Enabled || settings.AEP[0].HostAccessApproved || !entry.Enabled {
		t.Fatal("import self-approved or mutated input", err)
	}
	if err = s.SetAEPEnabled(ctx, session.ID, entry.ID, true, false); err == nil {
		t.Fatal("missing host consent accepted")
	}
	if err = s.SetAEPEnabled(ctx, "foreign", entry.ID, true, true); err == nil {
		t.Fatal("foreign product accepted")
	}
	if err = s.SetAEPEnabled(ctx, session.ID, entry.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	settings, err = s.Settings(ctx, session.ID)
	if err != nil || !settings.AEP[0].Enabled || settings.AEP[0].SHA256 != entry.SHA256 {
		t.Fatal("approved pin not durable", err)
	}
	if err = s.ReplaceMCP(ctx, session.ID, nil); err != nil {
		t.Fatal(err)
	}
	settings, err = s.Settings(ctx, session.ID)
	if err != nil || !settings.AEP[0].Enabled {
		t.Fatal("unrelated MCP edit invalidated AEP approval", err)
	}
	entry.InterpreterArgs = []string{"new-explicit-argument"}
	if err = s.ReplaceAEP(ctx, session.ID, []hub.AEPBundle{entry}); err != nil {
		t.Fatal(err)
	}
	settings, err = s.Settings(ctx, session.ID)
	if err != nil || settings.AEP[0].Enabled || settings.AEP[0].HostAccessApproved {
		t.Fatal("edited config retained approval", err)
	}
	if _, err = s.BeginTurn(ctx, session.ID, "pending"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetAEPEnabled(ctx, session.ID, entry.ID, true, true); err == nil {
		t.Fatal("active snapshot enabled")
	}
	if err = s.ReplaceAEP(ctx, session.ID, []hub.AEPBundle{entry}); err == nil {
		t.Fatal("active snapshot edited")
	}
}
