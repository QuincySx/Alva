// INPUT: Explicit browser snapshots and real private product database lifecycle
// OUTPUT: Disabled edits, independent host approval, persistence and active-run immutability regressions
// POS: [L7 Hub product] No browser executable is discovered, started or attached by product preferences
package sqlite

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func TestBrowserSettingsAreExplicitDisabledOnEditAndNextTurnOnly(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	p, err := s.CreateSession(ctx, "browser", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	browser := hub.BrowserSettings{Executable: filepath.Join(t.TempDir(), "not-started-browser"), Headless: true, Enabled: true, HostAccessApproved: true}
	if err = s.ReplaceBrowser(ctx, p.ID, browser); err != nil {
		t.Fatal(err)
	}
	v, err := s.Settings(ctx, p.ID)
	if err != nil || v.Browser.Enabled || v.Browser.HostAccessApproved {
		t.Fatal("browser edit self-approved", err)
	}
	if err = s.SetBrowserEnabled(ctx, p.ID, true, false); err == nil {
		t.Fatal("browser skipped independent consent")
	}
	if err = s.SetBrowserEnabled(ctx, p.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	v, err = s.Settings(ctx, p.ID)
	if err != nil || !v.Browser.Enabled || !v.Browser.HostAccessApproved {
		t.Fatal("browser snapshot not durable", err)
	}
	if err = s.SaveSettings(ctx, v); err != nil {
		t.Fatal(err)
	}
	v, err = s.Settings(ctx, p.ID)
	if err != nil || v.Browser.Enabled || v.Browser.HostAccessApproved {
		t.Fatal("whole settings import retained browser consent", err)
	}
	if _, err = s.BeginTurn(ctx, p.ID, "active"); err != nil {
		t.Fatal(err)
	}
	if s.ReplaceBrowser(ctx, p.ID, browser) == nil || s.SetBrowserEnabled(ctx, p.ID, false, false) == nil {
		t.Fatal("active browser snapshot mutated")
	}
	for _, invalid := range []hub.BrowserSettings{
		{Executable: "browser-from-PATH"},
		{Executable: browser.Executable, Env: map[string]string{"HOME": "existing-profile"}},
		{Executable: browser.Executable, Proxy: "http://user:secret@127.0.0.1:1"},
		{Executable: browser.Executable, StartupTimeoutMS: 60001},
	} {
		if invalid.Validate() == nil {
			t.Fatal("unsafe browser snapshot accepted", invalid)
		}
	}
}
