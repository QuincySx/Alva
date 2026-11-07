// INPUT: Trusted typed captured image selections, product file reader and actual native artifact authority
// OUTPUT: Verified selected-original native artifact references for the existing media input middleware
// POS: [L7 Hub assembly] Copies preserve native provenance; broad media tools and historical uploads stay disabled
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
)

func (a *Application) stageComposerImages(ctx context.Context, request engine.Request, nativeID string, capable bool) ([]string, error) {
	var selected []engine.InputFile
	for _, file := range request.InputFiles {
		if !strings.HasPrefix(strings.ToLower(file.MediaType), "image/") {
			continue
		}
		if !capable {
			return nil, fmt.Errorf("%w: selected images require an explicitly image-capable provider/model route", engine.ErrUnsupported)
		}
		if file.MediaType != "image/png" && file.MediaType != "image/jpeg" && file.MediaType != "image/gif" || file.Bytes > nativemedia.MaxInput {
			return nil, errors.New("selected native images support PNG, JPEG or GIF within 20 MiB each")
		}
		selected = append(selected, file)
	}
	if len(selected) == 0 {
		return nil, nil
	}
	if len(selected) > 2 {
		return nil, errors.New("at most two selected images may enter one model request")
	}
	if a.Service == nil {
		return nil, errors.New("captured image service is unavailable")
	}
	catalog, ok := a.Service.Store.(hub.ComposerFileStore)
	if !ok {
		return nil, errors.New("captured image catalog is unavailable")
	}
	var uris []string
	for _, input := range selected {
		file, err := catalog.ComposerFile(ctx, input.ID)
		if err != nil {
			return nil, err
		}
		if err := file.Validate(); err != nil {
			return nil, err
		}
		if file.SHA256 != input.SHA256 || file.Bytes != input.Bytes || file.MediaType != input.MediaType || file.Name != input.Name {
			return nil, hub.ErrComposerFileChanged
		}
		selection := product.SelectedComposerFile(file)
		path, err := a.Service.ComposerFilePrivatePath(ctx, selection, false)
		if err != nil {
			return nil, err
		}
		if path != input.Path {
			return nil, hub.ErrComposerFileChanged
		}
		data := make([]byte, 0, int(input.Bytes))
		for offset := int64(0); offset < input.Bytes; {
			page, err := a.Service.ReadComposerFile(ctx, selection, false, offset, hub.ComposerFileReadBytes)
			if err != nil {
				return nil, err
			}
			if page.NextOffset <= offset || page.NextOffset > input.Bytes || int64(len(data)+len(page.Data)) > input.Bytes {
				return nil, hub.ErrComposerFileChanged
			}
			data = append(data, page.Data...)
			offset = page.NextOffset
		}
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != input.SHA256 || int64(len(data)) != input.Bytes {
			return nil, hub.ErrComposerFileChanged
		}
		// Media Library validates native ownership on every manifest/read. The
		// captured product original remains untouched; this native copy supplies
		// that separate session provenance rather than borrowing a product URI.
		info, err := a.Repository.PutArtifact(ctx, request.OwnerID, nativeID, storage.ArtifactInput{Kind: "input-attachment", Name: input.Name, MediaType: input.MediaType, Processing: "Explicitly selected immutable composer original " + input.URI + "; original retained separately, no model delivery implied", Data: data})
		if err != nil {
			return nil, err
		}
		if info.SHA256 != input.SHA256 || info.Bytes != input.Bytes {
			return nil, hub.ErrComposerFileChanged
		}
		uris = append(uris, info.Reference().URI)
	}
	return uris, nil
}
