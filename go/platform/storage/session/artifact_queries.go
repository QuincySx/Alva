// INPUT: Authorized native artifact metadata, immutable chunk BLOBs and explicit bounded query cursors
// OUTPUT: ReadArtifactFull and fixed-high-water PageArtifacts without runtime/session restoration
// POS: [platform / durable storage] Cold owner-scoped binary verification and derivative indices share the existing artifact authority
package session

import (
	"context"
	"errors"
	"math"
	"unicode/utf8"
)

type ArtifactEntry struct {
	Sequence uint64       `json:"sequence"`
	Info     ArtifactInfo `json:"info"`
}
type ArtifactIndexPage struct {
	Entries   []ArtifactEntry `json:"entries"`
	Next      uint64          `json:"next"`
	HighWater uint64          `json:"high_water"`
	HasMore   bool            `json:"has_more"`
}

// ArtifactMetadata never reads a chunk, decodes media or resumes a runtime.
func (r *Repository) ArtifactMetadata(ctx context.Context, owner, id, uri string) (ArtifactInfo, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ArtifactInfo{}, ErrClosed
	}
	return r.artifactInfoLocked(ctx, owner, id, uri)
}

// ReadArtifactFull verifies the complete immutable byte stream and its full
// SHA-256, not only the pages a preview happened to visit. The caller must give
// a budget appropriate for its actual operation; this is not a model upload.
func (r *Repository) ReadArtifactFull(ctx context.Context, owner, id, uri string, maximum int) (ArtifactInfo, []byte, error) {
	if maximum < 1 || maximum > MaxArtifactBytes {
		return ArtifactInfo{}, nil, errors.New("artifact full-read budget must be 1..20 MiB")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ArtifactInfo{}, nil, ErrClosed
	}
	info, err := r.artifactInfoLocked(ctx, owner, id, uri)
	if err != nil {
		return ArtifactInfo{}, nil, err
	}
	data, err := r.readFullLocked(ctx, info.ID, maximum)
	if err != nil {
		return ArtifactInfo{}, nil, err
	}
	return info, data, nil
}

// PageArtifacts selects metadata only, using the never-deleted artifact row
// sequence as cursor. New immutable writes cannot enter a fixed high-water
// page set; Name, when set, is an exact subject identity, not a LIKE pattern.
func (r *Repository) PageArtifacts(ctx context.Context, owner, id, kind, name string, after, highWater uint64, limit int) (ArtifactIndexPage, error) {
	if kind == "" || len(kind) > 64 || len(name) > 256 || !utf8.ValidString(kind+name) || after > math.MaxInt64 || highWater > math.MaxInt64 || limit < 1 || limit > 64 {
		return ArtifactIndexPage{}, errors.New("bounded exact artifact kind/name and 1..64 metadata entries required")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ArtifactIndexPage{}, ErrClosed
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return ArtifactIndexPage{}, err
	}
	var actual uint64
	if err := r.db.QueryRowContext(ctx, "SELECT COALESCE(MAX(rowid),0) FROM artifacts WHERE session_id=?", id).Scan(&actual); err != nil {
		return ArtifactIndexPage{}, err
	}
	if highWater == 0 {
		highWater = actual
	}
	if highWater > actual || after > highWater {
		return ArtifactIndexPage{}, errors.New("artifact cursor exceeds committed high-water")
	}
	query := "SELECT rowid,id,session_id,kind,name,media_type,processing,sha256,bytes,is_text FROM artifacts WHERE session_id=? AND kind=? AND rowid>? AND rowid<=?"
	args := []any{id, kind, after, highWater}
	if name != "" {
		query += " AND name=?"
		args = append(args, name)
	}
	query += " ORDER BY rowid LIMIT ?"
	args = append(args, limit+1)
	rows, err := r.db.QueryContext(ctx, query, args...)
	if err != nil {
		return ArtifactIndexPage{}, err
	}
	defer rows.Close()
	page := ArtifactIndexPage{Next: after, HighWater: highWater}
	for rows.Next() {
		var e ArtifactEntry
		if err = rows.Scan(&e.Sequence, &e.Info.ID, &e.Info.SessionID, &e.Info.Kind, &e.Info.Name, &e.Info.MediaType, &e.Info.Processing, &e.Info.SHA256, &e.Info.Bytes, &e.Info.Text); err != nil {
			return ArtifactIndexPage{}, err
		}
		if len(page.Entries) == limit {
			page.HasMore = true
			break
		}
		page.Entries = append(page.Entries, e)
		page.Next = e.Sequence
	}
	if err = rows.Err(); err != nil {
		return ArtifactIndexPage{}, err
	}
	if !page.HasMore {
		page.Next = highWater
	}
	return page, nil
}
