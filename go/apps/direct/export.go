// INPUT: Selected owned direct session, explicit snapshot/child scope and private application directory
// OUTPUT: ExportInspector writes a bounded redacted JSONL file only after complete bundle verification
// POS: [L6 application] Native direct export never imports Hub policy or turns references into automatic file/media reads.
package direct

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	private "github.com/QuincySx/Alva/go/platform/storage/sqlite"
)

type InspectorExport struct {
	Path    string
	Summary session.BundleSummary
}

func (s *Service) ExportInspector(ctx context.Context, id string, highWater uint64) (InspectorExport, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return InspectorExport{}, session.ErrClosed
	}
	if _, err := s.metadata(ctx, id); err != nil {
		return InspectorExport{}, err
	}
	data, summary, err := s.repository.ExportBundle(ctx, owner, id, session.BundleOptions{HighWater: highWater})
	if err != nil {
		return InspectorExport{}, err
	}
	if err := ctx.Err(); err != nil {
		return InspectorExport{}, err
	}
	directory, err := private.EnsurePrivateDir(filepath.Join(s.config.DataDirectory, "exports"))
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
