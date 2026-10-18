// INPUT: Explicit native process configuration and Windows job/process/stdio creation APIs
// OUTPUT: Start, Process, BeginClose, Close, Wait, Done, ExitCode and physical CleanupComplete
// POS: [platform / OS primitive] Create directly inside a non-inherited kill-on-close Job Object before any child execution.
//go:build windows

package process

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
	"unsafe"

	"golang.org/x/sys/windows"
)

// Only the OS-provided system directory participates in bare-name lookup. No
// caller PATH/PATHEXT/current-directory search or environment discovery occurs.
var SafePath = func() string { path, _ := windows.GetSystemDirectory(); return path }()

const procThreadAttributeJobList = 0x0002000d

type Process struct {
	Stdin         io.WriteCloser
	Stdout        io.ReadCloser
	Stderr        io.ReadCloser
	process, job  windows.Handle
	once          sync.Once
	observe       sync.Mutex
	closing, done chan struct{}
	err           error
	code          uint32
	hasCode       bool
}

func windowsConfiguration(config Config) (string, []string, error) {
	if config.Command == "" || len(config.Command) > 4096 || !utf8.ValidString(config.Command) || strings.ContainsAny(config.Command, "\x00\"") || config.Cwd == "" || !filepath.IsAbs(config.Cwd) || !utf8.ValidString(config.Cwd) || strings.ContainsRune(config.Cwd, 0) || len(config.Args) > 256 || len(config.Env) > 128 {
		return "", nil, errors.New("explicit executable, absolute cwd and bounded native process configuration required")
	}
	if len(config.ReadFiles)+len(config.DirectoryFiles) > 0 {
		return "", nil, errors.New("Unix fd3 inheritance is unsupported on Windows; no ambient handles are inherited")
	}
	path := config.Command
	if !filepath.IsAbs(path) {
		if strings.ContainsAny(path, `/\:`) || SafePath == "" {
			return "", nil, errors.New("executable must be absolute or a bare system-directory name")
		}
		if filepath.Ext(path) == "" {
			path += ".exe"
		}
		path = filepath.Join(SafePath, path)
	}
	volume := filepath.VolumeName(path)
	if len(volume) != 2 || !strings.EqualFold(filepath.Ext(path), ".exe") {
		return "", nil, errors.New("owned Windows executable must be a local .exe; implicit command scripts are rejected")
	}
	rootPath, _ := windows.UTF16PtrFromString(volume + `\`)
	if kind := windows.GetDriveType(rootPath); kind != windows.DRIVE_FIXED && kind != windows.DRIVE_REMOVABLE {
		return "", nil, errors.New("owned executable requires a local drive")
	}
	if info, err := os.Stat(path); err != nil || !info.Mode().IsRegular() {
		return "", nil, errors.New("owned executable is not a regular local file")
	}
	root, err := windows.GetWindowsDirectory()
	if err != nil || SafePath == "" {
		return "", nil, errors.New("native system directory unavailable")
	}
	values := map[string]string{"PATH": SafePath, "SYSTEMROOT": root, "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"}
	total := 0
	for _, arg := range config.Args {
		if !utf8.ValidString(arg) || strings.ContainsRune(arg, 0) {
			return "", nil, errors.New("invalid process argument")
		}
		total += len(arg)
	}
	if total > 64<<10 {
		return "", nil, errors.New("process arguments exceed 64 KiB")
	}
	seen := make(map[string]bool)
	for key, value := range config.Env {
		canonical := strings.ToUpper(key)
		if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || !utf8.ValidString(key) || !utf8.ValidString(value) || strings.ContainsRune(value, 0) || len(value) > 8192 || seen[canonical] {
			return "", nil, errors.New("invalid or case-colliding explicit environment")
		}
		if canonical == "SYSTEMROOT" && !strings.EqualFold(value, root) {
			return "", nil, errors.New("native system root cannot be redirected")
		}
		seen[canonical] = true
		values[canonical] = value
		total += len(key) + len(value)
	}
	if total > 128<<10 {
		return "", nil, errors.New("explicit environment exceeds limit")
	}
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	environment := make([]string, 0, len(keys))
	for _, key := range keys {
		environment = append(environment, key+"="+values[key])
	}
	return path, environment, nil
}

func Start(config Config) (_ *Process, result error) {
	path, environment, err := windowsConfiguration(config)
	if err != nil {
		return nil, err
	}
	application, _ := windows.UTF16PtrFromString(path)
	line, err := windows.UTF16FromString(windows.ComposeCommandLine(append([]string{path}, config.Args...)))
	if err != nil || len(line) > 32767 {
		return nil, errors.New("native process command line exceeds the Windows limit")
	}
	cwd, err := windows.UTF16PtrFromString(config.Cwd)
	if err != nil {
		return nil, errors.New("invalid process cwd")
	}
	var block []uint16
	for _, value := range environment {
		encoded, _ := windows.UTF16FromString(value)
		block = append(block, encoded...)
	}
	block = append(block, 0)
	job, err := windows.CreateJobObject(nil, nil)
	if err != nil {
		return nil, errors.New("owned job creation failed")
	}
	defer func() {
		if result != nil {
			windows.CloseHandle(job)
		}
	}()
	limits := windows.JOBOBJECT_EXTENDED_LIMIT_INFORMATION{}
	limits.BasicLimitInformation.LimitFlags = windows.JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
	if _, err := windows.SetInformationJobObject(job, windows.JobObjectExtendedLimitInformation, uintptr(unsafe.Pointer(&limits)), uint32(unsafe.Sizeof(limits))); err != nil {
		return nil, errors.New("owned job limits could not be applied")
	}
	// A narrow HANDLE_LIST is mandatory even though only our three child pipe
	// endpoints are marked inheritable. The job itself is never inherited.
	attributes, err := windows.NewProcThreadAttributeList(2)
	if err != nil {
		return nil, errors.New("owned process attributes unavailable")
	}
	defer attributes.Delete()
	if err := attributes.Update(procThreadAttributeJobList, unsafe.Pointer(&job), unsafe.Sizeof(job)); err != nil {
		return nil, errors.New("creation-time job assignment is unsupported; no process was started")
	}
	var handles []windows.Handle
	defer func() {
		for _, handle := range handles {
			windows.CloseHandle(handle)
		}
	}()
	makePipe := func(parentReads bool) (*os.File, windows.Handle, error) {
		var read, write windows.Handle
		sa := windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), InheritHandle: 1}
		if err := windows.CreatePipe(&read, &write, &sa, 0); err != nil {
			return nil, 0, err
		}
		parent, child := write, read
		if parentReads {
			parent, child = read, write
		}
		if err := windows.SetHandleInformation(parent, windows.HANDLE_FLAG_INHERIT, 0); err != nil {
			windows.CloseHandle(read)
			windows.CloseHandle(write)
			return nil, 0, err
		}
		handles = append(handles, child)
		file := os.NewFile(uintptr(parent), "owned-stdio")
		if file == nil {
			windows.CloseHandle(parent)
			return nil, 0, errors.New("owned pipe unavailable")
		}
		return file, child, nil
	}
	var in io.WriteCloser = closedInput{}
	var childIn windows.Handle
	if config.StdinFile != nil {
		info, statErr := config.StdinFile.Stat()
		if statErr != nil || !info.Mode().IsRegular() {
			return nil, errors.New("stdin file must be an open regular file")
		}
		connection, err := config.StdinFile.SyscallConn()
		if err != nil {
			return nil, errors.New("stdin file handle unavailable")
		}
		var duplicateErr error
		controlErr := connection.Control(func(handle uintptr) {
			duplicateErr = windows.DuplicateHandle(windows.CurrentProcess(), windows.Handle(handle), windows.CurrentProcess(), &childIn, windows.GENERIC_READ|windows.SYNCHRONIZE, true, 0)
		})
		if childIn != 0 {
			handles = append(handles, childIn)
		}
		if controlErr != nil || duplicateErr != nil || childIn == 0 {
			return nil, errors.New("read-only stdin inheritance unavailable")
		}
	} else {
		in, childIn, err = makePipe(false)
		if err != nil {
			return nil, errors.New("owned stdin unavailable")
		}
	}
	defer func() {
		if result != nil {
			in.Close()
		}
	}()
	out, childOut, err := makePipe(true)
	if err != nil {
		return nil, errors.New("owned stdout unavailable")
	}
	defer func() {
		if result != nil {
			out.Close()
		}
	}()
	stderr, childErr, err := makePipe(true)
	if err != nil {
		return nil, errors.New("owned stderr unavailable")
	}
	defer func() {
		if result != nil {
			stderr.Close()
		}
	}()
	inherited := []windows.Handle{childIn, childOut, childErr}
	if err := attributes.Update(windows.PROC_THREAD_ATTRIBUTE_HANDLE_LIST, unsafe.Pointer(&inherited[0]), uintptr(len(inherited))*unsafe.Sizeof(inherited[0])); err != nil {
		return nil, errors.New("exact owned stdio inheritance unavailable")
	}
	startup := windows.StartupInfoEx{StartupInfo: windows.StartupInfo{Cb: uint32(unsafe.Sizeof(windows.StartupInfoEx{})), Flags: windows.STARTF_USESTDHANDLES, StdInput: childIn, StdOutput: childOut, StdErr: childErr}, ProcThreadAttributeList: attributes.List()}
	var info windows.ProcessInformation
	flags := uint32(windows.EXTENDED_STARTUPINFO_PRESENT | windows.CREATE_UNICODE_ENVIRONMENT | windows.CREATE_NO_WINDOW)
	err = windows.CreateProcess(application, &line[0], nil, nil, true, flags, &block[0], cwd, &startup.StartupInfo, &info)
	runtime.KeepAlive(attributes)
	runtime.KeepAlive(inherited)
	runtime.KeepAlive(block)
	if err != nil {
		return nil, errors.New("owned executable could not start in its job")
	}
	windows.CloseHandle(info.Thread)
	return &Process{Stdin: in, Stdout: out, Stderr: stderr, process: info.Process, job: job, closing: make(chan struct{}), done: make(chan struct{})}, nil
}

type jobAccounting struct {
	TotalUserTime, TotalKernelTime, PeriodUserTime, PeriodKernelTime int64
	PageFaults, TotalProcesses, ActiveProcesses, TerminatedProcesses uint32
}

func (p *Process) BeginClose() {
	p.once.Do(func() {
		p.observe.Lock()
		close(p.closing)
		p.observe.Unlock()
		go func() {
			// Kill before touching synchronous pipe endpoints: Close must never
			// wait on a blocked ReadFile while the child still owns its writer.
			if err := windows.TerminateJobObject(p.job, 1); err != nil {
				p.err = errors.New("owned job termination failed; physical cleanup is unconfirmed")
				close(p.done)
				return
			}
			for {
				var accounting jobAccounting
				if err := windows.QueryInformationJobObject(p.job, windows.JobObjectBasicAccountingInformation, uintptr(unsafe.Pointer(&accounting)), uint32(unsafe.Sizeof(accounting)), nil); err != nil {
					p.err = errors.New("owned job cleanup observation failed")
					close(p.done)
					return
				}
				if accounting.ActiveProcesses == 0 {
					break
				}
				time.Sleep(5 * time.Millisecond)
			}
			status, err := windows.WaitForSingleObject(p.process, windows.INFINITE)
			if err == nil && status == windows.WAIT_OBJECT_0 {
				err = windows.GetExitCodeProcess(p.process, &p.code)
				p.hasCode = err == nil
			} else if err == nil {
				err = errors.New("owned leader exit was not confirmed")
			}
			p.Stdin.Close()
			p.Stdout.Close()
			p.Stderr.Close()
			p.err = errors.Join(err, windows.CloseHandle(p.process), windows.CloseHandle(p.job))
			close(p.done)
		}()
	})
}

func (p *Process) Done() <-chan struct{} { return p.done }
func (p *Process) CleanupComplete() bool {
	select {
	case <-p.done:
		return p.err == nil
	default:
		return false
	}
}
func (p *Process) ExitCode() (int, bool) {
	select {
	case <-p.done:
		return int(p.code), p.hasCode
	default:
		return 0, false
	}
}
func (p *Process) Close(ctx context.Context) error {
	p.BeginClose()
	select {
	case <-p.done:
		return p.err
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (p *Process) cleanupFailure(cause error) error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return errors.Join(cause, p.Close(ctx))
}
func (p *Process) Wait(ctx context.Context) error {
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	for {
		if err := ctx.Err(); err != nil {
			return p.cleanupFailure(err)
		}
		p.observe.Lock()
		select {
		case <-p.closing:
			p.observe.Unlock()
			return p.Close(ctx)
		default:
		}
		status, err := windows.WaitForSingleObject(p.process, 0)
		p.observe.Unlock()
		if err != nil {
			return p.cleanupFailure(errors.New("owned leader observation failed"))
		}
		if status == windows.WAIT_OBJECT_0 {
			return p.Close(ctx)
		}
		if status != uint32(windows.WAIT_TIMEOUT) {
			return p.cleanupFailure(errors.New("unexpected owned leader state"))
		}
		select {
		case <-p.done:
			return p.err
		case <-ctx.Done():
			return p.cleanupFailure(ctx.Err())
		case <-ticker.C:
		}
	}
}
