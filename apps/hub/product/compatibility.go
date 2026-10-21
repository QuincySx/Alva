// INPUT: Saved product workspace and an explicit project-relative compatibility config.
// OUTPUT: InspectCompatibility and SetCompatibilityFile for native UI and authenticated HTTP.
// POS: [L7 product service] Cold bounded source review and next-turn opt-in; never starts an Agent or grants tools.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/extension/compatibility"
)

func (s *HubService) InspectCompatibility(ctx context.Context, id, file string) (receipt compatibility.Receipt, err error) {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return receipt, err
	}
	snapshot, err := compatibility.OpenFile(ctx, detail.Session.Workspace, file)
	if err != nil {
		return receipt, err
	}
	defer func() { err = errors.Join(err, snapshot.Close(context.WithoutCancel(ctx))) }()
	return snapshot.Receipt(), nil
}

func (s *HubService) SetCompatibilityFile(ctx context.Context, id, file string) error {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if hub.BlocksInput(detail.Session.Status) {
		return errors.New("finish current Turn before changing compatibility sources")
	}
	if file != "" {
		receipt, err := s.InspectCompatibility(ctx, id, file)
		if err != nil {
			return err
		}
		if len(receipt.Agents) > 0 {
			settings, err := s.Store.Settings(ctx, id)
			if err != nil {
				return err
			}
			if !settings.SubagentsEnabled {
				return errors.New("enable native SubAgents before selecting compatibility Agent definitions")
			}
		}
	}
	// The storage transaction rechecks idle state. No whole-settings import or
	// unrelated plugin approvals are changed by this operation.
	return s.Store.SetCompatibilityFile(ctx, id, file)
}
