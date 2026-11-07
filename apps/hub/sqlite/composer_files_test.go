// INPUT: Real temporary SQLite catalogs and maximum-size bounded composer-file metadata fixtures.
// OUTPUT: Immutable idempotency, explicit scope isolation, restart preservation and full-size metadata budget regressions.
// POS: [L7 SQLite adapter acceptance] Tests metadata without user files, documents, engines or external accounts.
package sqlite

import (
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestComposerFileCatalogFullSizeMetadataImmutableAndScoped(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "product")
	store, err := Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { store.Close() }()
	f := hub.ComposerFile{ID: "file-fixture", Scope: hub.ComposerDraftScope{Kind: "home", ID: "project"}, Name: "maximum.bin", SourcePath: filepath.Join(t.TempDir(), "maximum.bin"), MediaType: "application/octet-stream", SHA256: strings.Repeat("a", 64), Bytes: hub.MaxComposerFileBytes, OriginalKey: "original", Processing: "fixture metadata only; no body bytes stored in SQLite", CreatedAt: 1}
	for index := int64(0); index < f.Bytes; index += hub.ComposerFileChunkBytes {
		f.ChunkSHA256 = append(f.ChunkSHA256, strings.Repeat("b", 64))
	}
	receipt, err := store.SaveComposerFile(t.Context(), f)
	if err != nil || !reflect.DeepEqual(receipt, f) {
		t.Fatal("full-sized file metadata exceeded its bounded row budget", err)
	}
	if _, err = store.SaveComposerFile(t.Context(), f); err != nil {
		t.Fatal("identical immutable receipt was not idempotent", err)
	}
	modified := f
	modified.Name = "different"
	if _, err = store.SaveComposerFile(t.Context(), modified); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("immutable metadata was overwritten", err)
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	store, err = Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	saved, err := store.ComposerFile(t.Context(), f.ID)
	if err != nil || !reflect.DeepEqual(saved, f) {
		t.Fatal("restart changed original metadata", err)
	}
	files, err := store.ListComposerFiles(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "other"}, 10)
	if err != nil || len(files) != 0 {
		t.Fatal("recent files crossed ownership scope", err)
	}
	if _, err = store.ComposerFile(t.Context(), "missing"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("catalog leaked SQL missing semantics", err)
	}
	if _, err = store.db.ExecContext(t.Context(), `UPDATE hub_composer_files SET scope_id='corrupt' WHERE id=?`, f.ID); err != nil {
		t.Fatal(err)
	}
	if _, err = store.ComposerFile(t.Context(), f.ID); err == nil {
		t.Fatal("inconsistent indexed metadata was trusted")
	}
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	if reopened, err := Open(t.Context(), directory); err == nil {
		reopened.Close()
		t.Fatal("corrupt file catalog was accepted before recovery")
	}
}
