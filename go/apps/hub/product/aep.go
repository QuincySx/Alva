// INPUT: Explicit native-only AEP draft paths and bounded manifest/bundle staging
// OUTPUT: Inspected disabled pins and StagePinnedAEP for the shared next-turn native recipe
// POS: [L7 Hub product] Product host-file authority, never exposed as an HTTP client arbitrary-path import
package product

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/extension/loader"
	private "github.com/QuincySx/Alva/go/platform/storage/sqlite"
)

// SaveAEPDraft is a native/operator host-file operation, not a browser upload.
// Inspection copies and removes bounded snapshots but never starts an
// interpreter. Every edit clears approval, including interpreter/args/env-only
// edits. A saved path is NOT a pin on the interpreter binary itself.
func (s *HubService) SaveAEPDraft(ctx context.Context, id, raw string) error {
	if len(raw) > 64<<10 {
		return errors.New("AEP draft exceeds 64 KiB editor budget")
	}
	if trimmed := bytes.TrimSpace([]byte(raw)); len(trimmed) == 0 || trimmed[0] != '[' {
		return errors.New("AEP configuration must be an explicit JSON array, not null")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if detail.Session.Status == hub.Running {
		return errors.New("finish current Turn before inspecting next-turn AEP configuration")
	}
	var entries []hub.AEPBundle
	decoder := json.NewDecoder(bytes.NewBufferString(raw))
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&entries); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return errors.New("trailing AEP configuration")
	}
	for i := range entries {
		entries[i].Enabled = false
		entries[i].HostAccessApproved = false
		entries[i].Name, entries[i].Version, entries[i].SHA256 = "", "", ""
	}
	if err = hub.ValidateAEP(entries); err != nil {
		return err
	}
	if len(entries) > 0 {
		parent, err := private.EnsurePrivateDir(filepath.Join(s.DataDirectory, "aep-snapshots"))
		if err != nil {
			return err
		}
		for i := range entries {
			manifest, err := loader.ReadManifest(ctx, entries[i].SourceDirectory)
			if err != nil {
				return err
			}
			bundle, err := loader.Stage(ctx, entries[i].SourceDirectory, parent, manifest)
			if err != nil {
				return err
			}
			entries[i].Name, entries[i].Version, entries[i].SHA256 = manifest.Name, manifest.Version, bundle.Digest()
			if err = bundle.Close(); err != nil {
				return err
			}
		}
	}
	if err = hub.ValidateAEP(entries); err != nil {
		return err
	}
	return s.Store.ReplaceAEP(ctx, id, entries)
}

// StagePinnedAEP returns caller-owned immutable code for an already separately
// approved product setting. Callers transfer ownership only on OpenPlugin
// success; no source edit, manifest change or unapproved entry is accepted.
func StagePinnedAEP(ctx context.Context, entry hub.AEPBundle, parent string) (*loader.Bundle, error) {
	if !entry.Enabled || !entry.HostAccessApproved {
		return nil, errors.New("AEP entry is not explicitly enabled and approved")
	}
	if err := hub.ValidateAEP([]hub.AEPBundle{entry}); err != nil {
		return nil, err
	}
	manifest, err := loader.ReadManifest(ctx, entry.SourceDirectory)
	if err != nil {
		return nil, err
	}
	if manifest.Name != entry.Name || manifest.Version != entry.Version {
		return nil, errors.New("AEP manifest identity changed since inspection; edit and approve again")
	}
	bundle, err := loader.Stage(ctx, entry.SourceDirectory, parent, manifest)
	if err != nil {
		return nil, err
	}
	if !strings.EqualFold(bundle.Digest(), entry.SHA256) {
		return nil, errors.Join(errors.New("AEP code pin changed since inspection; edit and approve again"), bundle.Close())
	}
	return bundle, nil
}
