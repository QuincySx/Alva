// INPUT: Claimed Hub branch/typed child path, explicit export scope and private product directory
// OUTPUT: InspectorBundleAt and native-only SaveInspectorBundleAt without arbitrary client paths
// POS: [L7 Hub product] Export preserves product/native authorization and never restores an Agent or fetches referenced artifacts.
package product

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/session"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

type InspectorExport struct {
	Path    string                `json:"path"`
	Summary session.BundleSummary `json:"summary"`
}

func (s *HubService) InspectorBundleAt(ctx context.Context, id, branchID string, path []ChildHop, highWater uint64, children bool) ([]byte, session.BundleSummary, error) {
	inspector, scope, err := s.inspectionTarget(ctx, id, branchID, path)
	if err != nil {
		return nil, session.BundleSummary{}, err
	}
	if !inspector.InspectionCapabilities().ExportJSONL {
		return nil, session.BundleSummary{}, engine.ErrUnsupported
	}
	bundle, err := inspector.Export(ctx, scope, highWater, children)
	if err != nil {
		return nil, session.BundleSummary{}, err
	}
	return bundle.Data, session.BundleSummary{RootID: bundle.RootID, Sessions: bundle.Sessions, Records: bundle.Records, Bytes: len(bundle.Data), SHA256: bundle.SHA256, HighWaters: bundle.HighWaters}, nil
}
func (s *HubService) SaveInspectorBundleAt(ctx context.Context, id, branchID string, path []ChildHop, highWater uint64, children bool) (InspectorExport, error) {
	data, summary, err := s.InspectorBundleAt(ctx, id, branchID, path, highWater, children)
	if err != nil {
		return InspectorExport{}, err
	}
	if err := ctx.Err(); err != nil {
		return InspectorExport{}, err
	}
	directory, err := private.EnsurePrivateDir(filepath.Join(s.DataDirectory, "exports"))
	if err != nil {
		return InspectorExport{}, err
	}
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return InspectorExport{}, err
	}
	name := filepath.Join(directory, "inspector-"+hex.EncodeToString(random[:])+".jsonl")
	file, err := os.OpenFile(name, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return InspectorExport{}, err
	}
	n, writeErr := file.Write(data)
	if n != len(data) {
		writeErr = errors.Join(writeErr, io.ErrShortWrite)
	}
	err = errors.Join(writeErr, file.Sync(), file.Close(), ctx.Err())
	if err != nil {
		return InspectorExport{Path: name}, errors.Join(err, os.Remove(name))
	}
	return InspectorExport{Path: name, Summary: summary}, nil
}
