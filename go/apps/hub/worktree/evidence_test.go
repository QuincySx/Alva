// INPUT: Explicit temporary repositories, literal binary/text/mode edits and hostile clean/smudge commands
// OUTPUT: Actual immutable diff/check/apply, stale/dirty/tamper refusal and no commit/filter execution
// POS: [L7 Hub product] Fixtures never touch a real user worktree, ambient Git credentials or global configuration
package worktree

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func TestActualDiffAndExplicitApplyPreserveRawBytesWithoutHooksFiltersOrCommit(t *testing.T) {
	m, repo := managerFixture(t)
	for name, data := range map[string][]byte{".gitattributes": []byte("*.txt filter=trap\n"), "delete.txt": []byte("remove only after consent\n"), "mode.txt": []byte("executable\n")} {
		if err := os.WriteFile(filepath.Join(repo, name), data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	gitRun(t, m, repo, "add", "--", ".gitattributes", "delete.txt", "mode.txt")
	gitRun(t, m, repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "base for literal patch")
	h, err := m.Create(t.Context(), repo, "worker-difference")
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "must-not-execute")
	for _, kind := range []string{"clean", "smudge"} {
		gitRun(t, m, repo, "config", "filter.trap."+kind, "/bin/sh -c 'printf BAD > "+marker+"; cat'")
	}
	before := gitRun(t, m, repo, "rev-parse", "HEAD")
	index := gitRun(t, m, repo, "ls-files", "--stage", "-z")
	if err = os.WriteFile(filepath.Join(h.Path, "tracked.txt"), []byte("exact Worker answer\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(filepath.Join(h.Path, "delete.txt")); err != nil {
		t.Fatal(err)
	}
	if err = os.Chmod(filepath.Join(h.Path, "mode.txt"), 0700); err != nil {
		t.Fatal(err)
	}
	binary := []byte{0, 1, 2, 0, 255, 17, 18}
	if err = os.WriteFile(filepath.Join(h.Path, "new.bin"), binary, 0600); err != nil {
		t.Fatal(err)
	}
	e, err := m.Preview(t.Context(), h)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("preview executed repository filter before apply", err)
	}
	if len(e.Changes) != 4 || e.PatchSHA256 == "" || e.WorkerSHA256 == e.SharedSHA256 {
		t.Fatal("actual raw evidence missing", e)
	}
	part, more, err := m.ReadPatch(t.Context(), e, 0, 32)
	if err != nil || len(part) != 32 || !more {
		t.Fatal("bounded raw patch page", err)
	}
	unchanged, _ := os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if string(unchanged) != "base original\n" {
		t.Fatal("preview adopted automatically")
	}
	if err = m.Apply(t.Context(), h, e); err != nil {
		t.Fatal("explicit actual check/apply", err)
	}
	for _, change := range e.Changes {
		data, err := os.ReadFile(filepath.Join(repo, change.Path))
		if change.AfterSHA256 == "" {
			if !os.IsNotExist(err) {
				t.Fatal("deletion not applied", err)
			}
			continue
		}
		if err != nil || sha(data) != change.AfterSHA256 {
			t.Fatal("literal bytes not adopted", change.Path, err)
		}
	}
	if data, err := os.ReadFile(filepath.Join(repo, "new.bin")); err != nil || !bytes.Equal(data, binary) {
		t.Fatal("binary patch was transformed", err)
	}
	if after := gitRun(t, m, repo, "rev-parse", "HEAD"); !bytes.Equal(before, after) {
		t.Fatal("adoption committed automatically")
	}
	if after := gitRun(t, m, repo, "ls-files", "--stage", "-z"); !bytes.Equal(index, after) {
		t.Fatal("adoption staged changes")
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("preview/apply executed repository filter", err)
	}
	if err = m.Apply(t.Context(), h, e); err == nil {
		t.Fatal("already-adopted patch replayed")
	}
}

func TestDiffRejectsDirtySharedChangedWorkerSymlinkAndTamperedPatch(t *testing.T) {
	m, repo := managerFixture(t)
	h, err := m.Create(t.Context(), repo, "worker-stale")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(h.Path, "tracked.txt")
	if err = os.WriteFile(path, []byte("first\n"), 0600); err != nil {
		t.Fatal(err)
	}
	e, err := m.Preview(t.Context(), h)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, []byte("changed since preview\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = m.Apply(t.Context(), h, e); err == nil {
		t.Fatal("stale Worker snapshot applied")
	}
	if err = os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("human edit\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = m.Preview(t.Context(), h); err == nil {
		t.Fatal("dirty shared source allowed")
	}
	if err = os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("base original\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = os.Symlink("tracked.txt", filepath.Join(h.Path, "link")); err != nil {
		t.Fatal(err)
	}
	if _, err = m.Preview(t.Context(), h); err == nil {
		t.Fatal("current Worker symlink accepted")
	}
	if err = os.Remove(filepath.Join(h.Path, "link")); err != nil {
		t.Fatal(err)
	}
	e, err = m.Preview(t.Context(), h)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(m.config.Root, "evidence", e.ID, "diff.patch"), []byte("tampered"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = m.Apply(t.Context(), h, e); err == nil {
		t.Fatal("tampered durable patch applied")
	}
	data, _ := os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if string(data) != "base original\n" {
		t.Fatal("refused adoption changed source")
	}
}
