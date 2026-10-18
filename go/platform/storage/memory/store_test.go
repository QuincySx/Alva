// INPUT: context, math, filepath, strings, testing, agent/memory.
// OUTPUT: Real persistent FTS5/vector/cache, namespace isolation, restart and validation regressions.
// POS: [platform / durable storage] Native memory backend acceptance without embeddings services or workspace discovery.
package memorysqlite

import (
	"context"
	"math"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/memory"
)

func TestRealMemoryFTSVectorCacheAndRestart(t *testing.T) {
	ctx := context.Background()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	backend, err := store.Bind("workspace")
	if err != nil {
		t.Fatal(err)
	}
	file := memory.File{Path: "notes/project", Source: "project", Hash: "hash", Size: 32}
	if err = backend.UpsertFile(ctx, file); err != nil {
		t.Fatal(err)
	}
	first, err := backend.InsertChunk(ctx, memory.Chunk{Path: file.Path, StartLine: 1, EndLine: 1, Text: "needle logs 日志", Embedding: []float32{1, 0}})
	if err != nil {
		t.Fatal(err)
	}
	_, err = backend.InsertChunk(ctx, memory.Chunk{Path: file.Path, StartLine: 2, EndLine: 2, Text: "second needle", Embedding: []float32{0, 1}})
	if err != nil {
		t.Fatal(err)
	}
	fts, err := backend.FTSSearch(ctx, "needle", 5)
	if err != nil || len(fts) != 2 {
		t.Fatalf("real FTS5 not working: %+v %v", fts, err)
	}
	vector, err := backend.VectorSearch(ctx, []float32{1, 0}, 1)
	if err != nil || len(vector) != 1 || vector[0].ChunkID != first || vector[0].Score != 1 {
		t.Fatalf("cosine ranking wrong: %+v %v", vector, err)
	}
	if err = backend.CacheEmbedding(ctx, "local-fixture", "hash", []float32{1, 0}); err != nil {
		t.Fatal(err)
	}
	other, err := store.Bind("other-workspace")
	if err != nil {
		t.Fatal(err)
	}
	hidden, err := other.FTSSearch(ctx, "needle", 5)
	if err != nil || len(hidden) != 0 {
		t.Fatal("foreign namespace visible", err)
	}
	if _, found, err := other.GetCachedEmbedding(ctx, "local-fixture", "hash"); err != nil || found {
		t.Fatal("hash gave foreign embedding", err)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = Open(ctx, directory)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	backend, _ = store.Bind("workspace")
	restored, ok, err := backend.GetFile(ctx, file.Path)
	if err != nil || !ok || restored != file {
		t.Fatal("file metadata not durable", err)
	}
	cached, ok, err := backend.GetCachedEmbedding(ctx, "local-fixture", "hash")
	if err != nil || !ok || len(cached) != 2 || cached[0] != 1 {
		t.Fatal("embedding not durable", err)
	}
	if err = backend.DeleteChunksForPath(ctx, file.Path); err != nil {
		t.Fatal(err)
	}
	fts, err = backend.FTSSearch(ctx, "needle", 5)
	if err != nil || len(fts) != 0 {
		t.Fatal("FTS trigger kept deleted content", err)
	}
}
func TestMemoryBackendLimitsAndMalformedQueriesFailExplicitly(t *testing.T) {
	ctx := context.Background()
	store, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	backend, _ := store.Bind("namespace")
	if _, err = store.Bind("../other"); err == nil {
		t.Fatal("namespace accepted path")
	}
	if err = backend.UpsertFile(ctx, memory.File{Path: "file", Size: 1}); err != nil {
		t.Fatal(err)
	}
	for _, chunk := range []memory.Chunk{{Path: "file", StartLine: 1, EndLine: 1, Text: strings.Repeat("x", MaxChunkBytes+1)}, {Path: "file", StartLine: 1, EndLine: 1, Embedding: []float32{float32(math.NaN())}}} {
		if _, err = backend.InsertChunk(ctx, chunk); err == nil {
			t.Fatal("invalid chunk accepted")
		}
	}
	if _, err = backend.FTSSearch(ctx, `"unterminated`, 5); err == nil {
		t.Fatal("malformed FTS became fake empty success")
	}
	if _, err = backend.VectorSearch(ctx, []float32{1}, 201); err == nil {
		t.Fatal("query limit bypassed")
	}
}

func TestAtomicReplacePrevalidationAndFTSCommit(t *testing.T) {
	ctx := context.Background()
	store, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	backend, _ := store.Bind("project")
	file := memory.File{Path: "memory-entry", Hash: "old", Size: 3}
	if err = backend.ReplaceFile(ctx, file, []memory.Chunk{{Path: file.Path, StartLine: 1, EndLine: 1, Text: "oldmarker"}}); err != nil {
		t.Fatal(err)
	}
	file.Hash = "new"
	if err = backend.ReplaceFile(ctx, file, []memory.Chunk{{Path: file.Path, StartLine: 1, EndLine: 1, Text: "newmarker"}, {Path: "foreign", StartLine: 2, EndLine: 2, Text: "invalid"}}); err == nil {
		t.Fatal("invalid replacement accepted")
	}
	metadata, ok, err := backend.GetFile(ctx, file.Path)
	if err != nil || !ok || metadata.Hash != "old" {
		t.Fatal("failed replacement advanced checkpoint", err)
	}
	old, err := backend.FTSSearch(ctx, "oldmarker", 10)
	if err != nil || len(old) != 1 {
		t.Fatal("failed replacement deleted prior FTS", err)
	}
	if err = backend.ReplaceFile(ctx, file, []memory.Chunk{{Path: file.Path, StartLine: 1, EndLine: 1, Text: "newmarker"}}); err != nil {
		t.Fatal(err)
	}
	old, err = backend.FTSSearch(ctx, "oldmarker", 10)
	if err != nil || len(old) != 0 {
		t.Fatal("committed replacement retained stale FTS", err)
	}
	newEntries, err := backend.FTSSearch(ctx, "newmarker", 10)
	if err != nil || len(newEntries) != 1 {
		t.Fatal("committed replacement not indexed", err)
	}
}
