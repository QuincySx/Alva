// INPUT: context, errors, math, testing
// OUTPUT: Disabled retrieval modality and cancellation boundary regressions
// POS: [L3 capability library] Verifies search policies avoid disabled services and cancelled indexing never advances into another storage stage.
package memory

import (
	"context"
	"errors"
	"math"
	"testing"
)

type searchBackend struct {
	Backend
	fts, vectors int
	afterFTS     func()
	afterVector  func()
	afterDelete  func()
	inserts      int
}

func (backend *searchBackend) FTSSearch(context.Context, string, int) ([]Entry, error) {
	backend.fts++
	if backend.afterFTS != nil {
		backend.afterFTS()
	}
	return []Entry{{ChunkID: 1, Path: "literal", Text: "apple", Score: -1}}, nil
}
func (backend *searchBackend) VectorSearch(context.Context, []float32, int) ([]Entry, error) {
	backend.vectors++
	if backend.afterVector != nil {
		backend.afterVector()
	}
	return []Entry{{ChunkID: 2, Path: "semantic", Text: "pear", Score: 1}}, nil
}
func (backend *searchBackend) DeleteChunksForPath(context.Context, string) error {
	if backend.afterDelete != nil {
		backend.afterDelete()
	}
	return nil
}
func (backend *searchBackend) InsertChunk(context.Context, Chunk) (int64, error) {
	backend.inserts++
	return 1, nil
}

type countingEmbedder struct{ calls int }

func (*countingEmbedder) Model() string { return "test" }
func (provider *countingEmbedder) Embed(context.Context, []string) ([][]float32, error) {
	provider.calls++
	return [][]float32{{1, 0}}, nil
}

func TestSearchZeroWeightDisablesModality(t *testing.T) {
	for _, vectorOnly := range []bool{false, true} {
		name := "literal_only"
		if vectorOnly {
			name = "vector_only"
		}
		t.Run(name, func(t *testing.T) {
			backend := &searchBackend{}
			provider := &countingEmbedder{}
			service := NewService(backend, provider)
			ftsWeight, vectorWeight := 1.0, 0.0
			want := "literal"
			if vectorOnly {
				ftsWeight, vectorWeight, want = 0, 1, "semantic"
			}
			if err := service.SetSearchWeights(ftsWeight, vectorWeight); err != nil {
				t.Fatal(err)
			}
			result, err := service.Search(context.Background(), "apple", 10)
			if err != nil || len(result) != 1 || result[0].Path != want || result[0].Score != 1 {
				t.Fatal("disabled modality contributed candidates", result, err)
			}
			if vectorOnly && backend.fts != 0 || !vectorOnly && (backend.vectors != 0 || provider.calls != 0) {
				t.Fatal("disabled modality still invoked external service", backend.fts, backend.vectors, provider.calls)
			}
		})
	}
}

func TestSearchRejectsOverflowingWeightSum(t *testing.T) {
	if err := NewService(nil, nil).SetSearchWeights(math.MaxFloat64, math.MaxFloat64); err == nil {
		t.Fatal("accepted weights that produce infinite merged scores")
	}
}

func TestSearchUsesOneWeightSnapshot(t *testing.T) {
	backend := &searchBackend{}
	provider := &countingEmbedder{}
	service := NewService(backend, provider)
	if err := service.SetSearchWeights(1, 0); err != nil {
		t.Fatal(err)
	}
	backend.afterFTS = func() {
		if err := service.SetSearchWeights(0, 1); err != nil {
			t.Fatal(err)
		}
	}
	first, err := service.Search(context.Background(), "apple", 10)
	if err != nil || len(first) != 1 || first[0].Path != "literal" || provider.calls != 0 {
		t.Fatal("search mixed two configuration revisions", first, err)
	}
	second, err := service.Search(context.Background(), "apple", 10)
	if err != nil || len(second) != 1 || second[0].Path != "semantic" || provider.calls != 1 || backend.fts != 1 {
		t.Fatal("next search ignored accepted new weights", second, err)
	}
}

func TestMemoryServiceDoesNotAdvanceAfterCancellation(t *testing.T) {
	for _, stage := range []string{"before_zero_limit", "after_fts", "after_vector", "after_delete"} {
		t.Run(stage, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			provider := &countingEmbedder{}
			backend := &searchBackend{Backend: NewInMemoryBackend()}
			service := NewService(backend, provider)
			var err error
			switch stage {
			case "before_zero_limit":
				cancel()
				_, err = service.Search(ctx, "apple", 0)
			case "after_fts":
				backend.afterFTS = cancel
				_, err = service.Search(ctx, "apple", 5)
				if provider.calls != 0 {
					t.Error("cancelled search started embedding service")
				}
			case "after_vector":
				backend.afterVector = cancel
				_, err = service.Search(ctx, "apple", 5)
			case "after_delete":
				backend.afterDelete = cancel
				err = service.StoreEntry(ctx, "key", "content", "facts")
				if backend.inserts != 0 {
					t.Error("cancelled replacement started inserting chunks")
				}
			}
			if !errors.Is(err, context.Canceled) {
				t.Fatal("operation reported success after cancellation", err)
			}
		})
	}
}
