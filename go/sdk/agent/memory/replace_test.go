// INPUT: context, errors, testing
// OUTPUT: Atomic replacement, complete-batch validation and nontransactional checkpoint regressions
// POS: [L3 capability library] Prevents partial indexing from advertising a committed new file hash.
package memory

import (
	"context"
	"errors"
	"testing"
)

func TestMemoryReplaceFileAtomicValidationAndOwnership(t *testing.T) {
	backend := NewInMemoryBackend()
	file := File{Path: "notes", Hash: "old", Size: 3}
	old := Chunk{Path: "notes", StartLine: 1, EndLine: 1, Text: "old", Embedding: []float32{1, 0}}
	if err := backend.ReplaceFile(context.Background(), file, []Chunk{old}); err != nil {
		t.Fatal(err)
	}
	file.Hash = "new"
	newChunk := old
	newChunk.Text = "new"
	newChunk.Embedding = []float32{0, 1}
	invalid := newChunk
	invalid.Path = "foreign"
	if err := backend.ReplaceFile(context.Background(), file, []Chunk{newChunk, invalid}); err == nil {
		t.Fatal("partial invalid replacement accepted")
	}
	stored, _, _ := backend.GetFile(context.Background(), "notes")
	matches, _ := backend.FTSSearch(context.Background(), "old", 10)
	if stored.Hash != "old" || len(matches) != 1 {
		t.Fatal("failed batch changed existing revision", stored, matches)
	}
	if err := backend.ReplaceFile(context.Background(), file, []Chunk{newChunk}); err != nil {
		t.Fatal(err)
	}
	newChunk.Embedding[1] = 99
	vectors, _ := backend.VectorSearch(context.Background(), []float32{0, 1}, 10)
	if len(vectors) != 1 || vectors[0].Text != "new" || vectors[0].Score != 1 {
		t.Fatal("replacement not detached", vectors)
	}
}

type nonTransactionalFixture struct {
	Backend
	insertErr error
	upserts   int
}

func (backend *nonTransactionalFixture) InsertChunk(context.Context, Chunk) (int64, error) {
	return 0, backend.insertErr
}
func (backend *nonTransactionalFixture) UpsertFile(ctx context.Context, file File) error {
	backend.upserts++
	return backend.Backend.UpsertFile(ctx, file)
}
func TestServiceFallbackNeverCheckpointsHashAfterChunkFailure(t *testing.T) {
	inner := NewInMemoryBackend()
	_ = inner.UpsertFile(context.Background(), File{Path: "notes", Hash: "old", Size: 3})
	fixture := errors.New("chunk write failure")
	backend := &nonTransactionalFixture{Backend: inner, insertErr: fixture}
	service := NewService(backend, nil)
	if err := service.StoreEntry(context.Background(), "notes", "new", "fact"); !errors.Is(err, fixture) {
		t.Fatal(err)
	}
	stored, _, _ := inner.GetFile(context.Background(), "notes")
	if backend.upserts != 0 || stored.Hash != "old" {
		t.Fatal("partial index advertised new hash", stored)
	}
}

type atomicFixture struct {
	Backend
	calls int
	err   error
}

func (backend *atomicFixture) ReplaceFile(context.Context, File, []Chunk) error {
	backend.calls++
	return backend.err
}
func TestServicePrefersActualAtomicReplacement(t *testing.T) {
	fixture := errors.New("transaction failed")
	backend := &atomicFixture{Backend: NewInMemoryBackend(), err: fixture}
	if err := NewService(backend, nil).StoreEntry(context.Background(), "notes", "text", "fact"); !errors.Is(err, fixture) || backend.calls != 1 {
		t.Fatal(err, backend.calls)
	}
	if _, found, _ := backend.GetFile(context.Background(), "notes"); found {
		t.Fatal("fallback ran after transaction failure")
	}
}
