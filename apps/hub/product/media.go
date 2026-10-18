// INPUT: Committed product branch ownership and the actual native media Library
// OUTPUT: Cold media metadata/manifests/preview/receipts shared by HTTP and self-drawn desktop
// POS: [L7 Hub product] Product root inspection never restores an Agent, invokes a decoder or replays media selection
package product

import (
	"context"
	mediaextension "github.com/QuincySx/Alva/extension/media"
	"github.com/QuincySx/Alva/platform/storage/session"
)

func (s *HubService) mediaLibrary(ctx context.Context, id, branchID string) (*mediaextension.Library, error) {
	branch, err := s.boundBranch(ctx, id, branchID)
	if err != nil {
		return nil, err
	}
	return mediaextension.NewLibrary(ctx, s.Repository, id, branch.NativeSessionID)
}

func (s *HubService) MediaSources(ctx context.Context, id, branchID string, after, highWater uint64) (session.ArtifactIndexPage, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return session.ArtifactIndexPage{}, err
	}
	return library.Sources(ctx, after, highWater)
}
func (s *HubService) MediaSelections(ctx context.Context, id, branchID string, after, highWater uint64) (session.ArtifactIndexPage, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return session.ArtifactIndexPage{}, err
	}
	return library.Selections(ctx, after, highWater)
}
func (s *HubService) MediaSelection(ctx context.Context, id, branchID, uri string) (mediaextension.Selection, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return mediaextension.Selection{}, err
	}
	return library.ReadSelection(ctx, uri)
}
func (s *HubService) MediaReceipt(ctx context.Context, id, branchID, uri string) (mediaextension.Receipt, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return mediaextension.Receipt{}, err
	}
	return library.ReadReceipt(ctx, uri)
}
func (s *HubService) MediaIndex(ctx context.Context, id, branchID, sourceURI string, after, highWater uint64) (session.ArtifactIndexPage, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return session.ArtifactIndexPage{}, err
	}
	return library.ListArtifacts(ctx, sourceURI, after, highWater)
}
func (s *HubService) MediaManifest(ctx context.Context, id, branchID, manifestURI string) (mediaextension.Manifest, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return mediaextension.Manifest{}, err
	}
	return library.ReadManifest(ctx, manifestURI)
}
func (s *HubService) MediaPreview(ctx context.Context, id, branchID, manifestURI string) (mediaextension.Manifest, []byte, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return mediaextension.Manifest{}, nil, err
	}
	return library.ReadDerivative(ctx, manifestURI)
}
func (s *HubService) MediaReceipts(ctx context.Context, id, branchID, selectionID string, after, highWater uint64) (session.ArtifactIndexPage, error) {
	library, err := s.mediaLibrary(ctx, id, branchID)
	if err != nil {
		return session.ArtifactIndexPage{}, err
	}
	return library.Receipts(ctx, selectionID, after, highWater)
}
