// INPUT: Real private SQLite artifact store with interleaved immutable subject records
// OUTPUT: Exact subject, stable membership, full hash/budget and cross-owner read regressions
// POS: [platform / durable storage] Cold metadata indices must not inflate into conversation restore or unauthorized BLOB reads
package session

import (
	"bytes"
	"context"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
)

func TestArtifactIndexStableMembershipAndVerifiedFullRead(t *testing.T) {
	ctx := context.Background()
	r, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	if _, err = r.Create(ctx, "owner", "actual", ""); err != nil {
		t.Fatal(err)
	}
	put := func(name, text string) storage.ArtifactInfo {
		t.Helper()
		v, e := r.PutArtifact(ctx, "owner", "actual", storage.ArtifactInput{Kind: "media-test", Name: name, MediaType: "text/plain", Processing: "explicit fixture, no delivery", Data: []byte(text), Text: true})
		if e != nil {
			t.Fatal(e)
		}
		return v
	}
	first := put("selected", "first")
	put("other", "not selected")
	second := put("selected", "second")
	page, err := r.PageArtifacts(ctx, "owner", "actual", "media-test", "selected", 0, 0, 1)
	if err != nil || len(page.Entries) != 1 || page.Entries[0].Info.ID != first.ID || !page.HasMore {
		t.Fatal("incorrect metadata page", page, err)
	}
	put("selected", "later excluded")
	next, err := r.PageArtifacts(ctx, "owner", "actual", "media-test", "selected", page.Next, page.HighWater, 1)
	if err != nil || len(next.Entries) != 1 || next.Entries[0].Info.ID != second.ID || next.HasMore {
		t.Fatal("unstable fixed membership", next, err)
	}
	if _, err = r.PageArtifacts(ctx, "foreign", "actual", "media-test", "", 0, 0, 4); err == nil {
		t.Fatal("foreign metadata read")
	}
	if _, _, err = r.ReadArtifactFull(ctx, "foreign", "actual", first.Reference().URI, 100); err == nil {
		t.Fatal("foreign full read")
	}
	if _, _, err = r.ReadArtifactFull(ctx, "owner", "actual", first.Reference().URI, 4); err == nil {
		t.Fatal("full read exceeded budget")
	}
	info, data, err := r.ReadArtifactFull(ctx, "owner", "actual", first.Reference().URI, 100)
	if err != nil || info.SHA256 != first.SHA256 || !bytes.Equal(data, []byte("first")) {
		t.Fatal("full read mismatch", err)
	}
	if _, err = r.db.ExecContext(ctx, "UPDATE artifact_chunks SET data=? WHERE artifact_id=?", []byte("bad"), first.ID); err != nil {
		t.Fatal(err)
	}
	if _, _, err = r.ReadArtifactFull(ctx, "owner", "actual", first.Reference().URI, 100); err == nil {
		t.Fatal("corrupt bytes accepted")
	}
	// Metadata enumeration remains cold even if an unrelated body is corrupt.
	if _, err = r.PageArtifacts(ctx, "owner", "actual", "media-test", "", 0, 0, 4); err != nil {
		t.Fatal(err)
	}
}
