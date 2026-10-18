// INPUT: context, errors, math, testing
// OUTPUT: Backend ownership, fallback, hybrid scoring and replacement regressions
// POS: [L3 capability library] Proves default memory does real retrieval without inventing embeddings or relevance.
package memory

import (
	"context"
	"errors"
	"math"
	"testing"
)

type embedder struct{ fail bool }

func (embedder) Model() string { return "test" }
func (e embedder) Embed(_ context.Context, texts []string) ([][]float32, error) {
	if e.fail {
		return nil, errors.New("embedding fixture")
	}
	result := make([][]float32, len(texts))
	for i, text := range texts {
		if text == "apple" {
			result[i] = []float32{1, 0}
		} else {
			result[i] = []float32{0, 1}
		}
	}
	return result, nil
}
func TestNoopSearchDoesNotInventVectorRelevance(t *testing.T) {
	service := NewService(NewInMemoryBackend(), NoopEmbeddingProvider{})
	ctx := context.Background()
	for key, text := range map[string]string{"a": "apple orchard", "b": "unrelated database"} {
		if err := service.StoreEntry(ctx, key, text, "fact"); err != nil {
			t.Fatal(err)
		}
	}
	result, err := service.Search(ctx, "apple", 10)
	if err != nil || len(result) != 1 || result[0].Path != "a" {
		t.Fatal(result, err)
	}
}
func TestMemoryReplacementAndFailedEmbedderFallback(t *testing.T) {
	service := NewService(NewInMemoryBackend(), embedder{fail: true})
	ctx := context.Background()
	if err := service.StoreEntry(ctx, "key", "old keyword", "fact"); err != nil {
		t.Fatal(err)
	}
	if err := service.StoreEntry(ctx, "key", "new content", "fact"); err != nil {
		t.Fatal(err)
	}
	old, _ := service.Search(ctx, "keyword", 10)
	next, _ := service.Search(ctx, "new", 10)
	if len(old) != 0 || len(next) != 1 {
		t.Fatal(old, next)
	}
}
func TestVectorSearchOwnsEmbeddingsAndRejectsNonFinite(t *testing.T) {
	backend := NewInMemoryBackend()
	vector := []float32{1, 0}
	id, err := backend.InsertChunk(context.Background(), Chunk{Path: "a", Text: "apple", StartLine: 1, EndLine: 1, Embedding: vector})
	if err != nil {
		t.Fatal(err)
	}
	vector[0] = 0
	results, err := backend.VectorSearch(context.Background(), []float32{1, 0}, 5)
	if err != nil || len(results) != 1 || results[0].ChunkID != id || results[0].Score != 1 {
		t.Fatal(results, err)
	}
	if _, err := backend.VectorSearch(context.Background(), []float32{float32(math.NaN())}, 5); err == nil {
		t.Fatal("nonfinite query accepted")
	}
}
func TestHybridSearchRanksActualSemanticMatch(t *testing.T) {
	service := NewService(NewInMemoryBackend(), embedder{})
	ctx := context.Background()
	service.StoreEntry(ctx, "a", "apple", "fact")
	service.StoreEntry(ctx, "b", "other", "fact")
	result, err := service.Search(ctx, "apple", 1)
	if err != nil || len(result) != 1 || result[0].Path != "a" {
		t.Fatal(result, err)
	}
}
