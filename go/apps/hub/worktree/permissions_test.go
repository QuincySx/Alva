// INPUT: Explicit temporary Git repositories, actual regular permissions and text/binary proposals
// OUTPUT: Existing 0640/0750 preservation, owner-only new/exec transitions and permission/content conflict no-write regressions
// POS: [L7 Hub product] No complete ACL/xattr preservation, privileged execution or OS atomic hash-CAS is claimed
package worktree

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

func writePermissionFixture(t *testing.T, path string, data []byte, mode os.FileMode) {
	t.Helper()
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	// Fixtures must not depend on the caller's umask, just as adoption must not
	// let a restrictive umask silently strip an existing Lead's group read bit.
	if err := os.Chmod(path, mode); err != nil {
		t.Fatal(err)
	}
}
func assertPermissionFixture(t *testing.T, path string, want []byte, mode os.FileMode) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(data, want) {
		t.Fatalf("%s bytes mismatch: %v", filepath.Base(path), err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != mode || info.Mode()&(os.ModeSetuid|os.ModeSetgid|os.ModeSticky) != 0 {
		t.Fatalf("%s mode=%v want=%#o err=%v", filepath.Base(path), info.Mode(), mode, err)
	}
}

func TestActualAdoptionPreservesLeadReadWriteAndUnchangedExecutePermissions(t *testing.T) {
	m, repo := managerFixture(t)
	type fixture struct {
		name                       string
		before, after              []byte
		original, worker, expected os.FileMode
	}
	files := []fixture{
		{"text0640.txt", []byte("old text\n"), []byte("new text\n"), 0640, 0600, 0640},
		{"text0750.txt", []byte("old executable text\n"), []byte("new executable text\n"), 0750, 0700, 0750},
		{"binary0640.bin", []byte{0, 1, 2, 255}, []byte{0, 3, 4, 254}, 0640, 0600, 0640},
		{"binary0750.bin", []byte{0, 5, 6, 253}, []byte{0, 7, 8, 252}, 0750, 0700, 0750},
		{"enable-execute.txt", []byte("same bytes on purpose\n"), []byte("same bytes on purpose\n"), 0640, 0700, 0740},
		{"disable-execute.bin", []byte{0, 9, 10, 251}, []byte{0, 9, 10, 251}, 0750, 0600, 0640},
	}
	args := []string{"add", "--"}
	for _, file := range files {
		writePermissionFixture(t, filepath.Join(repo, file.name), file.before, file.original)
		args = append(args, file.name)
	}
	gitRun(t, m, repo, "config", "core.filemode", "true")
	gitRun(t, m, repo, args...)
	gitRun(t, m, repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "actual permission base")
	h, err := m.Create(t.Context(), repo, "worker-permission-preservation")
	if err != nil {
		t.Fatal(err)
	}
	for _, file := range files {
		writePermissionFixture(t, filepath.Join(h.Path, file.name), file.after, file.worker)
	}
	newText, newExecutable := []byte("new owner-only file\n"), []byte{0, 11, 12, 250}
	writePermissionFixture(t, filepath.Join(h.Path, "new-text.txt"), newText, 0664)
	writePermissionFixture(t, filepath.Join(h.Path, "new-executable.bin"), newExecutable, 0755)
	e, err := m.Preview(t.Context(), h)
	if err != nil {
		t.Fatal(err)
	}
	if len(e.Changes) != len(files)+2 {
		t.Fatal("text/binary/mode-only changes missing", len(e.Changes))
	}
	if err = m.Apply(t.Context(), h, e); err != nil {
		t.Fatal(err)
	}
	for _, file := range files {
		assertPermissionFixture(t, filepath.Join(repo, file.name), file.after, file.expected)
	}
	assertPermissionFixture(t, filepath.Join(repo, "new-text.txt"), newText, 0600)
	assertPermissionFixture(t, filepath.Join(repo, "new-executable.bin"), newExecutable, 0700)
}

func TestPermissionOrContentConflictsNeverRewriteLeadBytesOrMode(t *testing.T) {
	for _, conflict := range []string{"lead-rw", "lead-execute", "lead-bytes", "worker-bytes", "worker-execute"} {
		t.Run(conflict, func(t *testing.T) {
			m, repo := managerFixture(t)
			leadPath := filepath.Join(repo, "tracked.txt")
			writePermissionFixture(t, leadPath, []byte("base original\n"), 0640)
			h, err := m.Create(t.Context(), repo, "worker-permission-conflict")
			if err != nil {
				t.Fatal(err)
			}
			workerPath := filepath.Join(h.Path, "tracked.txt")
			writePermissionFixture(t, workerPath, []byte("proposed edit\n"), 0600)
			e, err := m.Preview(t.Context(), h)
			if err != nil {
				t.Fatal(err)
			}
			want, wantMode := []byte("base original\n"), os.FileMode(0640)
			switch conflict {
			case "lead-rw":
				wantMode = 0660
				writePermissionFixture(t, leadPath, want, wantMode)
			case "lead-execute":
				wantMode = 0750
				writePermissionFixture(t, leadPath, want, wantMode)
			case "lead-bytes":
				want = []byte("human changed file\n")
				writePermissionFixture(t, leadPath, want, wantMode)
			case "worker-bytes":
				writePermissionFixture(t, workerPath, []byte("different unreviewed edit\n"), 0600)
			case "worker-execute":
				writePermissionFixture(t, workerPath, []byte("proposed edit\n"), 0700)
			}
			if err = m.Apply(t.Context(), h, e); err == nil {
				t.Fatal("stale permissions/content evidence was applied")
			}
			assertPermissionFixture(t, leadPath, want, wantMode)
		})
	}
}

func TestReadPinnedRefusesSpecialPrivilegeModeBits(t *testing.T) {
	for _, bit := range []os.FileMode{os.ModeSetuid, os.ModeSetgid, os.ModeSticky} {
		t.Run(bit.String(), func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "regular")
			writePermissionFixture(t, path, []byte("never executed"), 0600|bit)
			info, err := os.Stat(path)
			if err != nil || info.Mode()&bit == 0 {
				t.Fatalf("fixture did not set special mode bit %v: %v", bit, err)
			}
			root, err := os.OpenRoot(dir)
			if err != nil {
				t.Fatal(err)
			}
			defer root.Close()
			if _, _, err = readPinned(root, "regular"); err == nil {
				t.Fatal("special privilege mode was accepted")
			}
		})
	}
}
