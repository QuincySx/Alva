// INPUT: Real Windows helper processes, owned Job Object handles and disposable ambient file handles
// OUTPUT: Exact job membership, descendant cleanup, owner-crash cleanup and no ambient inheritance regressions
// POS: [platform / OS primitive] Native acceptance observes kernel handles and actual exits rather than PID guesses or pipe EOF.
//go:build windows

package process

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var isProcessInJob = windows.NewLazySystemDLL("kernel32.dll").NewProc("IsProcessInJob")

func helperArguments(mode string) []string {
	return []string{"-test.run=^TestWindowsOwnedChild$", "--", "windows-owned-fixture", mode}
}
func fixtureEnvironment() []string {
	root, _ := windows.GetWindowsDirectory()
	return []string{"SystemRoot=" + root, "GORACE=atexit_sleep_ms=0"}
}

func TestWindowsOwnedChild(t *testing.T) {
	index := -1
	for i, arg := range os.Args {
		if arg == "windows-owned-fixture" {
			index = i
			break
		}
	}
	if index < 0 {
		return
	}
	args := os.Args[index+1:]
	executable, _ := os.Executable()
	switch args[0] {
	case "sleep":
		fmt.Println("running")
		time.Sleep(30 * time.Second)
		os.Exit(0)
	case "parent":
		child := exec.Command(executable, helperArguments("sleep")...)
		child.Env = fixtureEnvironment()
		child.Stdout = io.Discard
		child.Stderr = io.Discard
		if child.Start() != nil {
			os.Exit(10)
		}
		fmt.Println(child.Process.Pid)
		_, _ = bufio.NewReader(os.Stdin).ReadString('\n')
		os.Exit(0)
	case "owner":
		cwd, _ := os.Getwd()
		child, err := Start(Config{Command: executable, Args: helperArguments("sleep"), Cwd: cwd})
		if err != nil {
			os.Exit(11)
		}
		if line, err := bufio.NewReader(child.Stdout).ReadString('\n'); err != nil || line != "running\n" {
			os.Exit(12)
		}
		pid, err := windows.GetProcessId(child.process)
		if err != nil {
			os.Exit(13)
		}
		fmt.Println(pid)
		_, _ = bufio.NewReader(os.Stdin).ReadString('\n')
		// Intentionally bypass Close: the OS must close this owner's job handle.
		os.Exit(0)
	case "probe":
		handle, _ := strconv.ParseUint(args[1], 10, 64)
		var info windows.ByHandleFileInformation
		if windows.GetFileInformationByHandle(windows.Handle(handle), &info) != nil {
			os.Exit(0)
		}
		identity := fmt.Sprintf("%d:%d:%d", info.VolumeSerialNumber, info.FileIndexHigh, info.FileIndexLow)
		if identity == args[2] {
			os.Exit(19)
		}
		os.Exit(0)
	}
	os.Exit(20)
}

func readFixtureProcess(t *testing.T, reader io.Reader) windows.Handle {
	t.Helper()
	line, err := bufio.NewReader(reader).ReadString('\n')
	if err != nil {
		t.Fatal(err)
	}
	pid, err := strconv.ParseUint(strings.TrimSpace(line), 10, 32)
	if err != nil {
		t.Fatal(err)
	}
	handle, err := windows.OpenProcess(windows.SYNCHRONIZE|windows.PROCESS_QUERY_LIMITED_INFORMATION|windows.PROCESS_TERMINATE, false, uint32(pid))
	if err != nil {
		t.Fatal(err)
	}
	return handle
}

func TestWindowsNaturalLeaderExitReapsWholeJob(t *testing.T) {
	executable, _ := os.Executable()
	child, err := Start(Config{Command: executable, Args: helperArguments("parent"), Cwd: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	defer child.Close(context.Background())
	descendant := readFixtureProcess(t, child.Stdout)
	defer windows.CloseHandle(descendant)
	defer windows.TerminateProcess(descendant, 1)
	for _, handle := range []windows.Handle{child.process, descendant} {
		var member int32
		ok, _, err := isProcessInJob.Call(uintptr(handle), uintptr(child.job), uintptr(unsafe.Pointer(&member)))
		if ok == 0 || member == 0 {
			t.Fatal("initial/descendant process is outside its exact owned job", err)
		}
	}
	if _, err := io.WriteString(child.Stdin, "finish\n"); err != nil {
		t.Fatal(err)
	}
	if err := child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	if code, ok := child.ExitCode(); !ok || code != 0 || !child.CleanupComplete() {
		t.Fatal("natural outcome or actual job cleanup lost", code, ok)
	}
	if status, err := windows.WaitForSingleObject(descendant, 2000); err != nil || status != windows.WAIT_OBJECT_0 {
		t.Fatal("descendant survived natural leader cleanup", err)
	}
}

func TestWindowsOwnerExitKillsJobWithoutExplicitClose(t *testing.T) {
	executable, _ := os.Executable()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	owner := exec.CommandContext(ctx, executable, helperArguments("owner")...)
	owner.Env = fixtureEnvironment()
	owner.Dir = t.TempDir()
	stdin, err := owner.StdinPipe()
	if err != nil {
		t.Fatal(err)
	}
	stdout, err := owner.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := owner.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = owner.Process.Kill(); _ = owner.Wait() }()
	descendant := readFixtureProcess(t, stdout)
	defer windows.CloseHandle(descendant)
	defer windows.TerminateProcess(descendant, 1)
	if _, err := io.WriteString(stdin, "exit without Close\n"); err != nil {
		t.Fatal(err)
	}
	if err := owner.Wait(); err != nil {
		t.Fatal(err)
	}
	if status, err := windows.WaitForSingleObject(descendant, 2000); err != nil || status != windows.WAIT_OBJECT_0 {
		t.Fatal("owner exit left an owned process alive", err)
	}
}

func TestWindowsExactHandleListDoesNotInheritAmbientFile(t *testing.T) {
	file, err := os.Create(filepath.Join(t.TempDir(), "ambient-fixture"))
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	handle := windows.Handle(file.Fd())
	if err := windows.SetHandleInformation(handle, windows.HANDLE_FLAG_INHERIT, windows.HANDLE_FLAG_INHERIT); err != nil {
		t.Fatal(err)
	}
	defer windows.SetHandleInformation(handle, windows.HANDLE_FLAG_INHERIT, 0)
	var info windows.ByHandleFileInformation
	if err := windows.GetFileInformationByHandle(handle, &info); err != nil {
		t.Fatal(err)
	}
	identity := fmt.Sprintf("%d:%d:%d", info.VolumeSerialNumber, info.FileIndexHigh, info.FileIndexLow)
	executable, _ := os.Executable()
	args := append(helperArguments("probe"), strconv.FormatUint(uint64(handle), 10), identity)
	child, err := Start(Config{Command: executable, Args: args, Cwd: t.TempDir()})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	defer child.Close(context.Background())
	if err := child.Wait(ctx); err != nil {
		t.Fatal(err)
	}
	if code, ok := child.ExitCode(); !ok || code != 0 {
		t.Fatal("ambient inheritable file escaped exact HANDLE_LIST", code, ok)
	}
}

func TestWindowsConfigurationRejectsCaseCollisionsAndImplicitScripts(t *testing.T) {
	executable, _ := os.Executable()
	for _, config := range []Config{
		{Command: executable, Cwd: t.TempDir(), Env: map[string]string{"Path": "one", "PATH": "two"}},
		{Command: executable, Cwd: t.TempDir(), Env: map[string]string{"SYSTEMROOT": `C:\redirected`}},
		{Command: `C:\unapproved.cmd`, Cwd: t.TempDir()},
	} {
		if child, err := Start(config); err == nil {
			child.Close(context.Background())
			t.Fatal("unsafe process configuration accepted")
		}
	}
}
