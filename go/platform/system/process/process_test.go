// INPUT: testing, local owned helper child and explicit process configuration
// OUTPUT: Environment isolation, natural exit, cancellation and EOF-not-exit regressions
// POS: [platform / OS primitive] Local-only native process ownership acceptance without running user commands
package process

import (
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
	"time"
)

func TestProcessChild(t *testing.T) {
	mode := ""
	for _, a := range os.Args {
		if strings.HasPrefix(a, "--owned-child=") {
			mode = strings.TrimPrefix(a, "--owned-child=")
		}
	}
	if mode == "" {
		return
	}
	switch mode {
	case "dirfd":
		file := os.NewFile(3, "pinned-directory")
		if err := file.Chdir(); err != nil {
			os.Exit(11)
		}
		data, err := os.ReadFile("only-selected.txt")
		if err != nil {
			os.Exit(12)
		}
		fmt.Print(string(data))
		os.Exit(0)
	case "readfd":
		file := os.NewFile(3, "granted-read-only")
		data, err := io.ReadAll(file)
		if err != nil {
			os.Exit(9)
		}
		if _, err = file.Write([]byte("must-not-write")); err == nil {
			os.Exit(10)
		}
		fmt.Print(string(data))
		os.Exit(0)
	case "env":
		fmt.Print(os.Getenv("ALVA_TEST_AMBIENT_SECRET"))
		fmt.Print(os.Getenv("EXPLICIT"))
		os.Exit(7)
	case "eof":
		os.Stdout.Close()
		for {
			time.Sleep(time.Hour)
		}
	}
	os.Exit(0)
}

func TestExplicitInheritedDescriptorMustBeReadOnly(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix fd3 ABI is explicitly unsupported; Windows verifies exact stdio HANDLE_LIST instead")
	}
	name := filepath.Join(t.TempDir(), "exact-file")
	if err := os.WriteFile(name, []byte("only explicitly granted bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	read, err := os.Open(name)
	if err != nil {
		t.Fatal(err)
	}
	defer read.Close()
	config := fixtureConfig(t, "readfd")
	config.ReadFiles = []*os.File{read}
	child, err := Start(config)
	if err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(child.Stdout)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	if err = child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	code, ok := child.ExitCode()
	if !ok || code != 0 || string(data) != "only explicitly granted bytes" || !child.CleanupComplete() {
		t.Fatal("read descriptor not exact/read-only", string(data), code, ok)
	}
	write, err := os.OpenFile(name, os.O_RDWR, 0600)
	if err != nil {
		t.Fatal(err)
	}
	defer write.Close()
	config.ReadFiles = []*os.File{write}
	if child, err = Start(config); err == nil {
		child.Close(context.Background())
		t.Fatal("writable descriptor granted as read-only")
	}
}

func TestPinnedDirectoryDescriptorUsesActualHandleNotRetargetedPath(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix pinned fd3 cwd ABI is explicitly unsupported on Windows")
	}
	root := t.TempDir()
	directory := filepath.Join(root, "selected")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "only-selected.txt"), []byte("actual approved directory"), 0600); err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err = os.Rename(directory, filepath.Join(root, "original-moved")); err != nil {
		t.Fatal(err)
	}
	if err = os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(directory, "only-selected.txt"), []byte("unapproved replacement path"), 0600); err != nil {
		t.Fatal(err)
	}
	config := fixtureConfig(t, "dirfd")
	config.DirectoryFiles = []*os.File{file}
	child, err := Start(config)
	if err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(child.Stdout)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	if err = child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	code, ok := child.ExitCode()
	if !ok || code != 0 || string(data) != "actual approved directory" {
		t.Fatal("directory path retarget changed actual granted cwd", string(data), code, ok)
	}
	config.ReadFiles = []*os.File{file}
	if _, err = Start(config); err == nil {
		t.Fatal("directory was silently allowed as a regular read file")
	}
}

func TestConcurrentWaitAndOwnerClose(t *testing.T) {
	for range 12 {
		p, err := Start(fixtureConfig(t, "eof"))
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		var wg sync.WaitGroup
		failures := make(chan error, 8)
		for range 8 {
			wg.Go(func() { failures <- p.Wait(ctx) })
		}
		p.BeginClose()
		wg.Wait()
		close(failures)
		for err := range failures {
			if err != nil {
				t.Errorf("concurrent owner close: %v", err)
			}
		}
		cancel()
	}
}
func fixtureConfig(t *testing.T, mode string) Config {
	return Config{Command: os.Args[0], Args: []string{"-test.run=^TestProcessChild$", "--", "--owned-child=" + mode}, Cwd: t.TempDir(), Env: map[string]string{"EXPLICIT": "visible"}}
}
func TestEnvironmentAndNaturalExit(t *testing.T) {
	t.Setenv("ALVA_TEST_AMBIENT_SECRET", "hidden")
	p, err := Start(fixtureConfig(t, "env"))
	if err != nil {
		t.Fatal(err)
	}
	body, err := io.ReadAll(p.Stdout)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err = p.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	code, ok := p.ExitCode()
	if !ok || code != 7 || string(body) != "visible" {
		t.Fatalf("exit=%d,%v body=%q", code, ok, body)
	}
}
func TestEOFDoesNotProveCompletion(t *testing.T) {
	p, err := Start(fixtureConfig(t, "eof"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = io.ReadAll(p.Stdout); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	defer cancel()
	if err = p.Wait(ctx); err == nil {
		t.Fatal("pipe EOF treated as successful exit")
	}
	if code, ok := p.ExitCode(); !ok || code == 0 {
		t.Fatalf("unexpected final code %d,%v", code, ok)
	}
}
