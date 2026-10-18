// INPUT: context, crypto/sha256, encoding/hex, errors, math, sort, strings, sync, time
// OUTPUT: Backend, EmbeddingProvider, InMemoryBackend, Service, File, Chunk, Entry
// POS: [L3 capability library] Pluggable process-lifetime memory with literal full-text/vector search and weighted hybrid retrieval.
package memory

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"math"
	"sort"
	"strings"
	"sync"
	"time"
)

type File struct {
	Path, Source, Hash, MTime string
	Size                      int64
}
type Chunk struct {
	ID                 int64
	Path, Source       string
	StartLine, EndLine int64
	Hash, Text         string
	Embedding          []float32
}
type Entry struct {
	ChunkID            int64
	Path, Text         string
	Score              float64
	StartLine, EndLine int64
}
type SyncReport struct{ FilesScanned, FilesAdded, FilesUpdated, FilesUnchanged, ChunksCreated int }
type Backend interface {
	UpsertFile(context.Context, File) error
	GetFile(context.Context, string) (File, bool, error)
	InsertChunk(context.Context, Chunk) (int64, error)
	DeleteChunksForPath(context.Context, string) error
	FTSSearch(context.Context, string, int) ([]Entry, error)
	VectorSearch(context.Context, []float32, int) ([]Entry, error)
	CacheEmbedding(context.Context, string, string, []float32) error
}

// FileReplacer atomically replaces the file metadata and its complete chunk set.
// Backend's individual CRUD methods alone do not imply this stronger guarantee.
type FileReplacer interface {
	ReplaceFile(context.Context, File, []Chunk) error
}
type EmbeddingProvider interface {
	Model() string
	Embed(context.Context, []string) ([][]float32, error)
}
type NoopEmbeddingProvider struct{}

func (NoopEmbeddingProvider) Model() string { return "noop" }
func (NoopEmbeddingProvider) Embed(ctx context.Context, texts []string) ([][]float32, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return make([][]float32, len(texts)), nil
}

type InMemoryBackend struct {
	mu         sync.RWMutex
	next       int64
	files      map[string]File
	chunks     map[int64]Chunk
	embeddings map[string][]float32
}

func NewInMemoryBackend() *InMemoryBackend {
	return &InMemoryBackend{files: make(map[string]File), chunks: make(map[int64]Chunk), embeddings: make(map[string][]float32)}
}
func (backend *InMemoryBackend) UpsertFile(ctx context.Context, file File) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if file.Path == "" || file.Size < 0 {
		return errors.New("invalid memory file")
	}
	backend.mu.Lock()
	defer backend.mu.Unlock()
	backend.files[file.Path] = file
	return nil
}
func (backend *InMemoryBackend) GetFile(ctx context.Context, path string) (File, bool, error) {
	if err := ctx.Err(); err != nil {
		return File{}, false, err
	}
	backend.mu.RLock()
	defer backend.mu.RUnlock()
	file, ok := backend.files[path]
	return file, ok, nil
}
func (backend *InMemoryBackend) InsertChunk(ctx context.Context, chunk Chunk) (int64, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	if chunk.Path == "" || chunk.StartLine < 1 || chunk.EndLine < chunk.StartLine || !validVector(chunk.Embedding) {
		return 0, errors.New("invalid memory chunk")
	}
	backend.mu.Lock()
	defer backend.mu.Unlock()
	backend.next++
	chunk.ID = backend.next
	chunk.Embedding = append([]float32(nil), chunk.Embedding...)
	backend.chunks[chunk.ID] = chunk
	return chunk.ID, nil
}
func (backend *InMemoryBackend) DeleteChunksForPath(ctx context.Context, path string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	backend.mu.Lock()
	defer backend.mu.Unlock()
	for id, chunk := range backend.chunks {
		if chunk.Path == path {
			delete(backend.chunks, id)
		}
	}
	return nil
}
func (backend *InMemoryBackend) ReplaceFile(ctx context.Context, file File, chunks []Chunk) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if file.Path == "" || file.Size < 0 {
		return errors.New("invalid memory file")
	}
	owned := make([]Chunk, len(chunks))
	for i, chunk := range chunks {
		if chunk.Path != file.Path || chunk.StartLine < 1 || chunk.EndLine < chunk.StartLine || !validVector(chunk.Embedding) {
			return errors.New("invalid replacement memory chunk")
		}
		owned[i] = chunk
		owned[i].Embedding = append([]float32(nil), chunk.Embedding...)
	}
	backend.mu.Lock()
	defer backend.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if int64(len(owned)) > int64(^uint64(0)>>1)-backend.next {
		return errors.New("memory chunk identity exhausted")
	}
	for id, chunk := range backend.chunks {
		if chunk.Path == file.Path {
			delete(backend.chunks, id)
		}
	}
	for _, chunk := range owned {
		backend.next++
		chunk.ID = backend.next
		backend.chunks[chunk.ID] = chunk
	}
	backend.files[file.Path] = file
	return nil
}
func (backend *InMemoryBackend) CacheEmbedding(ctx context.Context, model, hash string, embedding []float32) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if model == "" || hash == "" || !validVector(embedding) {
		return errors.New("invalid embedding cache entry")
	}
	backend.mu.Lock()
	defer backend.mu.Unlock()
	backend.embeddings[model+"\x00"+hash] = append([]float32(nil), embedding...)
	return nil
}
func (backend *InMemoryBackend) FTSSearch(ctx context.Context, query string, limit int) ([]Entry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if limit < 0 {
		return nil, errors.New("negative memory result limit")
	}
	terms := strings.Fields(strings.ToLower(query))
	if len(terms) == 0 || limit == 0 {
		return nil, nil
	}
	backend.mu.RLock()
	defer backend.mu.RUnlock()
	var entries []Entry
	for _, chunk := range backend.chunks {
		score := 0
		for _, term := range terms {
			score += strings.Count(strings.ToLower(chunk.Text), term)
		}
		if score > 0 {
			entries = append(entries, entry(chunk, -float64(score)))
		}
	}
	sort.Slice(entries, func(i, j int) bool {
		if entries[i].Score == entries[j].Score {
			return entries[i].ChunkID < entries[j].ChunkID
		}
		return entries[i].Score < entries[j].Score
	})
	if len(entries) > limit {
		entries = entries[:limit]
	}
	return entries, nil
}
func (backend *InMemoryBackend) VectorSearch(ctx context.Context, query []float32, limit int) ([]Entry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if limit < 0 || !validVector(query) {
		return nil, errors.New("invalid vector query")
	}
	if len(query) == 0 || limit == 0 {
		return nil, nil
	}
	backend.mu.RLock()
	defer backend.mu.RUnlock()
	var entries []Entry
	for _, chunk := range backend.chunks {
		if len(chunk.Embedding) != len(query) || len(chunk.Embedding) == 0 {
			continue
		}
		score := cosine(query, chunk.Embedding)
		entries = append(entries, entry(chunk, score))
	}
	sort.Slice(entries, func(i, j int) bool {
		if entries[i].Score == entries[j].Score {
			return entries[i].ChunkID < entries[j].ChunkID
		}
		return entries[i].Score > entries[j].Score
	})
	if len(entries) > limit {
		entries = entries[:limit]
	}
	return entries, nil
}
func entry(chunk Chunk, score float64) Entry {
	return Entry{ChunkID: chunk.ID, Path: chunk.Path, Text: chunk.Text, Score: score, StartLine: chunk.StartLine, EndLine: chunk.EndLine}
}
func validVector(vector []float32) bool {
	for _, value := range vector {
		if math.IsNaN(float64(value)) || math.IsInf(float64(value), 0) {
			return false
		}
	}
	return true
}
func cosine(left, right []float32) float64 {
	var dot, a, b float64
	for i := range left {
		l, r := float64(left[i]), float64(right[i])
		dot += l * r
		a += l * l
		b += r * r
	}
	if a == 0 || b == 0 {
		return 0
	}
	return dot / math.Sqrt(a*b)
}

type Service struct {
	store                   Backend
	embedder                EmbeddingProvider
	mu                      sync.RWMutex
	ftsWeight, vectorWeight float64
	writeMu                 sync.Mutex
}

func NewService(store Backend, embedder EmbeddingProvider) *Service {
	if store == nil {
		store = NewInMemoryBackend()
	}
	if embedder == nil {
		embedder = NoopEmbeddingProvider{}
	}
	return &Service{store: store, embedder: embedder, ftsWeight: 0.4, vectorWeight: 0.6}
}
func (service *Service) Store() Backend              { return service.store }
func (service *Service) Embedder() EmbeddingProvider { return service.embedder }
func (service *Service) SetSearchWeights(fts, vector float64) error {
	if math.IsNaN(fts) || math.IsNaN(vector) || math.IsInf(fts+vector, 0) || fts < 0 || vector < 0 || fts+vector <= 0 {
		return errors.New("invalid hybrid weights")
	}
	service.mu.Lock()
	service.ftsWeight, service.vectorWeight = fts, vector
	service.mu.Unlock()
	return nil
}
func (service *Service) StoreEntry(ctx context.Context, key, content, category string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if key == "" {
		return errors.New("memory key required")
	}
	sum := sha256.Sum256([]byte(content))
	hash := hex.EncodeToString(sum[:])
	embeddings, embedErr := service.embedder.Embed(ctx, []string{content})
	if ctx.Err() != nil {
		return ctx.Err()
	}
	var vector []float32
	if embedErr == nil && len(embeddings) > 0 && validVector(embeddings[0]) {
		vector = embeddings[0]
	}
	service.writeMu.Lock()
	defer service.writeMu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	file := File{Path: key, Source: category, Hash: hash, MTime: time.Now().UTC().Format(time.RFC3339Nano), Size: int64(len(content))}
	if len(vector) > 0 {
		if err := service.store.CacheEmbedding(ctx, service.embedder.Model(), hash, vector); err != nil {
			return err
		}
		if err := ctx.Err(); err != nil {
			return err
		}
	}
	lines := int64(strings.Count(content, "\n") + 1)
	chunk := Chunk{Path: key, Source: category, StartLine: 1, EndLine: lines, Hash: hash, Text: content, Embedding: vector}
	if replacer, ok := service.store.(FileReplacer); ok {
		return replacer.ReplaceFile(ctx, file, []Chunk{chunk})
	}
	// Nontransactional backends remain nontransactional. Commit the new hash
	// last so partial indexing failures cannot advertise a completed revision.
	if err := service.store.DeleteChunksForPath(ctx, key); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if _, err := service.store.InsertChunk(ctx, chunk); err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	return service.store.UpsertFile(ctx, file)
}
func (service *Service) Search(ctx context.Context, query string, limit int) ([]Entry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if limit < 0 || limit > 10000 {
		return nil, errors.New("invalid memory result limit")
	}
	if limit == 0 {
		return nil, nil
	}
	service.mu.RLock()
	fw, vw := service.ftsWeight, service.vectorWeight
	service.mu.RUnlock()
	var fts, vector []Entry
	var err error
	if fw > 0 {
		fts, err = service.store.FTSSearch(ctx, query, limit*2)
		if err = errors.Join(err, ctx.Err()); err != nil {
			return nil, err
		}
	}
	if vw > 0 {
		embeddings, embedErr := service.embedder.Embed(ctx, []string{query})
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if embedErr == nil && len(embeddings) > 0 && len(embeddings[0]) > 0 && validVector(embeddings[0]) {
			vector, err = service.store.VectorSearch(ctx, embeddings[0], limit*2)
			if err = errors.Join(err, ctx.Err()); err != nil {
				return nil, err
			}
		}
	}
	merged := make(map[int64]Entry)
	for _, input := range []struct {
		entries []Entry
		weight  float64
		invert  bool
	}{{fts, fw, true}, {vector, vw, false}} {
		if len(input.entries) == 0 {
			continue
		}
		min, max := input.entries[0].Score, input.entries[0].Score
		for _, entry := range input.entries {
			if entry.Score < min {
				min = entry.Score
			}
			if entry.Score > max {
				max = entry.Score
			}
		}
		for _, entry := range input.entries {
			score := 1.0
			if max > min {
				score = (entry.Score - min) / (max - min)
				if input.invert {
					score = 1 - score
				}
			}
			if previous, ok := merged[entry.ChunkID]; ok {
				previous.Score += score * input.weight
				merged[entry.ChunkID] = previous
			} else {
				entry.Score = score * input.weight
				merged[entry.ChunkID] = entry
			}
		}
	}
	result := make([]Entry, 0, len(merged))
	for _, entry := range merged {
		result = append(result, entry)
	}
	sort.Slice(result, func(i, j int) bool {
		if result[i].Score == result[j].Score {
			return result[i].ChunkID < result[j].ChunkID
		}
		return result[i].Score > result[j].Score
	})
	if len(result) > limit {
		result = result[:limit]
	}
	return result, nil
}
