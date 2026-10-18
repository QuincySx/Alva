// INPUT: context, encoding/json, errors, filepath/testing, kernel/abi.
// OUTPUT: Cold exact-stream metadata, owner isolation and atomic stale-snapshot regression.
// POS: [platform / durable storage] Optional consumers cannot restore unrelated large history or overwrite a newer durable state snapshot.
package session

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"path/filepath"
	"testing"
)

func TestExactTypedStreamMetadataAndOptimisticAppend(t *testing.T) {
	ctx := context.Background()
	r, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	s, err := r.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	first, err := s.AppendExtensionSnapshot(ctx, "alva.test", "snapshot", 0, json.RawMessage(`{"value":1}`))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.AppendExtensionSnapshot(ctx, "alva.test", "snapshot", 0, json.RawMessage(`{"value":2}`)); !errors.Is(err, ErrRevisionConflict) {
		t.Fatal(err)
	}
	if _, err = s.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.other", Type: "snapshot", SchemaVersion: 1, Payload: json.RawMessage(`{"unrelated":true}`)}}); err != nil {
		t.Fatal(err)
	}
	entry, found, err := r.LatestRecord(ctx, "owner", "native", "alva.test", "snapshot")
	if err != nil || !found || entry.Sequence != first.Sequence {
		t.Fatalf("wrong exact stream %+v %v", entry, err)
	}
	if _, _, err = r.LatestRecord(ctx, "other-owner", "native", "alva.test", "snapshot"); err == nil {
		t.Fatal("foreign latest metadata")
	}
	if _, err = s.AppendExtensionSnapshot(ctx, "alva.test", "snapshot", first.Sequence, json.RawMessage(`{"value":3}`)); err != nil {
		t.Fatal(err)
	}
	records, err := s.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(records) != 3 {
		t.Fatal("failed compare wrote a fact")
	}
}

func TestLatestCorruptForeignArtifactIsErrorNotMissingSnapshot(t *testing.T) {
	ctx := context.Background()
	r, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	s, err := r.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = r.Create(ctx, "owner", "other", ""); err != nil {
		t.Fatal(err)
	}
	info, err := r.PutArtifact(ctx, "owner", "other", ArtifactInput{Kind: "test", Name: "foreign", MediaType: "text/plain", Processing: "fixture", Data: []byte("private foreign body"), Text: true})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.AppendExtensionSnapshot(ctx, "alva.test", "snapshot", 0, json.RawMessage(`{"value":1}`)); err != nil {
		t.Fatal(err)
	}
	if _, err = r.db.ExecContext(ctx, "UPDATE records SET artifact_id=? WHERE session_id=?", info.ID, "native"); err != nil {
		t.Fatal(err)
	}
	if _, found, err := r.LatestRecord(ctx, "owner", "native", "alva.test", "snapshot"); err == nil || found {
		t.Fatal("corrupt foreign metadata silently treated as missing/valid")
	}
}
