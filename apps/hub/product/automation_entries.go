// INPUT: context, hub AutomationStore owned by the AutomationCoordinator
// OUTPUT: AutomationCoordinator List/Read/Create/Update/Delete entry methods
// POS: [L7 Hub product] The one schedule-configuration entry desktop and HTTP share; editing never runs a Turn
package product

import (
	"context"

	"github.com/QuincySx/Alva/apps/hub"
)

func (a *AutomationCoordinator) List(ctx context.Context, session string) ([]hub.Automation, error) {
	return a.Store.ListAutomations(ctx, session)
}

func (a *AutomationCoordinator) Read(ctx context.Context, session, id string) (hub.Automation, error) {
	return a.Store.Automation(ctx, session, id)
}

func (a *AutomationCoordinator) Create(ctx context.Context, session string, draft hub.AutomationDraft) (hub.Automation, error) {
	return a.Store.CreateAutomation(ctx, session, draft)
}

func (a *AutomationCoordinator) Update(ctx context.Context, session, id string, revision uint64, draft hub.AutomationDraft) (hub.Automation, error) {
	return a.Store.UpdateAutomation(ctx, session, id, revision, draft)
}

// Delete removes only the schedule configuration, never its Turns or history.
func (a *AutomationCoordinator) Delete(ctx context.Context, session, id string, revision uint64) error {
	return a.Store.DeleteAutomation(ctx, session, id, revision)
}
