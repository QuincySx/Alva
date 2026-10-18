// INPUT: Real original upload, actual image processor and private native derivative evidence fixtures
// OUTPUT: Cold HTTP media indices/manifests/ranges, cross-owner denial and receipt-only read regressions
// POS: [L7 Hub product] Preview acceptance verifies immutable bytes without running an Agent, decoder or media replay
package server

import (
	"bytes"
	"encoding/json"
	"image"
	"image/png"
	"net/url"
	"strings"
	"testing"
	"time"

	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	mediaextension "github.com/QuincySx/Alva/extension/media"
	"github.com/QuincySx/Alva/platform/storage/session"
	nativemedia "github.com/QuincySx/Alva/platform/system/media"
)

func TestHTTPMediaColdEvidenceAndVerifiedBinaryRange(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	owned, foreign := create(t, f, "media"), create(t, f, "other")
	prefix := "/v1/sessions/" + owned.ID
	var original bytes.Buffer
	if err := png.Encode(&original, image.NewGray(image.Rect(0, 0, 64, 32))); err != nil {
		t.Fatal(err)
	}
	var upload product.DraftAttachment
	if json.Unmarshal(body(t, f.request(t, "POST", prefix+"/attachments", original.Bytes(), map[string]string{"Content-Type": "application/octet-stream", "X-File-Name": "source.png"}), 201), &upload) != nil {
		t.Fatal("invalid original upload")
	}
	query := "?branch=" + upload.BranchID
	indexPath := prefix + "/media/artifacts" + query + "&source_uri=" + url.QueryEscape(upload.Reference.URI)
	var page session.ArtifactIndexPage
	if json.Unmarshal(body(t, f.request(t, "GET", indexPath, nil, nil), 200), &page) != nil || len(page.Entries) != 0 {
		t.Fatal("empty original pretended processed")
	}
	// Test fixture records a genuine completed processor result; HTTP itself
	// offers no processing mutation and must never invoke the processor.
	output, err := nativemedia.ProcessImage(t.Context(), original.Bytes(), "image/png")
	if err != nil {
		t.Fatal(err)
	}
	derived, err := f.app.Repository.PutArtifact(t.Context(), owned.ID, upload.Info.SessionID, session.ArtifactInput{Kind: "media-derivative", Name: upload.Info.ID, MediaType: output.MediaType, Processing: output.Operation.Description, Data: output.Data})
	if err != nil {
		t.Fatal(err)
	}
	manifest := mediaextension.Manifest{Version: 1, Source: upload.Reference, Artifact: derived.Reference(), Output: output, Processing: "Actual test-only derivative evidence; originals preserved, no model delivery."}
	data, _ := json.Marshal(manifest)
	entry, err := f.app.Repository.PutArtifact(t.Context(), owned.ID, upload.Info.SessionID, session.ArtifactInput{Kind: "media-manifest", Name: upload.Info.ID, MediaType: "application/json", Processing: "Explicit complete fixture provenance", Data: data, Text: true})
	if err != nil {
		t.Fatal(err)
	}
	if json.Unmarshal(body(t, f.request(t, "GET", indexPath, nil, nil), 200), &page) != nil || len(page.Entries) != 1 {
		t.Fatal("missing manifest metadata")
	}
	manifestPath := prefix + "/media/manifest" + query + "&manifest_uri=" + url.QueryEscape(entry.Reference().URI)
	metadata := body(t, f.request(t, "GET", manifestPath, nil, nil), 200)
	if bytes.Contains(metadata, []byte(`"data"`)) || !bytes.Contains(metadata, []byte(upload.Info.SHA256)) {
		t.Fatal("metadata leaked BLOB or lost source provenance")
	}
	previewPath := strings.Replace(manifestPath, "/manifest?", "/preview?", 1)
	response := f.request(t, "GET", previewPath, nil, map[string]string{"Range": "bytes=0-7"})
	if response.Header.Get("Content-Type") != "image/jpeg" || response.Header.Get("X-Content-SHA256") != derived.SHA256 || response.Header.Get("Content-Range") == "" {
		t.Fatal("binary preview metadata/range wrong")
	}
	if got := body(t, response, 206); !bytes.Equal(got, output.Data[:8]) {
		t.Fatal("range not actual immutable derivative bytes")
	}
	body(t, f.request(t, "GET", strings.Replace(previewPath, owned.ID, foreign.ID, 1), nil, nil), 409)
	body(t, f.request(t, "POST", previewPath, []byte(`{}`), nil), 405)
	body(t, f.request(t, "GET", prefix+"/media/preview"+query+"&manifest_uri="+url.QueryEscape(upload.Reference.URI), nil, nil), 409)
	selection := mediaextension.Selection{ID: strings.Repeat("1", 32), RunID: "fixture-run", CallID: "fixture-call", Route: "explicit fixture", Reason: "fixture only", Manifests: []mediaextension.Manifest{manifest}, Processing: "Prepared fixture, not submitted"}
	data, _ = json.Marshal(selection)
	if _, err = f.app.Repository.PutArtifact(t.Context(), owned.ID, upload.Info.SessionID, session.ArtifactInput{Kind: "media-selection", Name: selection.ID, MediaType: "application/json", Processing: selection.Processing, Data: data, Text: true}); err != nil {
		t.Fatal(err)
	}
	data, _ = json.Marshal(mediaextension.Receipt{Version: 1, SelectionID: selection.ID, RunID: selection.RunID, State: "prepared", Time: time.Now().UTC(), Processing: "Not sent"})
	if _, err = f.app.Repository.PutArtifact(t.Context(), owned.ID, upload.Info.SessionID, session.ArtifactInput{Kind: "media-receipt", Name: selection.ID, MediaType: "application/json", Processing: "Actual fixture preparation only; not delivered", Data: data, Text: true}); err != nil {
		t.Fatal(err)
	}
	if json.Unmarshal(body(t, f.request(t, "GET", prefix+"/media/receipts"+query+"&selection_id="+selection.ID, nil, nil), 200), &page) != nil || len(page.Entries) != 1 {
		t.Fatal("receipt metadata missing")
	}
	if f.app.Runtime.HasActive() {
		t.Fatal("cold media read restored model/decoder/selection")
	}
}
