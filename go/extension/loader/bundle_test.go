// INPUT: isolated explicit source bundles and owner-only temporary snapshot parents
// OUTPUT: Real snapshot/digest pin, symlink/special budget and no source mutation regressions
// POS: [L4 extension] Plugin staging acceptance without executing installed programs or user bundles
package loader

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func writeBundle(t *testing.T, source, pin string) Manifest {
	t.Helper()
	manifest := `name = "fixture"
version = "1"
runtime = "python"
entry = "main.py"
requested_capabilities = ["host:log"]
`
	if pin != "" {
		manifest += "content_sha256 = \"" + pin + "\"\n"
	}
	if err := os.WriteFile(filepath.Join(source, "alva.toml"), []byte(manifest), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(source, "main.py"), []byte("# local fixture; never run\n"), 0600); err != nil {
		t.Fatal(err)
	}
	value, err := ReadManifest(context.Background(), source)
	if err != nil {
		t.Fatal(err)
	}
	return value
}
func privateParent(t *testing.T) string {
	t.Helper()
	path := t.TempDir()
	if err := os.Chmod(path, 0700); err != nil {
		t.Fatal(err)
	}
	return path
}
func TestSnapshotPinsExactBytesAndDigestWithoutExecutingOrMutatingSource(t *testing.T) {
	source, parent := t.TempDir(), privateParent(t)
	manifest := writeBundle(t, source, "")
	bundle, err := Stage(context.Background(), source, parent, manifest)
	if err != nil {
		t.Fatal(err)
	}
	digest := bundle.Digest()
	if err = os.WriteFile(filepath.Join(source, "main.py"), []byte("changed after snapshot"), 0600); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(bundle.Entry())
	if err != nil || string(data) != "# local fixture; never run\n" {
		t.Fatal("snapshot read mutable discovery bytes")
	}
	if err = bundle.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(bundle.Root()); !os.IsNotExist(err) {
		t.Fatal("generated snapshot not reclaimed")
	}
	if data, err = os.ReadFile(filepath.Join(source, "main.py")); err != nil || string(data) != "changed after snapshot" {
		t.Fatal("cleanup mutated source")
	}
	manifest = writeBundle(t, source, digest)
	bundle, err = Stage(context.Background(), source, parent, manifest)
	if err != nil {
		t.Fatalf("matching semantic digest pin rejected: %v", err)
	}
	if bundle.Digest() != digest {
		t.Fatal("pin changed its own digest")
	}
	bundle.Close()
	if err = os.WriteFile(filepath.Join(source, "main.py"), []byte("changed before snapshot"), 0600); err != nil {
		t.Fatal(err)
	}
	if bundle, err = Stage(context.Background(), source, parent, manifest); err == nil {
		bundle.Close()
		t.Fatal("mismatched pin accepted")
	}
	entries, err := os.ReadDir(parent)
	if err != nil || len(entries) != 0 {
		t.Fatal("failed snapshot leaked owned files")
	}
}
func TestSnapshotRejectsSymlinkSourcesAndOversizedFiles(t *testing.T) {
	for _, mode := range []string{"symlink-file", "symlink-dir", "large-file"} {
		t.Run(mode, func(t *testing.T) {
			source, parent := t.TempDir(), privateParent(t)
			manifest := writeBundle(t, source, "")
			if mode == "large-file" {
				if err := os.WriteFile(filepath.Join(source, "large"), make([]byte, (8<<20)+1), 0600); err != nil {
					t.Fatal(err)
				}
			} else {
				outside := t.TempDir()
				if mode == "symlink-file" {
					if err := os.WriteFile(filepath.Join(outside, "outside"), []byte("not granted"), 0600); err != nil {
						t.Fatal(err)
					}
					outside = filepath.Join(outside, "outside")
				}
				if err := os.Symlink(outside, filepath.Join(source, "alias")); err != nil {
					t.Skip(err)
				}
			}
			if bundle, err := Stage(context.Background(), source, parent, manifest); err == nil {
				bundle.Close()
				t.Fatal("unsafe snapshot accepted")
			}
			entries, err := os.ReadDir(parent)
			if err != nil || len(entries) != 0 {
				t.Fatal("failed snapshot leaked")
			}
		})
	}
}
func TestManifestStrictValidationAndChangedIdentity(t *testing.T) {
	for _, data := range []string{`name="x"` + "\n" + `version="1"` + "\n" + `runtime="python"` + "\n" + `entry="../outside.py"`, strings.Repeat("x", (64<<10)+1)} {
		if _, err := ParseManifest([]byte(data)); err == nil {
			t.Fatal("invalid manifest accepted")
		}
	}
	source, parent := t.TempDir(), privateParent(t)
	manifest := writeBundle(t, source, "")
	manifest.Version = "not-the-approved-version"
	if bundle, err := Stage(context.Background(), source, parent, manifest); err == nil {
		bundle.Close()
		t.Fatal("changed manifest identity accepted")
	}
}

func TestSnapshotCanonicalParentSurvivesAliasReplacementAndRejectsChangedCode(t *testing.T) {
	source, parent, outside := t.TempDir(), privateParent(t), privateParent(t)
	manifest := writeBundle(t, source, "")
	alias := filepath.Join(t.TempDir(), "snapshot-alias")
	if err := os.Symlink(parent, alias); err != nil {
		t.Skip(err)
	}
	// The explicit parent itself must be a real directory, but a caller may
	// reach it through an alias ancestor (as macOS /tmp itself does).
	if err := os.Mkdir(filepath.Join(parent, "private"), 0700); err != nil {
		t.Fatal(err)
	}
	bundle, err := Stage(context.Background(), source, filepath.Join(alias, "private"), manifest)
	if err != nil {
		t.Fatal(err)
	}
	defer bundle.Close()
	if err = os.Remove(alias); err != nil {
		t.Fatal(err)
	}
	if err = os.Symlink(outside, alias); err != nil {
		t.Fatal(err)
	}
	if err = bundle.ValidateForLaunch(context.Background()); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(bundle.Root(), "snapshot-alias") {
		t.Fatal("execution path retained mutable ancestor alias")
	}
	if err = os.WriteFile(bundle.Entry(), []byte("changed staged code"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = bundle.ValidateForLaunch(context.Background()); err == nil {
		t.Fatal("changed snapshot executed under old digest")
	}
}
