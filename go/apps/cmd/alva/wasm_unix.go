// INPUT: context, pinned os.Root, host/command, native/process and explicit Unix approval file
// OUTPUT: Pinned-directory native shell executor and cancellable approval reads
// POS: [L6 application] Explicitly unsandboxed host escape hatch, not WASM confinement or ambient authority.
//go:build darwin || linux

package main

import (
	"context"
	"errors"
	"fmt"
	"os"
	"syscall"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/host/command"
	sandboxhost "github.com/QuincySx/Alva/go/platform/sandbox/host"
	"github.com/QuincySx/Alva/go/platform/system/process"
	sandboxabi "github.com/QuincySx/Alva/go/sdk/sandbox/abi"
	"golang.org/x/sys/unix"
)

func executeWASMShell(ctx context.Context, target sandboxhost.EscalationTarget) (sandboxabi.EscalationResponse, error) {
	if target.Directory == nil {
		return sandboxabi.EscalationResponse{}, errors.New("approved shell requires a pinned directory")
	}
	directory, err := target.Directory.Open(".")
	if err != nil {
		return sandboxabi.EscalationResponse{}, err
	}
	defer directory.Close()
	executable, err := os.Executable()
	if err != nil {
		return sandboxabi.EscalationResponse{}, err
	}
	// The owned child receives only the pinned directory capability. Its
	// private trampoline fchdirs before exec; parent process cwd never changes.
	// This also works on Darwin where chdir(/dev/fd/N) is ENOTDIR.
	result, err := command.Run(ctx, process.Config{Command: executable, Args: []string{wasmShellTrampoline, target.Request.Command}, Cwd: "/", DirectoryFiles: []*os.File{directory}, Env: map[string]string{"PATH": process.SafePath, "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8"}}, nil, 1<<20)
	if err != nil {
		return sandboxabi.EscalationResponse{}, err
	}
	if !result.Complete || !result.Reaped {
		return sandboxabi.EscalationResponse{}, errors.New("approved host shell did not complete cleanup")
	}
	if !utf8.Valid(result.Stdout) || !utf8.Valid(result.Stderr) {
		return sandboxabi.EscalationResponse{}, errors.New("host shell emitted non-UTF8 output; no lossy conversion performed")
	}
	return sandboxabi.EscalationResponse{Stdout: string(result.Stdout), Stderr: string(result.Stderr), ExitCode: result.ExitCode}, nil
}

const wasmShellTrampoline = "__alva_private_wasm_shell_v1"

// This private child entry is not an authorization boundary: an OS user able
// to launch this binary already owns native execution. Only the approved host
// Executor launches it inside our product workflow. No persisted job is replayed.
func runWASMShellTrampoline(args []string) (bool, error) {
	if len(args) == 0 || args[0] != wasmShellTrampoline {
		return false, nil
	}
	if len(args) != 2 || len(args[1]) > 64<<10 || !utf8.ValidString(args[1]) {
		return true, errors.New("invalid private shell invocation")
	}
	directory := os.NewFile(3, "approved-pinned-directory")
	if directory == nil {
		return true, errors.New("missing private directory descriptor")
	}
	defer directory.Close()
	info, err := directory.Stat()
	if err != nil || !info.IsDir() {
		return true, errors.New("invalid private directory capability")
	}
	flags, err := unix.FcntlInt(directory.Fd(), unix.F_GETFL, 0)
	if err != nil || flags&unix.O_ACCMODE != unix.O_RDONLY {
		return true, errors.New("private directory must be opened read-only")
	}
	if err := directory.Chdir(); err != nil {
		return true, err
	}
	if err := directory.Close(); err != nil {
		return true, err
	}
	return true, syscall.Exec("/bin/sh", []string{"sh", "-c", args[1]}, []string{"PATH=" + process.SafePath, "LANG=C.UTF-8", "LC_ALL=C.UTF-8"})
}
func openApprovalTTY() (*os.File, error) { return os.OpenFile("/dev/tty", os.O_RDWR, 0) }
func readApprovalFile(ctx context.Context, file *os.File) (string, error) {
	fd := file.Fd()
	if uint64(fd) > uint64(^uint32(0)>>1) {
		return "", errors.New("approval descriptor out of range")
	}
	// The CLI owns the sole reader. Poll before each one-byte read, preserving
	// later approvals without buffering past a line; no detached reader task.
	reader := &approvalFileReader{ctx: ctx, file: file, fd: int32(fd)}
	return finiteApprovalLine(ctx, reader)
}

type approvalFileReader struct {
	ctx  context.Context
	file *os.File
	fd   int32
}

func (r *approvalFileReader) Read(data []byte) (int, error) {
	for {
		if err := r.ctx.Err(); err != nil {
			return 0, err
		}
		timeout := 100
		if deadline, ok := r.ctx.Deadline(); ok {
			timeout = min(timeout, max(1, int(time.Until(deadline).Milliseconds())))
		}
		fds := []unix.PollFd{{Fd: r.fd, Events: unix.POLLIN}}
		_, err := unix.Poll(fds, timeout)
		if errors.Is(err, unix.EINTR) {
			continue
		}
		if err != nil {
			return 0, err
		}
		if fds[0].Revents&unix.POLLNVAL != 0 {
			return 0, fmt.Errorf("approval descriptor closed")
		}
		if fds[0].Revents&(unix.POLLIN|unix.POLLHUP|unix.POLLERR) != 0 {
			return r.file.Read(data)
		}
	}
}
