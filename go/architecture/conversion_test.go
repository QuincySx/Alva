// INPUT: Pinned absorbed Magpie provenance, local source/license bytes and the Go module manifest.
// OUTPUT: Offline source attribution/integrity and removed-dependency regression gates.
// POS: CI verifies that the protocol source fork remains reviewable without downloading or running its application.
package architecture_test

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestMagpieSourceProvenanceAndRemovedDependency(t *testing.T) {
	root := goRoot()
	dir := filepath.Join(root, "integration", "llm", "codec", "internal", "magpie")
	raw, err := os.ReadFile(filepath.Join(dir, "provenance.json"))
	if err != nil {
		t.Fatal(err)
	}
	var provenance struct {
		Repository, Commit, License string
		LicenseSHA                  string                          `json:"license_sha256"`
		LocalFiles                  []struct{ Path, SHA256 string } `json:"local_files"`
		Files                       []struct {
			Path, SHA256 string
			Declarations []string
		}
	}
	if err = json.Unmarshal(raw, &provenance); err != nil {
		t.Fatal(err)
	}
	if provenance.Repository != "https://github.com/yetone/magpie" || provenance.Commit != "e085d6e9262786815c0f6c5ff65c5a585916cc9e" || provenance.License != "MIT" {
		t.Fatal("conversion source pin/license changed without review")
	}
	seen := map[string]bool{}
	for _, file := range provenance.LocalFiles {
		if filepath.Base(file.Path) != file.Path || seen[file.Path] || !strings.HasSuffix(file.Path, ".go") {
			t.Fatal("invalid source manifest member", file.Path)
		}
		seen[file.Path] = true
		data, err := os.ReadFile(filepath.Join(dir, file.Path))
		if err != nil {
			t.Fatal(err)
		}
		sum := sha256.Sum256(data)
		if hex.EncodeToString(sum[:]) != file.SHA256 {
			t.Error("absorbed source changed without provenance update", file.Path)
		}
	}
	sourceFiles, err := filepath.Glob(filepath.Join(dir, "*.go"))
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range sourceFiles {
		if !seen[filepath.Base(path)] {
			t.Error("source lacks provenance", path)
		}
	}
	if len(provenance.Files) == 0 || len(sourceFiles) == 0 {
		t.Fatal("empty conversion-source extraction")
	}
	for _, file := range provenance.Files {
		if len(file.Declarations) == 0 || len(file.SHA256) != 64 {
			t.Error("missing upstream path/hash/declaration evidence", file.Path)
		}
	}
	license, err := os.ReadFile(filepath.Join(dir, "LICENSE.magpie"))
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(license)
	if hex.EncodeToString(sum[:]) != provenance.LicenseSHA || !strings.Contains(string(license), "Copyright (c) 2026 yetone") || !strings.Contains(string(license), "MIT License") {
		t.Fatal("upstream license notice missing or changed")
	}
	for _, name := range []string{"go.mod", "go.sum"} {
		data, err := os.ReadFile(filepath.Join(root, name))
		if err != nil {
			t.Fatal(err)
		}
		if strings.Contains(string(data), "github.com/QuantumNous/new-api") {
			t.Error("removed protocol dependency returned", name)
		}
	}
}
