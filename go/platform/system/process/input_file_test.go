// INPUT: Explicit local test executable and borrowed regular input file
// OUTPUT: Real seek/read-only child stdin, borrowed lifetime and nonregular-file rejection regressions
// POS: [platform / OS primitive] No input copy goroutine, ambient file grant or dependency on decoder installation.
package process

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestSeekableInputHelper(t *testing.T) {
	if os.Getenv("ALVA_SEEKABLE_INPUT_HELPER") != "1" {
		return
	}
	if _, err := os.Stdin.Seek(-4, io.SeekEnd); err != nil {
		os.Exit(91)
	}
	if _, err := os.Stdin.Write([]byte("BAD")); err == nil {
		os.Exit(92)
	}
	data, err := io.ReadAll(os.Stdin)
	if err != nil || string(data) != "tail" {
		os.Exit(93)
	}
	if _, err := os.Stdout.Write(data); err != nil {
		os.Exit(94)
	}
	os.Exit(0)
}

func TestExplicitSeekableReadOnlyInputPreservesBorrowedFile(t *testing.T) {
	workspace := t.TempDir()
	name := filepath.Join(workspace, "input")
	if err := os.WriteFile(name, []byte("original bytes tail"), 0600); err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(name)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	child, err := Start(Config{Command: binary, Args: []string{"-test.run=^TestSeekableInputHelper$"}, Cwd: workspace, Env: map[string]string{"ALVA_SEEKABLE_INPUT_HELPER": "1"}, StdinFile: file})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	defer child.Close(context.Background())
	if _, err := child.Stdin.Write([]byte("must not replace the file")); err == nil {
		t.Fatal("parent exposed writable input")
	}
	if err := child.Stdin.Close(); err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(child.Stdout)
	if err != nil {
		t.Fatal(err)
	}
	if err = child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	if code, exited := child.ExitCode(); !exited || code != 0 || string(data) != "tail" || !child.CleanupComplete() {
		t.Fatal("child lost seek/read-only input or failed physical exit", code, exited, string(data))
	}
	if _, err := file.Stat(); err != nil {
		t.Fatal("process closed borrowed input", err)
	}
	if original, err := os.ReadFile(name); err != nil || string(original) != "original bytes tail" {
		t.Fatal("child modified input", err)
	}
	read, write, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer read.Close()
	defer write.Close()
	if p, err := Start(Config{Command: binary, Cwd: workspace, StdinFile: read}); err == nil {
		_ = p.Close(ctx)
		t.Fatal("nonregular input granted")
	}
}
