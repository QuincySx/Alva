// INPUT: Real private byte snapshot, owned helper process and explicit read-only stdin
// OUTPUT: Seek/read-only transport, no pathname reopen, original bytes and actual last-handle cleanup regressions
// POS: [platform / OS primitive] Decoder input capabilities are verified independently of FFmpeg installation and paid services.
package media

import (
	"bytes"
	"context"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
)

func TestMediaSnapshotHelper(t *testing.T) {
	if os.Getenv("ALVA_MEDIA_SNAPSHOT_HELPER") != "1" {
		return
	}
	if _, err := os.Stdin.Seek(-4, io.SeekEnd); err != nil {
		os.Exit(81)
	}
	if _, err := os.Stdin.Write([]byte("bad")); err == nil {
		os.Exit(82)
	}
	data, err := io.ReadAll(os.Stdin)
	if err != nil || string(data) != "tail" {
		os.Exit(83)
	}
	_, _ = os.Stdout.Write(data)
	os.Exit(0)
}

func exerciseInputSnapshot(t *testing.T, directory string) {
	t.Helper()
	source := []byte("unchanged media bytes tail")
	read, err := inputSnapshot(directory, "input-", source)
	if err != nil {
		t.Fatal(err)
	}
	defer read.Close()
	if _, err := read.Write([]byte("bad")); err == nil {
		t.Fatal("snapshot retained write authority")
	}
	if name, err := os.Open(read.Name()); err == nil {
		name.Close()
		t.Fatal("snapshot input could be reopened by path")
	}
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	child, err := process.Start(process.Config{Command: binary, Args: []string{"-test.run=^TestMediaSnapshotHelper$"}, Cwd: directory, Env: map[string]string{"ALVA_MEDIA_SNAPSHOT_HELPER": "1"}, StdinFile: read})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	defer child.Close(context.Background())
	data, err := io.ReadAll(child.Stdout)
	if err != nil {
		t.Fatal(err)
	}
	if err := child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	if code, exited := child.ExitCode(); !exited || code != 0 || string(data) != "tail" || !child.CleanupComplete() {
		t.Fatal("input handle did not survive actual child seek/read-only/reap", code, exited, string(data))
	}
	if _, err := read.Stat(); err != nil {
		t.Fatal("child closed caller snapshot", err)
	}
	if err := read.Close(); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(directory)
	if err != nil || len(entries) != 0 {
		t.Fatal("snapshot survived last-handle cleanup", len(entries), err)
	}
	if !bytes.Equal(source, []byte("unchanged media bytes tail")) {
		t.Fatal("snapshot changed original input")
	}
	if file, err := inputSnapshot(directory, "too-large-", make([]byte, MaxInput+1)); err == nil {
		file.Close()
		t.Fatal("oversized input snapshot created")
	}
	entries, err = os.ReadDir(directory)
	if err != nil || len(entries) != 0 {
		t.Fatal("failed input left temporary files", err)
	}
}

func TestNativeInputSnapshotReadOnlyAndLifetime(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows counterpart supplies a real protected DACL root")
	}
	directory := filepath.Join(t.TempDir(), "private")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	exerciseInputSnapshot(t, directory)
}
