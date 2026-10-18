// INPUT: Explicit selected regular file, direct session identity and native artifact authority
// OUTPUT: Upload, DraftAttachments and RemoveDraftAttachment with bounded reference-only admission
// POS: [L6 application] Original bytes stay session-owned; upload/selection never starts a model or decoder.
package direct

import (
	"context"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

func (s *Service) Upload(ctx context.Context, id, path string) (session.ArtifactInfo, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closing {
		return session.ArtifactInfo{}, session.ErrClosed
	}
	if _, err := s.metadata(ctx, id); err != nil {
		return session.ArtifactInfo{}, err
	}
	if b := s.bindings[id]; b != nil && b.lastRun != "" {
		snapshot, err := b.controller.Snapshot(b.lastRun)
		if err != nil {
			return session.ArtifactInfo{}, err
		}
		if snapshot.Status == host.RunRunning {
			return session.ArtifactInfo{}, errors.New("cannot edit attachment draft while this actual root is active")
		}
	}
	if len(s.drafts[id]) >= 8 || len(s.drafts[id]) == 0 && len(s.drafts) >= 16 {
		return session.ArtifactInfo{}, errors.New("direct attachment draft budget exhausted")
	}
	if !filepath.IsAbs(path) || (security.SensitivePathFilter{}).IsSensitive(path) {
		return session.ArtifactInfo{}, security.ErrPermissionDenied
	}
	resolved, err := filepath.EvalSymlinks(path)
	if err != nil {
		return session.ArtifactInfo{}, err
	}
	if (security.SensitivePathFilter{}).IsSensitive(resolved) {
		return session.ArtifactInfo{}, security.ErrPermissionDenied
	}
	info, err := os.Lstat(path)
	if err != nil {
		return session.ArtifactInfo{}, err
	}
	if !info.Mode().IsRegular() || info.Size() > session.MaxArtifactBytes {
		return session.ArtifactInfo{}, errors.New("attachment must be a regular non-symlink file of at most20MiB")
	}
	var total int64
	for _, artifact := range s.drafts[id] {
		total += artifact.Bytes
	}
	if total+info.Size() > 40<<20 {
		return session.ArtifactInfo{}, errors.New("attachment turn budget exceeds40MiB")
	}
	file, err := os.Open(path)
	if err != nil {
		return session.ArtifactInfo{}, err
	}
	defer file.Close()
	opened, err := file.Stat()
	if err != nil || !os.SameFile(info, opened) {
		return session.ArtifactInfo{}, errors.New("selected attachment changed while opening")
	}
	if err := ctx.Err(); err != nil {
		return session.ArtifactInfo{}, err
	}
	data, err := io.ReadAll(io.LimitReader(file, session.MaxArtifactBytes+1))
	if err != nil {
		return session.ArtifactInfo{}, err
	}
	if err := ctx.Err(); err != nil {
		return session.ArtifactInfo{}, err
	}
	after, err := file.Stat()
	if err != nil || !os.SameFile(opened, after) || after.Size() != opened.Size() || !after.ModTime().Equal(opened.ModTime()) || int64(len(data)) != after.Size() || total+int64(len(data)) > 40<<20 || len(data) > session.MaxArtifactBytes {
		return session.ArtifactInfo{}, errors.New("selected attachment changed or exceeded budget while reading")
	}
	mediaType := http.DetectContentType(data)
	text := utf8.Valid(data) && (len(data) == 0 || strings.HasPrefix(mediaType, "text/") || mediaType == "application/json")
	artifact, err := s.repository.PutArtifact(ctx, owner, id, session.ArtifactInput{Kind: "upload", Name: filepath.Base(path), MediaType: mediaType, Processing: "Original bytes saved with full SHA-256; no compression, frame extraction, transcription or media understanding. Model receives only this session-owned reference, never raw upload automatically.", Data: data, Text: text})
	if err != nil {
		return session.ArtifactInfo{}, err
	}
	for _, existing := range s.drafts[id] {
		if existing.ID == artifact.ID {
			return artifact, nil
		}
	}
	s.drafts[id] = append(s.drafts[id], artifact)
	return artifact, nil
}
func (s *Service) DraftAttachments(id string) []session.ArtifactInfo {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]session.ArtifactInfo(nil), s.drafts[id]...)
}
func (s *Service) RemoveDraftAttachment(id, artifactID string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	files := s.drafts[id]
	for i, file := range files {
		if file.ID == artifactID {
			files = append(files[:i:i], files[i+1:]...)
			break
		}
	}
	if len(files) == 0 {
		delete(s.drafts, id)
	} else {
		s.drafts[id] = files
	}
}
