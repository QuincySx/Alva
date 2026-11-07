// INPUT: Exact persisted composer draft associations and immutable captured-file catalog receipts.
// OUTPUT: Restored captured selections and bounded metadata without retargeting original ownership or reading source paths.
// POS: [L7 Hub product] Restores only the file IDs actually associated with the committed logical draft, including explicit first-Send adoption.
package product

import (
	"context"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *HubService) RestoreComposerFileSelections(ctx context.Context, draft hub.ComposerDraft) ([]ComposerFileSelection, map[string]hub.ComposerFile, error) {
	if err := draft.Validate(); err != nil {
		return nil, nil, err
	}
	var current hub.ComposerDraft
	var err error
	if s.ComposerDrafts != nil {
		current, err = s.ComposerDrafts.Read(ctx, draft.Scope)
	} else {
		drafts, ok := s.Store.(hub.ComposerDraftStore)
		if !ok {
			return nil, nil, errors.New("durable composer draft associations are unavailable")
		}
		current, err = drafts.ComposerDraft(ctx, draft.Scope)
	}
	if err != nil {
		return nil, nil, err
	}
	if !reflect.DeepEqual(hub.CloneComposerDraft(current), hub.CloneComposerDraft(draft)) {
		return nil, nil, hub.ErrComposerDraftConflict
	}
	hasFiles := false
	for _, ref := range draft.References {
		if ref.Kind == "file" {
			hasFiles = true
			break
		}
	}
	if !hasFiles {
		return []ComposerFileSelection{}, map[string]hub.ComposerFile{}, ctx.Err()
	}
	files, err := s.composerFileStore()
	if err != nil {
		return nil, nil, err
	}
	var selections []ComposerFileSelection
	records := make(map[string]hub.ComposerFile)
	for _, reference := range draft.References {
		if reference.Kind != "file" {
			continue
		}
		f, err := files.ComposerFile(ctx, reference.ID)
		if err != nil {
			return nil, nil, err
		}
		if f.Validate() != nil || f.SHA256 != reference.SHA256 || f.Scope != draft.Scope && (f.Scope.Kind != "home" || draft.Scope.Kind != "session") {
			return nil, nil, hub.ErrComposerFileChanged
		}
		if draft.Scope.Kind == "session" {
			if err = s.ValidateComposerFileOwner(ctx, draft.Scope.ID, f); err != nil {
				return nil, nil, err
			}
		}
		selections = append(selections, SelectedComposerFile(f))
		records[f.ID] = f
	}
	return selections, records, ctx.Err()
}
