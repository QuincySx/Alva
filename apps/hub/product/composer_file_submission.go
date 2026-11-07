// INPUT: Explicit captured-file selections, bounded prompt text and the existing shared composer admission owner.
// OUTPUT: Shared exact file freezing for normal Turn and Worker FIFO admission, with original text retained separately.
// POS: [L7 Hub product] File identity and real logical session ownership remain independent of model prompt text.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/integration/engine"
)

func (s *HubService) SubmitComposerFileDraft(ctx context.Context, id, prompt string, selections []ComposerPreparation, legacy []DraftReferenceSelection, captured []ComposerFileSelection) (string, error) {
	if len(captured) == 0 {
		return s.submitDraft(ctx, id, prompt, selections, legacy)
	}
	modelPrompt, inputs, err := s.prepareComposerFileDraft(ctx, id, prompt, legacy, captured)
	if err != nil {
		return "", err
	}
	return s.submitDraftWithFiles(ctx, id, modelPrompt, selections, legacy, inputs, prompt)
}

func (s *HubService) prepareComposerFileDraft(ctx context.Context, id, prompt string, legacy []DraftReferenceSelection, captured []ComposerFileSelection) (string, []engine.InputFile, error) {
	if len(captured) == 0 {
		return prompt, nil, nil
	}
	if len(captured)+len(legacy) > 8 {
		return "", nil, errors.New("at most eight captured files or legacy references may be submitted")
	}
	for _, selection := range captured {
		file, err := s.selectedComposerFile(ctx, selection)
		if err != nil {
			return "", nil, err
		}
		if err = s.ValidateComposerFileOwner(ctx, id, file); err != nil {
			return "", nil, err
		}
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return "", nil, err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return "", nil, err
	}
	descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
	if !ok || descriptor.Runtime == nil || !descriptor.Runtime.Capabilities().FileInputs {
		return "", nil, engine.ErrUnsupported
	}
	// This is the product's prompt byte capacity, not a claim that an unknown
	// model context window has that much free space. Native context admission
	// still applies its actual complete request budget at model execution.
	budget := max(0, min(maxComposerInlineTotalBytes, (64<<10)-len(prompt)-(8<<10)))
	_, manifest, err := s.PrepareComposerFileContext(ctx, captured, budget)
	if err != nil {
		return "", nil, err
	}
	if len(prompt)+len(manifest) > 56<<10 {
		_, manifest, err = s.PrepareComposerFileContext(ctx, captured, 0)
		if err != nil {
			return "", nil, err
		}
	}
	var inputs []engine.InputFile
	for _, selection := range captured {
		f, err := s.selectedComposerFile(ctx, selection)
		if err != nil {
			return "", nil, err
		}
		path, err := s.ComposerFilePrivatePath(ctx, selection, false)
		if err != nil {
			return "", nil, err
		}
		reference := composerReference(f, false)
		input := engine.InputFile{ID: f.ID, Name: f.Name, URI: reference.URI, Path: path, MediaType: f.MediaType, SHA256: f.SHA256, Bytes: f.Bytes}
		if f.Text {
			input.TextURI, input.TextPath, input.TextSHA256, input.TextBytes = input.URI, input.Path, input.SHA256, input.Bytes
		} else if f.TextKey != "" {
			textPath, err := s.ComposerFilePrivatePath(ctx, selection, true)
			if err != nil {
				return "", nil, err
			}
			input.TextURI = composerReference(f, true).URI
			input.TextPath = textPath
			input.TextSHA256 = f.TextSHA256
			input.TextBytes = f.TextBytes
		}
		inputs = append(inputs, input)
	}
	if err = engine.ValidateInputFiles(inputs); err != nil {
		return "", nil, err
	}
	return prompt + manifest, inputs, nil
}
