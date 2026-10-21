// INPUT: Explicit browser snapshot and independent product-scoped native host approval
// OUTPUT: Disabled browser edits and idle-only next-turn enablement
// POS: [L7 Hub product] Product persistence never starts a browser or imports user profiles/credentials
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *Store) editBrowser(ctx context.Context, id string, edit func(*hub.Settings) error) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn browser settings")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		if err = edit(&value); err != nil {
			return err
		}
		return saveSettings(ctx, tx, value)
	})
}

func (s *Store) ReplaceBrowser(ctx context.Context, id string, browser hub.BrowserSettings) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	browser.Enabled, browser.HostAccessApproved = false, false
	if err := browser.Validate(); err != nil {
		return err
	}
	return s.editBrowser(ctx, id, func(value *hub.Settings) error { value.Browser = browser; return nil })
}
func (s *Store) SetBrowserEnabled(ctx context.Context, id string, enabled, approved bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.editBrowser(ctx, id, func(value *hub.Settings) error {
		if enabled && (!approved || value.Browser.Executable == "") {
			return errors.New("saved explicit browser executable and independent host/network access approval required")
		}
		value.Browser.Enabled, value.Browser.HostAccessApproved = enabled, enabled && approved
		return nil
	})
}
