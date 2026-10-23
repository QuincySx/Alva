// INPUT: Shared connection edits, selected private SQLite and existing transaction helpers.
// OUTPUT: Durable optimistic connection CRUD and explicitly scoped credential resolution.
// POS: [L7 SQLite adapter] Separates public connection metadata from private credential rows; never discovers accounts.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
	"slices"

	"github.com/QuincySx/Alva/apps/hub"
)

// Credentials are separate from ordinary metadata and exports. The containing
// product database is private (0600); SQLite does not encrypt these bytes.
type connectionCredentialRow struct {
	APIKey  string            `json:"api_key"`
	Headers map[string]string `json:"headers"`
}

func connection(ctx context.Context, q querier, id string) (hub.ModelConnection, error) {
	c, err := read[hub.ModelConnection](ctx, q, `SELECT data FROM hub_connections WHERE id=?`, id)
	if err != nil {
		return hub.ModelConnection{}, err
	}
	var revision uint64
	if err = q.QueryRowContext(ctx, `SELECT revision FROM hub_connections WHERE id=?`, id).Scan(&revision); err != nil {
		return hub.ModelConnection{}, err
	}
	if c.ID != id || revision != c.Revision || c.Validate() != nil {
		return hub.ModelConnection{}, errors.New("product model connection is corrupt")
	}
	return c, nil
}
func connectionCredentials(ctx context.Context, q querier, c hub.ModelConnection) (hub.ConnectionSecrets, error) {
	r, err := read[connectionCredentialRow](ctx, q, `SELECT data FROM hub_connection_credentials WHERE connection=?`, c.ID)
	if err != nil {
		return hub.ConnectionSecrets{}, err
	}
	secret := hub.ConnectionSecrets{APIKey: r.APIKey, Headers: r.Headers}
	check := hub.ConnectionUpdate{ID: c.ID, ExpectedRevision: c.Revision, Name: c.Name, Protocol: c.Protocol, Endpoint: c.Endpoint, Auth: c.Auth, Enabled: c.Enabled, Models: c.Models}
	validated, checkedSecret, err := c.Apply(check, secret)
	if err != nil || checkedSecret.APIKey != secret.APIKey || !maps.Equal(checkedSecret.Headers, secret.Headers) || validated.HasAPIKey != c.HasAPIKey || !slices.Equal(validated.HeaderNames, c.HeaderNames) {
		return hub.ConnectionSecrets{}, errors.New("product model credentials are corrupt")
	}
	return secret, nil
}
func connections(ctx context.Context, tx *sql.Tx) ([]hub.ModelConnection, error) {
	rows, err := tx.QueryContext(ctx, `SELECT id FROM hub_connections ORDER BY created,id LIMIT 129`)
	if err != nil {
		return nil, err
	}
	var ids []string
	for rows.Next() {
		var id string
		if err = rows.Scan(&id); err != nil {
			rows.Close()
			return nil, err
		}
		ids = append(ids, id)
	}
	err = errors.Join(rows.Err(), rows.Close())
	if err != nil {
		return nil, err
	}
	if len(ids) > 128 {
		return nil, errors.New("too many stored model connections")
	}
	result := make([]hub.ModelConnection, 0, len(ids))
	for _, id := range ids {
		c, err := connection(ctx, tx, id)
		if err != nil {
			return nil, err
		}
		if _, err = connectionCredentials(ctx, tx, c); err != nil {
			return nil, err
		}
		result = append(result, c)
	}
	return result, nil
}
func (s *Store) Connections(ctx context.Context) (value []hub.ModelConnection, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { value, err = connections(ctx, tx); return err })
	if err != nil {
		return nil, err
	}
	return value, nil
}
func (s *Store) SaveConnection(ctx context.Context, u hub.ConnectionUpdate) (value hub.ModelConnection, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var current hub.ModelConnection
		var secret hub.ConnectionSecrets
		creating := u.ID == ""
		if creating {
			if u.ExpectedRevision != 0 {
				return hub.ErrConnectionConflict
			}
			var count int
			if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_connections`).Scan(&count); err != nil {
				return err
			}
			if count >= 128 {
				return errors.New("at most 128 model connections")
			}
			id, err := hub.NewID("connection-")
			if err != nil {
				return err
			}
			u.ID = id
			current.ID = id
			current.CreatedAt = now()
		} else {
			var err error
			current, err = connection(ctx, tx, u.ID)
			if err != nil {
				return err
			}
			secret, err = connectionCredentials(ctx, tx, current)
			if err != nil {
				return err
			}
		}
		var err error
		value, secret, err = current.Apply(u, secret)
		if err != nil {
			return err
		}
		value.UpdatedAt = now()
		data, err := body(value)
		if err != nil {
			return err
		}
		private, err := json.Marshal(connectionCredentialRow{secret.APIKey, secret.Headers})
		if err != nil {
			return err
		}
		if creating {
			if _, err = tx.ExecContext(ctx, `INSERT INTO hub_connections(id,revision,created,data) VALUES(?,?,?,?)`, value.ID, value.Revision, value.CreatedAt, data); err != nil {
				return err
			}
			_, err = tx.ExecContext(ctx, `INSERT INTO hub_connection_credentials(connection,data) VALUES(?,?)`, value.ID, private)
			return err
		}
		result, err := tx.ExecContext(ctx, `UPDATE hub_connections SET revision=?,data=? WHERE id=? AND revision=?`, value.Revision, data, value.ID, u.ExpectedRevision)
		if err != nil {
			return err
		}
		n, err := result.RowsAffected()
		if err != nil {
			return err
		}
		if n != 1 {
			return hub.ErrConnectionConflict
		}
		_, err = tx.ExecContext(ctx, `UPDATE hub_connection_credentials SET data=? WHERE connection=?`, private, value.ID)
		return err
	})
	if err != nil {
		return hub.ModelConnection{}, err
	}
	return value, nil
}
func (s *Store) DeleteConnection(ctx context.Context, id string, expected uint64) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		c, err := connection(ctx, tx, id)
		if err != nil {
			return err
		}
		if expected == 0 || c.Revision != expected {
			return hub.ErrConnectionConflict
		}
		if _, err = tx.ExecContext(ctx, `DELETE FROM hub_connection_credentials WHERE connection=?`, id); err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `DELETE FROM hub_connections WHERE id=? AND revision=?`, id, expected)
		return err
	})
}
func (s *Store) ConnectionSecrets(ctx context.Context, id string, revision uint64) (value hub.ConnectionSecrets, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		c, err := connection(ctx, tx, id)
		if err != nil {
			return err
		}
		if revision == 0 || c.Revision != revision {
			return hub.ErrConnectionConflict
		}
		value, err = connectionCredentials(ctx, tx, c)
		return err
	})
	if err != nil {
		return hub.ConnectionSecrets{}, err
	}
	return value, nil
}

var _ hub.ConnectionsStore = (*Store)(nil)
