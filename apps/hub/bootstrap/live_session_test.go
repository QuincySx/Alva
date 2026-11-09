// INPUT: A bootstrap application and the product session-creation command.
// OUTPUT: createLive, a session whose durable waits are off, and the default-on regression.
// POS: [L7 native acceptance] Tests of the live (non-durable) approval path opt out of the default-on durable waits explicitly.
package bootstrap

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
)

// createLive creates a session and turns durable waits off so a test can
// exercise the in-process approval and question path.
func createLive(service *product.HubService, ctx context.Context, title, workspace string) (hub.Session, error) {
	session, err := service.Create(ctx, title, workspace)
	if err != nil {
		return session, err
	}
	return session, service.EnableDurableWaits(ctx, session.ID, false)
}

func TestNewSessionsDefaultToDurableWaits(t *testing.T) {
	a, err := Open(t.Context(), waitConfig(t, "http://127.0.0.1:1"))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	session, err := a.Service.Create(t.Context(), "default waits", "")
	if err != nil {
		t.Fatal(err)
	}
	settings, err := a.Service.Store.Settings(t.Context(), session.ID)
	if err != nil || !settings.DurableWaits {
		t.Fatal("a new session must start with durable waits on", err)
	}
	view, err := a.Service.Waits(t.Context(), session.ID)
	if err != nil || !view.Enabled {
		t.Fatal("the wait view must report the default", err)
	}
}
