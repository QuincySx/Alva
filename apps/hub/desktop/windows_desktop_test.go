// INPUT: Two independent native pages and real shared product/runtime authority
// OUTPUT: New-window actions reuse product identity while draft/view changes stay per window
// POS: [L7 Hub product] Headless window-controller behavior; actual window/AX acceptance is separately observed.
//go:build desktop

package hubdesktop

import (
	"context"
	"github.com/egoist/mygo/ui"
	"testing"
)

func TestNewWindowActionSharesServiceAndKeepsViewDraftsIndependent(t *testing.T) {
	s, requests := fixture(t)
	session, err := s.Create(context.Background(), "two observers", "")
	if err != nil {
		t.Fatal(err)
	}
	first, second := NewNativePage(s), NewNativePage(s)
	first.selected, second.selected = session.ID, session.ID
	first.model.Draft = "first window unsent draft"
	second.model.Draft = "second window unsent draft"
	opened := ""
	first.openWindow = func(id string) error { opened = id; return nil }
	tester := ui.NewTester(first.Build, 1400, 1400)
	if err = tester.Click("New window"); err != nil {
		t.Fatal(err)
	}
	if opened != session.ID || first.Service != second.Service || first.model.Draft == second.model.Draft || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("window action copied execution or view draft")
	}
}
