// INPUT: Product-bound inspected AEP pins and independent explicit host-access consent
// OUTPUT: ReplaceAEP and idle-only SetAEPEnabled with durable next-turn snapshots
// POS: [L7 Hub product] Product configuration authority; never reads source files, launches or trusts imported approval flags
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func (s *Store) ReplaceAEP(ctx context.Context, id string, entries []hub.AEPBundle) error {
	entries = append([]hub.AEPBundle(nil), entries...)
	for i := range entries {
		entries[i].Enabled = false
		entries[i].HostAccessApproved = false
	}
	if err := hub.ValidateAEP(entries); err != nil {
		return err
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if session.Status == hub.Running {
			return errors.New("finish current Turn before editing next-turn AEP snapshots")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.AEP = entries
		return saveSettings(ctx, tx, value)
	})
}

func (s *Store) SetAEPEnabled(ctx context.Context, id, bundleID string, enabled, approved bool) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if session.Status == hub.Running {
			return errors.New("finish current Turn before changing next-turn AEP capability")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		for i := range value.AEP {
			if value.AEP[i].ID != bundleID {
				continue
			}
			if enabled && (!approved || value.AEP[i].SHA256 == "" || value.AEP[i].Name == "" || value.AEP[i].Version == "") {
				return errors.New("approve the inspected native bundle's full host access before enabling")
			}
			value.AEP[i].Enabled = enabled
			value.AEP[i].HostAccessApproved = enabled && approved
			return saveSettings(ctx, tx, value)
		}
		return errors.New("AEP bundle not found in this product session")
	})
}
