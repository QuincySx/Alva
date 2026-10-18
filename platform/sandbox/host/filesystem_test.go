// INPUT: Actual confined Root and WASI access/open descriptors
// OUTPUT: Read-only create cannot acquire write/truncate rights, append cannot silently ignore positioned-write offsets
// POS: [platform / WASM host] Directory mutation intent is distinct from the narrower returned file descriptor access capability.
package host

import (
	"os"
	"path/filepath"
	"testing"

	expsys "github.com/tetratelabs/wazero/experimental/sys"
)

func TestWASIReadonlyCreateDoesNotGrantWritableDescriptor(t *testing.T) {
	directory := t.TempDir()
	root, err := os.OpenRoot(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	budget := &fsBudget{}
	filesystem := &rootedFS{root: root, budget: budget}
	file, errno := filesystem.OpenFile("read-created", expsys.O_RDONLY|expsys.O_CREAT, 0600)
	if errno != 0 {
		t.Fatal("valid read-only create failed", errno)
	}
	if _, errno := file.Write([]byte("not granted")); errno != expsys.EBADF {
		t.Fatal("creation granted data write authority", errno)
	}
	if errno := file.Truncate(10); errno != expsys.EBADF {
		t.Fatal("creation granted later truncate authority", errno)
	}
	if budget.written.Load() != 0 {
		t.Fatal("rejected write consumed successful-write budget")
	}
	if errno := file.Close(); errno != 0 {
		t.Fatal(errno)
	}
	if info, err := os.Stat(filepath.Join(directory, "read-created")); err != nil || info.Size() != 0 {
		t.Fatal("read-only create changed data", err)
	}
	file, errno = filesystem.OpenFile("append", expsys.O_WRONLY|expsys.O_CREAT|expsys.O_APPEND, 0600)
	if errno != 0 {
		t.Fatal(errno)
	}
	if _, errno := file.Write([]byte("original")); errno != 0 {
		t.Fatal(errno)
	}
	if _, errno := file.Pwrite([]byte("wrong offset"), 0); errno != expsys.EINVAL {
		t.Fatal("append descriptor silently accepted a positioned write", errno)
	}
	if errno := file.Close(); errno != 0 {
		t.Fatal(errno)
	}
	if data, err := os.ReadFile(filepath.Join(directory, "append")); err != nil || string(data) != "original" {
		t.Fatal("positioned write changed append file", string(data), err)
	}
	if budget.open.Load() != 0 {
		t.Fatal("descriptor budget leaked")
	}
}
