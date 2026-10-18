// INPUT: context, crypto/sha256, database/sql, encoding/hex, unicode/utf8, agent/context, llm/wire.
// OUTPUT: ArtifactInput, ArtifactInfo, ArtifactPage, owned artifact APIs and BoundReferences.
// POS: [platform / durable storage] Session-owned immutable chunk bytes shared by facts and reference retrieval.
package session

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const ChunkBytes = 4096
const MaxArtifactBytes = 20 << 20
const maxRecordBytes = 32 << 20

type ArtifactInput struct {
	Kind, Name, MediaType, Processing string
	Data                              []byte
	Text                              bool
}
type ArtifactInfo struct {
	ID         string `json:"id"`
	SessionID  string `json:"session_id"`
	Kind       string `json:"kind"`
	Name       string `json:"name"`
	MediaType  string `json:"media_type"`
	Processing string `json:"processing"`
	SHA256     string `json:"sha256"`
	Bytes      int64  `json:"bytes"`
	Text       bool   `json:"is_text"`
}
type ArtifactPage struct {
	Info       ArtifactInfo `json:"info"`
	Offset     int64        `json:"offset"`
	NextOffset int64        `json:"next_offset"`
	Data       []byte       `json:"data"`
	Truncated  bool         `json:"truncated"`
}

func (info ArtifactInfo) Reference() wire.Reference {
	return wire.Reference{URI: "alva-artifact://" + info.ID, MediaType: info.MediaType, SHA256: info.SHA256, Bytes: info.Bytes, Processing: info.Processing}
}
func artifactID(uri string) (string, error) {
	const prefix = "alva-artifact://"
	if !strings.HasPrefix(uri, prefix) {
		return "", ErrNotFound
	}
	id := strings.TrimPrefix(uri, prefix)
	if len(id) != 32 {
		return "", ErrNotFound
	}
	decoded, err := hex.DecodeString(id)
	if err != nil || len(decoded) != 16 || id != strings.ToLower(id) {
		return "", ErrNotFound
	}
	return id, nil
}
func digest(data []byte) string { sum := sha256.Sum256(data); return hex.EncodeToString(sum[:]) }

func putArtifactTx(ctx context.Context, tx *sql.Tx, id string, input ArtifactInput, maxBytes int) (ArtifactInfo, error) {
	if len(input.Data) > maxBytes {
		return ArtifactInfo{}, errors.New("artifact byte budget exceeded")
	}
	if input.Kind == "" || len(input.Kind) > 64 || len(input.Name) > 256 || input.MediaType == "" || len(input.MediaType) > 256 || input.Processing == "" || len(input.Processing) > 2048 {
		return ArtifactInfo{}, errors.New("bounded artifact metadata and explicit processing description required")
	}
	if input.Text && !utf8.Valid(input.Data) {
		return ArtifactInfo{}, errors.New("text artifact must be valid UTF-8")
	}
	sha := digest(input.Data)
	var existing ArtifactInfo
	err := tx.QueryRowContext(ctx, "SELECT id,session_id,kind,name,media_type,processing,sha256,bytes,is_text FROM artifacts WHERE session_id=? AND kind=? AND sha256=? AND name=? AND media_type=? AND processing=? AND is_text=?", id, input.Kind, sha, input.Name, input.MediaType, input.Processing, input.Text).Scan(&existing.ID, &existing.SessionID, &existing.Kind, &existing.Name, &existing.MediaType, &existing.Processing, &existing.SHA256, &existing.Bytes, &existing.Text)
	if err == nil {
		return existing, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return ArtifactInfo{}, err
	}
	key, err := newID()
	if err != nil {
		return ArtifactInfo{}, err
	}
	info := ArtifactInfo{ID: key, SessionID: id, Kind: input.Kind, Name: input.Name, MediaType: input.MediaType, Processing: input.Processing, SHA256: sha, Bytes: int64(len(input.Data)), Text: input.Text}
	if _, err = tx.ExecContext(ctx, "INSERT INTO artifacts(id,session_id,kind,name,media_type,processing,sha256,bytes,is_text,created_ns) VALUES(?,?,?,?,?,?,?,?,?,?)", info.ID, id, input.Kind, input.Name, input.MediaType, input.Processing, sha, len(input.Data), input.Text, time.Now().UTC().UnixNano()); err != nil {
		return ArtifactInfo{}, err
	}
	for offset := 0; offset < len(input.Data); offset += ChunkBytes {
		end := offset + ChunkBytes
		if end > len(input.Data) {
			end = len(input.Data)
		}
		chunk := input.Data[offset:end]
		if _, err = tx.ExecContext(ctx, "INSERT INTO artifact_chunks(artifact_id,chunk_index,data,sha256) VALUES(?,?,?,?)", key, offset/ChunkBytes, chunk, digest(chunk)); err != nil {
			return ArtifactInfo{}, err
		}
	}
	return info, nil
}

func (r *Repository) PutArtifact(ctx context.Context, owner, id string, input ArtifactInput) (ArtifactInfo, error) {
	if input.Kind == "typed-record-json" {
		return ArtifactInfo{}, errors.New("typed record artifacts are owned by AppendRecords")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ArtifactInfo{}, ErrClosed
	}
	tx, err := r.db.BeginTx(ctx, nil)
	if err != nil {
		return ArtifactInfo{}, err
	}
	defer tx.Rollback()
	if err = authorize(ctx, tx, owner, id); err != nil {
		return ArtifactInfo{}, err
	}
	info, err := putArtifactTx(ctx, tx, id, input, MaxArtifactBytes)
	if err != nil {
		return ArtifactInfo{}, err
	}
	if err = tx.Commit(); err != nil {
		return ArtifactInfo{}, err
	}
	return info, nil
}

func (r *Repository) artifactInfoLocked(ctx context.Context, owner, id, uri string) (ArtifactInfo, error) {
	key, err := artifactID(uri)
	if err != nil {
		return ArtifactInfo{}, err
	}
	var info ArtifactInfo
	err = r.db.QueryRowContext(ctx, "SELECT a.id,a.session_id,a.kind,a.name,a.media_type,a.processing,a.sha256,a.bytes,a.is_text FROM artifacts a JOIN sessions s ON s.id=a.session_id WHERE s.owner=? AND s.id=? AND a.id=?", owner, id, key).Scan(&info.ID, &info.SessionID, &info.Kind, &info.Name, &info.MediaType, &info.Processing, &info.SHA256, &info.Bytes, &info.Text)
	if errors.Is(err, sql.ErrNoRows) {
		return ArtifactInfo{}, ErrNotFound
	}
	return info, err
}

func (r *Repository) readRangeLocked(ctx context.Context, info ArtifactInfo, offset int64, limit int) ([]byte, error) {
	if offset < 0 || offset > info.Bytes || limit < 1 || limit > ChunkBytes+4 {
		return nil, errors.New("artifact range outside bounded byte limits")
	}
	end := offset + int64(limit)
	if end > info.Bytes {
		end = info.Bytes
	}
	if end == offset {
		return []byte{}, nil
	}
	rows, err := r.db.QueryContext(ctx, "SELECT chunk_index,data,sha256 FROM artifact_chunks WHERE artifact_id=? AND chunk_index>=? AND chunk_index<=? ORDER BY chunk_index", info.ID, offset/ChunkBytes, (end-1)/ChunkBytes)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	data := make([]byte, 0, end-offset)
	expected := offset / ChunkBytes
	for rows.Next() {
		var index int64
		var chunk []byte
		var sha string
		if err = rows.Scan(&index, &chunk, &sha); err != nil {
			return nil, err
		}
		if index != expected || len(chunk) > ChunkBytes || digest(chunk) != sha {
			return nil, errors.New("artifact chunk integrity verification failed")
		}
		expected++
		start := int64(0)
		if offset > index*ChunkBytes {
			start = offset - index*ChunkBytes
		}
		finish := int64(len(chunk))
		if end < index*ChunkBytes+finish {
			finish = end - index*ChunkBytes
		}
		if start > finish {
			return nil, errors.New("artifact chunk length mismatch")
		}
		data = append(data, chunk[start:finish]...)
	}
	if err = rows.Err(); err != nil {
		return nil, err
	}
	if int64(len(data)) != end-offset {
		return nil, errors.New("artifact chunk is missing")
	}
	return data, nil
}

func (r *Repository) ReadArtifact(ctx context.Context, owner, id, uri string, offset int64, limit int) (ArtifactPage, error) {
	if limit < 1 || limit > ChunkBytes {
		return ArtifactPage{}, errors.New("artifact page must be 1..4096 bytes")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return ArtifactPage{}, ErrClosed
	}
	info, err := r.artifactInfoLocked(ctx, owner, id, uri)
	if err != nil {
		return ArtifactPage{}, err
	}
	data, err := r.readRangeLocked(ctx, info, offset, limit)
	if err != nil {
		return ArtifactPage{}, err
	}
	next := offset + int64(len(data))
	return ArtifactPage{Info: info, Offset: offset, NextOffset: next, Data: data, Truncated: next < info.Bytes}, nil
}

func (r *Repository) readFullLocked(ctx context.Context, key string, bytesLimit int) ([]byte, error) {
	var total int64
	var sha string
	if err := r.db.QueryRowContext(ctx, "SELECT bytes,sha256 FROM artifacts WHERE id=?", key).Scan(&total, &sha); err != nil {
		return nil, err
	}
	if total > int64(bytesLimit) || total < 0 {
		return nil, errors.New("runtime record body budget exceeded")
	}
	rows, err := r.db.QueryContext(ctx, "SELECT chunk_index,data,sha256 FROM artifact_chunks WHERE artifact_id=? ORDER BY chunk_index", key)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	data := make([]byte, 0, int(total))
	expected := 0
	for rows.Next() {
		var index int
		var chunk []byte
		var hash string
		if err = rows.Scan(&index, &chunk, &hash); err != nil {
			return nil, err
		}
		if index != expected || len(chunk) > ChunkBytes || digest(chunk) != hash {
			return nil, errors.New("record artifact chunk integrity verification failed")
		}
		expected++
		if len(data)+len(chunk) > bytesLimit {
			return nil, errors.New("runtime record body budget exceeded")
		}
		data = append(data, chunk...)
	}
	if err = rows.Err(); err != nil {
		return nil, err
	}
	if int64(len(data)) != total || digest(data) != sha {
		return nil, errors.New("record artifact full hash verification failed")
	}
	return data, nil
}

type References struct {
	repository *Repository
	owner, id  string
}

func (r *Repository) BoundReferences(ctx context.Context, owner, id string) (*References, error) {
	if _, err := r.OpenSession(ctx, owner, id); err != nil {
		return nil, err
	}
	return &References{repository: r, owner: owner, id: id}, nil
}
func (b *References) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	info, err := b.repository.PutArtifact(ctx, b.owner, b.id, ArtifactInput{Kind: "context-text", MediaType: "text/plain; charset=utf-8", Processing: "Original UTF-8 text retained in this native session; raw bytes are not automatically sent to the model; no media transformation performed", Data: []byte(text), Text: true})
	if err != nil {
		return wire.Reference{}, err
	}
	return info.Reference(), nil
}
func (b *References) ReadText(ctx context.Context, uri string, offset int64, limit int) (agentcontext.TextPage, error) {
	if limit < 1 || limit > ChunkBytes {
		return agentcontext.TextPage{}, errors.New("text page must be 1..4096 bytes")
	}
	r := b.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return agentcontext.TextPage{}, ErrClosed
	}
	info, err := r.artifactInfoLocked(ctx, b.owner, b.id, uri)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	if !info.Text {
		return agentcontext.TextPage{}, errors.New("artifact is not declared UTF-8 text; raw media was not interpreted")
	}
	data, err := r.readRangeLocked(ctx, info, offset, limit+3)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	if len(data) > 0 && !utf8.RuneStart(data[0]) {
		return agentcontext.TextPage{}, errors.New("offset is not a UTF-8 boundary")
	}
	end := limit
	if end > len(data) {
		end = len(data)
	}
	for end > 0 && end < len(data) && !utf8.RuneStart(data[end]) {
		end--
	}
	if end == 0 && len(data) > 0 {
		return agentcontext.TextPage{}, errors.New("page limit cannot contain the next UTF-8 rune")
	}
	data = data[:end]
	if !utf8.Valid(data) {
		return agentcontext.TextPage{}, errors.New("text artifact contains invalid UTF-8")
	}
	next := offset + int64(len(data))
	return agentcontext.TextPage{URI: uri, Text: string(data), Offset: offset, NextOffset: next, TotalBytes: info.Bytes, Truncated: next < info.Bytes}, nil
}

func (b *References) SearchText(ctx context.Context, uri, query string, limit int) ([]agentcontext.TextMatch, error) {
	if query == "" || len(query) > 256 || !utf8.ValidString(query) || limit < 1 || limit > 20 {
		return nil, errors.New("literal search requires 1..256 UTF-8 bytes and 1..20 matches")
	}
	r := b.repository
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	info, err := r.artifactInfoLocked(ctx, b.owner, b.id, uri)
	if err != nil {
		return nil, err
	}
	if !info.Text {
		return nil, errors.New("artifact is not text")
	}
	return r.searchArtifactLocked(ctx, info, query, limit)
}

func (r *Repository) searchArtifactLocked(ctx context.Context, info ArtifactInfo, query string, limit int) ([]agentcontext.TextMatch, error) {
	rows, err := r.db.QueryContext(ctx, "SELECT chunk_index,data,sha256 FROM artifact_chunks WHERE artifact_id=? ORDER BY chunk_index", info.ID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var matches []agentcontext.TextMatch
	var carry string
	var expected int64
	var nextMatch int64
	for rows.Next() {
		var index int64
		var data []byte
		var sha string
		if err = rows.Scan(&index, &data, &sha); err != nil {
			return nil, err
		}
		if index != expected || len(data) > ChunkBytes || digest(data) != sha {
			return nil, errors.New("search artifact chunk integrity verification failed")
		}
		expected++
		window := carry + string(data)
		base := index*ChunkBytes - int64(len(carry))
		position := 0
		for position <= len(window) {
			found := strings.Index(window[position:], query)
			if found < 0 {
				break
			}
			found += position
			absolute := base + int64(found)
			position = found + 1
			if absolute < nextMatch {
				continue
			}
			nextMatch = absolute + 1
			end := found + 256
			if end > len(window) {
				end = len(window)
			}
			for end > found && end < len(window) && !utf8.RuneStart(window[end]) {
				end--
			}
			snippet := window[found:end]
			for dropped := 0; !utf8.ValidString(snippet) && dropped < 3 && len(snippet) > 0; dropped++ {
				snippet = snippet[:len(snippet)-1]
			}
			if !utf8.ValidString(snippet) {
				return nil, errors.New("artifact search text is invalid UTF-8")
			}
			matches = append(matches, agentcontext.TextMatch{URI: info.Reference().URI, Text: snippet, Offset: absolute})
			if len(matches) == limit {
				return matches, nil
			}
		}
		keep := len(query) - 1
		if keep > len(window) {
			keep = len(window)
		}
		carry = window[len(window)-keep:]
	}
	if err = rows.Err(); err != nil {
		return nil, err
	}
	if expected != (info.Bytes+ChunkBytes-1)/ChunkBytes {
		return nil, fmt.Errorf("artifact search has missing chunks")
	}
	return matches, nil
}
