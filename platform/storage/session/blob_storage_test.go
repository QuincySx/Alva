// INPUT: Native session transactions, real directory/volatile object stores and fault-injecting byte adapters.
// OUTPUT: Swappable artifact storage acceptance with actual references, history, isolation and rollback boundaries.
// POS: [storage acceptance] Proves external bytes do not become a second transcript or weaken owned native facts.
package session

import (
	"bytes"
	"context"
	"errors"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/blob"
	"github.com/QuincySx/Alva/platform/storage/blob/directory"
	"github.com/QuincySx/Alva/platform/storage/blob/memory"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type observedBlobs struct {
	blob.Store
	puts, reads int
	failPut     bool
	corruptRead bool
	shortRead   bool
}

func (s *observedBlobs) Put(ctx context.Context, key string, data []byte) error {
	s.puts++
	if err := s.Store.Put(ctx, key, data); err != nil {
		return err
	}
	if s.failPut {
		return errors.New("transport failed after publication")
	}
	return nil
}
func (s *observedBlobs) Read(ctx context.Context, key string, offset int64, limit int) ([]byte, error) {
	s.reads++
	data, err := s.Store.Read(ctx, key, offset, limit)
	if err == nil && len(data) > 0 {
		if s.corruptRead {
			data[0] ^= 1
		}
		if s.shortRead {
			data = data[:len(data)-1]
		}
	}
	return data, err
}

func blobMemory(t *testing.T) *observedBlobs {
	t.Helper()
	s, err := memory.New(memory.Limits{Bytes: 4 << 20, Objects: 128})
	if err != nil {
		t.Fatal(err)
	}
	return &observedBlobs{Store: s}
}

func TestArtifactBackendsPreserveNativeContract(t *testing.T) {
	backends := map[string]func(*testing.T) (blob.Store, blob.Retention){
		"memory": func(t *testing.T) (blob.Store, blob.Retention) { return blobMemory(t), blob.Volatile },
	}
	if runtime.GOOS != "windows" {
		backends["directory"] = func(t *testing.T) (blob.Store, blob.Retention) {
			s, err := directory.Open(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				if err := s.Close(); err != nil {
					t.Error(err)
				}
			})
			return s, blob.Persistent
		}
	}
	for name, makeStore := range backends {
		t.Run(name, func(t *testing.T) {
			store, retention := makeStore(t)
			observed := &observedBlobs{Store: store}
			config := BlobConfig{ID: "test-artifacts", Retention: retention, Store: observed}
			dir := filepath.Join(t.TempDir(), "native")
			repo, err := OpenWithBlobs(t.Context(), dir, config)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				if err := repo.Close(); err != nil {
					t.Error(err)
				}
			})
			log, err := repo.Create(t.Context(), "owner", "session", "")
			if err != nil {
				t.Fatal(err)
			}
			if _, err = repo.Create(t.Context(), "owner", "sibling", ""); err != nil {
				t.Fatal(err)
			}
			backend, err := repo.BoundReferences(t.Context(), "owner", log.ID())
			if err != nil {
				t.Fatal(err)
			}
			text := strings.Repeat("a", 4090) + "cross-boundary 中文 " + strings.Repeat("z", 9000)
			ref, err := backend.StoreText(t.Context(), text)
			if err != nil {
				t.Fatal(err)
			}
			if observed.puts != 1 {
				t.Fatal("one immutable upload expected", observed.puts)
			}
			again, err := backend.StoreText(t.Context(), text)
			if err != nil || again.URI != ref.URI || observed.puts != 1 {
				t.Fatal("dedup", err)
			}
			if retention == blob.Volatile && !strings.Contains(ref.Processing, "process-lifetime") {
				t.Fatal("volatile storage claimed durability")
			}
			var inlineBytes int
			if err = repo.db.QueryRowContext(t.Context(), "SELECT coalesce(sum(length(data)),0) FROM artifact_chunks").Scan(&inlineBytes); err != nil || inlineBytes != 0 {
				t.Fatal("external bytes duplicated into SQLite", inlineBytes, err)
			}
			reads := observed.reads
			if _, err = repo.ArtifactMetadata(t.Context(), "owner", log.ID(), ref.URI); err != nil || observed.reads != reads {
				t.Fatal("metadata fetched content", err)
			}
			for _, binding := range []struct{ owner, id string }{{"other", log.ID()}, {"owner", "sibling"}} {
				if _, err = repo.ReadArtifact(t.Context(), binding.owner, binding.id, ref.URI, 0, 64); !errors.Is(err, ErrNotFound) || observed.reads != reads {
					t.Fatal("unauthorized blob lookup", err)
				}
			}
			page, err := backend.ReadText(t.Context(), ref.URI, 4090, 64)
			if err != nil || !strings.HasPrefix(page.Text, "cross-boundary 中文") {
				t.Fatal("reference page", page, err)
			}
			matches, err := backend.SearchText(t.Context(), ref.URI, "cross-boundary 中文", 5)
			if err != nil || len(matches) != 1 || matches[0].Offset != 4090 {
				t.Fatal("cross-chunk search", matches, err)
			}
			_, data, err := repo.ReadArtifactFull(t.Context(), "owner", log.ID(), ref.URI, MaxArtifactBytes)
			if err != nil || string(data) != text {
				t.Fatal("full hash read", err)
			}
			msg := wire.Message{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &ref}}}
			if _, err = log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &msg})}); err != nil {
				t.Fatal(err)
			}
			if observed.puts != 1 {
				t.Fatal("execution facts must stay in atomic fact transaction")
			}
			if err = repo.Close(); err != nil {
				t.Fatal(err)
			}
			repo, err = OpenWithBlobs(t.Context(), dir, config)
			if err != nil {
				t.Fatal(err)
			}
			log, err = repo.OpenSession(t.Context(), "owner", "session")
			if err != nil {
				t.Fatal(err)
			}
			messages, err := log.Messages(t.Context())
			if err != nil || len(messages) != 1 || messages[0].Content[0].Reference.URI != ref.URI {
				t.Fatal("reopened same facts", err)
			}
			_, data, err = repo.ReadArtifactFull(t.Context(), "owner", log.ID(), ref.URI, MaxArtifactBytes)
			if err != nil || string(data) != text {
				t.Fatal("reopened external content", err)
			}
		})
	}
}

func TestBlobPublicationFailureNeverCommitsReferenceOrRetries(t *testing.T) {
	store := blobMemory(t)
	store.failPut = true
	repo, err := OpenWithBlobs(t.Context(), filepath.Join(t.TempDir(), "native"), BlobConfig{ID: "remote", Retention: blob.Persistent, Store: store})
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err = repo.Create(t.Context(), "owner", "session", ""); err != nil {
		t.Fatal(err)
	}
	_, err = repo.PutArtifact(t.Context(), "owner", "session", ArtifactInput{Kind: "output", MediaType: "text/plain", Processing: "actual output", Data: []byte("may already exist remotely"), Text: true})
	if err == nil || store.puts != 1 {
		t.Fatal("uncertain upload must fail without retry", err, store.puts)
	}
	var count int
	if err = repo.db.QueryRowContext(t.Context(), "SELECT count(*) FROM artifacts").Scan(&count); err != nil || count != 0 {
		t.Fatal("partial reference committed", count, err)
	}
}

func TestRemoteFullReadUsesBoundedWindowsWithChunkHashes(t *testing.T) {
	store := blobMemory(t)
	repo, err := OpenWithBlobs(t.Context(), filepath.Join(t.TempDir(), "native"), BlobConfig{ID: "remote", Retention: blob.Volatile, Store: store})
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err = repo.Create(t.Context(), "owner", "session", ""); err != nil {
		t.Fatal(err)
	}
	refs, err := repo.BoundReferences(t.Context(), "owner", "session")
	if err != nil {
		t.Fatal(err)
	}
	text := strings.Repeat("bounded remote bytes ", 8000)
	ref, err := refs.StoreText(t.Context(), text)
	if err != nil {
		t.Fatal(err)
	}
	_, data, err := repo.ReadArtifactFull(t.Context(), "owner", "session", ref.URI, MaxArtifactBytes)
	expected := (len(text) + blob.MaxReadBytes - 1) / blob.MaxReadBytes
	if err != nil || string(data) != text || store.reads != expected {
		t.Fatal("full read not amortized/bounded", store.reads, expected, err)
	}
	store.reads = 0
	if _, err = refs.SearchText(t.Context(), ref.URI, "absent", 1); err != nil || store.reads != expected {
		t.Fatal("search not amortized/bounded", store.reads, err)
	}
	// A correct byte backend cannot conceal a corrupt catalog hash in a later
	// chunk just because the complete remote window was read successfully.
	key, _ := artifactID(ref.URI)
	if _, err = repo.db.ExecContext(t.Context(), "UPDATE artifact_chunks SET sha256=? WHERE artifact_id=? AND chunk_index=3", strings.Repeat("0", 64), key); err != nil {
		t.Fatal(err)
	}
	if _, _, err = repo.ReadArtifactFull(t.Context(), "owner", "session", ref.URI, MaxArtifactBytes); err == nil {
		t.Fatal("window bypassed per-chunk catalog integrity")
	}
}

func TestExternalCorruptionAndDisappearanceFailClosed(t *testing.T) {
	store := blobMemory(t)
	repo, err := OpenWithBlobs(t.Context(), filepath.Join(t.TempDir(), "native"), BlobConfig{ID: "remote", Retention: blob.Volatile, Store: store})
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err = repo.Create(t.Context(), "owner", "session", ""); err != nil {
		t.Fatal(err)
	}
	refs, err := repo.BoundReferences(t.Context(), "owner", "session")
	if err != nil {
		t.Fatal(err)
	}
	ref, err := refs.StoreText(t.Context(), "searchable private data")
	if err != nil {
		t.Fatal(err)
	}
	for _, fault := range []string{"hash", "length", "missing"} {
		t.Run(fault, func(t *testing.T) {
			store.corruptRead = fault == "hash"
			store.shortRead = fault == "length"
			if fault == "missing" {
				store.Store = blobMemory(t).Store
			}
			if _, err := refs.ReadText(t.Context(), ref.URI, 0, 64); err == nil {
				t.Fatal("partial read accepted corrupt bytes")
			}
			if _, err := refs.SearchText(t.Context(), ref.URI, "private", 1); err == nil {
				t.Fatal("search accepted corrupt bytes")
			}
			if _, _, err := repo.ReadArtifactFull(t.Context(), "owner", "session", ref.URI, 1024); err == nil {
				t.Fatal("full read accepted corrupt bytes")
			}
		})
	}
}

func TestBlobBindingRejectsMissingSubstitutionAndPreservesExistingInlineBytes(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "native")
	repo, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = repo.Create(t.Context(), "owner", "session", ""); err != nil {
		t.Fatal(err)
	}
	old, err := repo.PutArtifact(t.Context(), "owner", "session", ArtifactInput{Kind: "original", MediaType: "application/octet-stream", Processing: "original inline bytes", Data: []byte{0, 1, 2, 3}})
	if err != nil {
		t.Fatal(err)
	}
	if err = repo.Close(); err != nil {
		t.Fatal(err)
	}
	store := blobMemory(t)
	config := BlobConfig{ID: "first", Retention: blob.Volatile, Store: store}
	repo, err = OpenWithBlobs(t.Context(), dir, config)
	if err != nil {
		t.Fatal(err)
	}
	_, data, err := repo.ReadArtifactFull(t.Context(), "owner", "session", old.Reference().URI, 100)
	if err != nil || !bytes.Equal(data, []byte{0, 1, 2, 3}) || store.reads != 0 {
		t.Fatal("existing inline artifact was moved or lost", err)
	}
	if err = repo.Close(); err != nil {
		t.Fatal(err)
	}
	if unexpected, err := Open(t.Context(), dir); err == nil {
		unexpected.Close()
		t.Fatal("missing adapter silently falls back")
	}
	wrong := config
	wrong.ID = "second"
	if unexpected, err := OpenWithBlobs(t.Context(), dir, wrong); err == nil {
		unexpected.Close()
		t.Fatal("substituted binding accepted")
	}
	wrong = config
	wrong.Retention = blob.Persistent
	if unexpected, err := OpenWithBlobs(t.Context(), dir, wrong); err == nil {
		unexpected.Close()
		t.Fatal("retention claim changed")
	}
	repo, err = OpenWithBlobs(t.Context(), dir, config)
	if err != nil {
		t.Fatal("failed opens leaked actual owner lock", err)
	}
	if err = repo.Close(); err != nil {
		t.Fatal(err)
	}
	key, _ := blob.NewKey()
	if err = store.Put(t.Context(), key, []byte("host still owns adapter")); err != nil {
		t.Fatal("repository closed shared adapter", err)
	}
}
