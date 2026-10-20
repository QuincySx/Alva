// INPUT: Concrete SQL errors and the shared Hub storage error vocabulary.
// OUTPUT: Backend-independent not-found classification at every public provider boundary.
// POS: [L7 storage adapter] Keeps product, Tool and HTTP consumers independent of SQL sentinel errors.
package sqlite

import (
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func normalizeStoreError(err error) error {
	if errors.Is(err, sql.ErrNoRows) && !errors.Is(err, hub.ErrNotFound) {
		return errors.Join(hub.ErrNotFound, err)
	}
	return err
}
