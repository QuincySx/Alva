// INPUT: Actual private product SQLite and explicit unstarted decoder snapshots
// OUTPUT: Default-off, separate approval, disabled edits and next-turn persistence regressions
// POS: [L7 Hub product] Product media preferences do not discover, execute or claim model image support
package sqlite

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestMediaSettingsAreDefaultOffAndDecoderEditsClearConsent(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = store.Close() }()
	product, err := store.CreateSession(ctx, "media", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	value, err := store.Settings(ctx, product.ID)
	if err != nil || value.MediaEnabled || value.MediaDecoder.Enabled {
		t.Fatal("media auto-enabled", err)
	}
	d := hub.MediaDecoder{FFmpeg: filepath.Join(t.TempDir(), "not-started-ffmpeg"), FFprobe: filepath.Join(t.TempDir(), "not-started-ffprobe"), Enabled: true, HostAccessApproved: true}
	if err = store.ReplaceMediaDecoder(ctx, product.ID, d); err != nil {
		t.Fatal(err)
	}
	value, err = store.Settings(ctx, product.ID)
	if err != nil || value.MediaDecoder.Enabled || value.MediaDecoder.HostAccessApproved || value.MediaEnabled {
		t.Fatal("decoder draft self-approved", err)
	}
	if err = store.SetMediaDecoderEnabled(ctx, product.ID, true, false); err == nil {
		t.Fatal("decoder host access self-approved")
	}
	if err = store.SetMediaDecoderEnabled(ctx, product.ID, true, true); err != nil {
		t.Fatal(err)
	}
	if err = store.SetMediaEnabled(ctx, product.ID, true); err != nil {
		t.Fatal(err)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	value, err = store.Settings(ctx, product.ID)
	if err != nil || !value.MediaEnabled || !value.MediaDecoder.Enabled || !value.MediaDecoder.HostAccessApproved {
		t.Fatal("explicit consent did not persist", err)
	}
	if err = store.SaveSettings(ctx, value); err != nil {
		t.Fatal(err)
	}
	value, err = store.Settings(ctx, product.ID)
	if err != nil || value.MediaDecoder.Enabled || value.MediaDecoder.HostAccessApproved {
		t.Fatal("whole import retained command approval", err)
	}
	if _, err = store.BeginTurn(ctx, product.ID, "active"); err != nil {
		t.Fatal(err)
	}
	for _, action := range []func() error{
		func() error { return store.SetMediaEnabled(ctx, product.ID, false) },
		func() error { return store.ReplaceMediaDecoder(ctx, product.ID, d) },
		func() error { return store.SetMediaDecoderEnabled(ctx, product.ID, false, false) },
	} {
		if action() == nil {
			t.Fatal("active media configuration mutated")
		}
	}
	if err = store.SetMediaEnabled(ctx, "foreign", true); err == nil {
		t.Fatal("foreign media settings accepted")
	}
}
