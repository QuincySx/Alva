// INPUT: Actual native Hub Turn, authenticated HTTP bundle downloads and foreign branch requests
// OUTPUT: Exact fixed snapshot bytes/hash, no filesystem writes/replay and transport scope rejection
// POS: [L7 Hub product] Export uses the same native fact encoder as the actual desktop action.
package server

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"

	bootstrap "github.com/QuincySx/Alva/go/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/go/apps/hub/product"
)

func TestHTTPInspectorBundleIsExactColdScopedDownloadWithoutHostWrites(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	root, foreign := create(t, f, "bundle"), create(t, f, "foreign")
	prefix := "/v1/sessions/" + root.ID
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"only the original demo turn"}`), nil), 202)
	waitIdle(t, f, root.ID)
	detail, err := f.app.Store.Detail(t.Context(), root.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	expected, summary, err := f.app.Service.InspectorBundleAt(t.Context(), root.ID, branch.ID, nil, 0, false)
	if err != nil {
		t.Fatal(err)
	}
	response := f.request(t, "GET", prefix+"/inspector-bundle?branch="+branch.ID, nil, nil)
	hashHeader := response.Header.Get("X-Content-SHA256")
	if response.Header.Get("Content-Type") != "application/x-ndjson" {
		t.Fatal("wrong bundle media type")
	}
	actual := body(t, response, 200)
	hash := sha256.Sum256(actual)
	if !bytes.Equal(expected, actual) || summary.SHA256 != hashHeader || hex.EncodeToString(hash[:]) != hashHeader || !bytes.Contains(actual, []byte(`"type":"complete"`)) {
		t.Fatal("HTTP export bytes/hash or footer diverged")
	}
	if _, err := os.Stat(filepath.Join(f.app.Service.DataDirectory, "exports")); !os.IsNotExist(err) {
		t.Fatal("read-only HTTP download wrote host files", err)
	}
	body(t, f.request(t, "GET", "/v1/sessions/"+foreign.ID+"/inspector-bundle?branch="+branch.ID, nil, nil), 409)
	for _, query := range []string{"&include_children=1", "&include_children=", "&child_path=0", "&native_id=chosen-by-client", "&output=/tmp/host-file"} {
		body(t, f.request(t, "GET", prefix+"/inspector-bundle?branch="+branch.ID+query, nil, nil), 400)
	}
	if f.app.Runtime.HasActive() {
		t.Fatal("cold export started runtime")
	}
	page, err := f.app.Store.History(t.Context(), root.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	inputs := 0
	for _, entry := range page {
		if entry.Input != "" {
			inputs++
		}
	}
	if inputs != 1 {
		t.Fatal("export replayed input", inputs)
	}
}
