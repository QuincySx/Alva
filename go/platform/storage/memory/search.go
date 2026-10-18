// INPUT: context, container/heap, encoding/binary, errors, math, sort, strings, agent/memory.
// OUTPUT: Backend.FTSSearch and Backend.VectorSearch.
// POS: [platform / durable storage] Actual FTS5/BM25 and bounded brute-force cosine retrieval, never a fake vector index.
package memorysqlite

import (
	"container/heap"
	"context"
	"encoding/binary"
	"errors"
	"math"
	"sort"
	"strings"

	"github.com/QuincySx/Alva/go/sdk/agent/memory"
)

func encodeVector(vector []float32) ([]byte, error) {
	if len(vector) > 4096 {
		return nil, errors.New("embedding dimensions exceed 4096")
	}
	data := make([]byte, len(vector)*4)
	for index, value := range vector {
		if math.IsNaN(float64(value)) || math.IsInf(float64(value), 0) {
			return nil, errors.New("embedding contains nonfinite value")
		}
		binary.LittleEndian.PutUint32(data[index*4:], math.Float32bits(value))
	}
	return data, nil
}
func decodeVector(data []byte) ([]float32, error) {
	if len(data)%4 != 0 || len(data) > 4096*4 {
		return nil, errors.New("invalid persisted embedding dimensions")
	}
	vector := make([]float32, len(data)/4)
	for index := range vector {
		vector[index] = math.Float32frombits(binary.LittleEndian.Uint32(data[index*4:]))
	}
	if _, err := encodeVector(vector); err != nil {
		return nil, err
	}
	return vector, nil
}
func queryLimit(limit int) error {
	if limit < 0 || limit > 200 {
		return errors.New("persistent memory result limit must be 0..200")
	}
	return nil
}
func (b *Backend) FTSSearch(ctx context.Context, query string, limit int) ([]memory.Entry, error) {
	if err := queryLimit(limit); err != nil {
		return nil, err
	}
	if !bounded(query, 1024) {
		return nil, errors.New("FTS query must be at most 1024 UTF-8 bytes")
	}
	if strings.TrimSpace(query) == "" || limit == 0 {
		return nil, ctx.Err()
	}
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return nil, ErrClosed
	}
	rows, err := b.store.db.QueryContext(ctx, `SELECT mc.id,mc.path,mc.text,mc.start_line,mc.end_line,bm25(chunks_fts) FROM chunks_fts JOIN memory_chunks mc ON mc.id=chunks_fts.rowid WHERE chunks_fts MATCH ? AND mc.namespace=? ORDER BY bm25(chunks_fts),mc.id LIMIT ?`, query, b.namespace, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var entries []memory.Entry
	bytes := 0
	for rows.Next() {
		var entry memory.Entry
		if err := rows.Scan(&entry.ChunkID, &entry.Path, &entry.Text, &entry.StartLine, &entry.EndLine, &entry.Score); err != nil {
			return nil, err
		}
		bytes += len(entry.Text) + len(entry.Path)
		if bytes > 4<<20 {
			return nil, ErrBudget
		}
		entries = append(entries, entry)
	}
	return entries, rows.Err()
}
func cosine(a, b []float32) float64 {
	var dot, left, right float64
	for index, value := range a {
		av, bv := float64(value), float64(b[index])
		dot += av * bv
		left += av * av
		right += bv * bv
	}
	if left == 0 || right == 0 {
		return 0
	}
	return dot / math.Sqrt(left*right)
}

type topEntries []memory.Entry

func (h topEntries) Len() int { return len(h) }
func (h topEntries) Less(i, j int) bool {
	if h[i].Score == h[j].Score {
		return h[i].ChunkID > h[j].ChunkID
	}
	return h[i].Score < h[j].Score
}
func (h topEntries) Swap(i, j int)   { h[i], h[j] = h[j], h[i] }
func (h *topEntries) Push(value any) { *h = append(*h, value.(memory.Entry)) }
func (h *topEntries) Pop() any       { value := (*h)[len(*h)-1]; *h = (*h)[:len(*h)-1]; return value }
func (b *Backend) VectorSearch(ctx context.Context, query []float32, limit int) ([]memory.Entry, error) {
	if err := queryLimit(limit); err != nil {
		return nil, err
	}
	if _, err := encodeVector(query); err != nil {
		return nil, err
	}
	if len(query) == 0 || limit == 0 {
		return nil, ctx.Err()
	}
	b.store.mu.Lock()
	defer b.store.mu.Unlock()
	if b.store.closed {
		return nil, ErrClosed
	}
	rows, err := b.store.db.QueryContext(ctx, "SELECT id,path,text,start_line,end_line,embedding FROM memory_chunks WHERE namespace=? AND length(embedding)=? ORDER BY id LIMIT 10001", b.namespace, len(query)*4)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	top := &topEntries{}
	count, bytes := 0, 0
	for rows.Next() {
		var entry memory.Entry
		var data []byte
		if err := rows.Scan(&entry.ChunkID, &entry.Path, &entry.Text, &entry.StartLine, &entry.EndLine, &data); err != nil {
			return nil, err
		}
		count++
		bytes += len(data) + len(entry.Text) + len(entry.Path)
		if count > 10000 || bytes > 64<<20 {
			return nil, ErrBudget
		}
		vector, err := decodeVector(data)
		if err != nil {
			return nil, err
		}
		entry.Score = cosine(query, vector)
		heap.Push(top, entry)
		if top.Len() > limit {
			heap.Pop(top)
		}
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	result := []memory.Entry(*top)
	sort.Slice(result, func(i, j int) bool {
		if result[i].Score == result[j].Score {
			return result[i].ChunkID < result[j].ChunkID
		}
		return result[i].Score > result[j].Score
	})
	bytes = 0
	for _, entry := range result {
		bytes += len(entry.Text) + len(entry.Path)
	}
	if bytes > 4<<20 {
		return nil, ErrBudget
	}
	return result, nil
}
