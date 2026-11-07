// INPUT: Explicit native-owned selected originals and the existing run lifecycle
// OUTPUT: Verified image derivatives and a one-request selection using existing media receipts
// POS: [L4 extension] Input selection reuses the media owner and middleware without registering processing tools
package media

import (
	"context"
	"errors"

	nativemedia "github.com/QuincySx/Alva/platform/system/media"
)

func (p *Plugin) prepareSelectedInputs(ctx context.Context, runID string) error {
	ctx, finish, err := p.begin(ctx, runID)
	if err != nil {
		return err
	}
	defer finish()
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	case <-ctx.Done():
		return ctx.Err()
	}
	var manifests []string
	for _, uri := range p.config.SelectedInputURIs {
		info, err := p.library.source(ctx, uri)
		if err != nil {
			return err
		}
		if info.Bytes > nativemedia.MaxInput || info.MediaType != "image/png" && info.MediaType != "image/jpeg" && info.MediaType != "image/gif" {
			return errors.New("selected image requires PNG, JPEG or GIF within 20 MiB")
		}
		_, original, err := p.library.Original(ctx, uri)
		if err != nil {
			return err
		}
		output, err := nativemedia.ProcessImage(ctx, original, info.MediaType)
		if err != nil {
			return err
		}
		refs, err := p.library.saveOutputs(ctx, info, []nativemedia.Output{output})
		if err != nil {
			return err
		}
		for _, ref := range refs {
			manifests = append(manifests, ref.URI)
		}
	}
	selected, _, err := p.library.prepare(ctx, runID, "selected-image-input", p.config.Route, "Explicitly selected composer images for this request", manifests)
	if err != nil {
		return err
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.ending || p.runID != runID {
		return errors.New("media run ended before selected images were prepared")
	}
	p.pending = &selected
	return nil
}
