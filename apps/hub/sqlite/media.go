// INPUT: Session-scoped media preferences and independent explicit decoder host consent
// OUTPUT: Idle-only default-off media capability and disabled decoder edits
// POS: [L7 Hub product] Durable next-turn configuration never discovers or executes media programs
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *Store) editMedia(ctx context.Context, id string, edit func(*hub.Settings) error) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if session.Status == hub.Running {
			return errors.New("finish current Turn before changing next-turn media settings")
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

func (s *Store) SetMediaEnabled(ctx context.Context, id string, enabled bool) error {
	return s.editMedia(ctx, id, func(value *hub.Settings) error {
		value.MediaEnabled = enabled
		return nil
	})
}

// ReplaceMediaDecoder is a native/operator configuration surface. HTTP clients
// may enable an already-saved snapshot, not import arbitrary host executables.
func (s *Store) ReplaceMediaDecoder(ctx context.Context, id string, decoder hub.MediaDecoder) error {
	decoder.Enabled, decoder.HostAccessApproved = false, false
	if err := decoder.Validate(); err != nil {
		return err
	}
	return s.editMedia(ctx, id, func(value *hub.Settings) error {
		value.MediaDecoder = decoder
		return nil
	})
}

func (s *Store) SetMediaDecoderEnabled(ctx context.Context, id string, enabled, approved bool) error {
	return s.editMedia(ctx, id, func(value *hub.Settings) error {
		if enabled && (!approved || value.MediaDecoder.FFmpeg == "" || value.MediaDecoder.FFprobe == "") {
			return errors.New("saved explicit decoder snapshot and independent host access approval required")
		}
		value.MediaDecoder.Enabled = enabled
		value.MediaDecoder.HostAccessApproved = enabled && approved
		return nil
	})
}
