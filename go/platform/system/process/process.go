// INPUT: context, os/exec, explicit bounded argv/environment and platform process ownership
// OUTPUT: Config, Process, Start, BeginClose, Close, Done, Wait, CleanupComplete
// POS: [platform / OS primitive] Native owned foreground process without inherited credentials or shell interpretation
//go:build !windows

package process

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

const SafePath = "/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin"

type Process struct {
	Stdin   io.WriteCloser
	Stdout  io.ReadCloser
	Stderr  io.ReadCloser
	cmd     *exec.Cmd
	once    sync.Once
	observe sync.Mutex
	closing chan struct{}
	done    chan struct{}
	err     error
}

func executable(command string) (string, error) {
	if command == "" || len(command) > 4096 || strings.ContainsRune(command, 0) {
		return "", errors.New("invalid executable")
	}
	if filepath.IsAbs(command) {
		return command, nil
	}
	if strings.ContainsAny(command, "/\\") {
		return "", errors.New("relative executable paths are not allowed")
	}
	for _, dir := range filepath.SplitList(SafePath) {
		p := filepath.Join(dir, command)
		st, err := os.Stat(p)
		if err == nil && st.Mode().IsRegular() && st.Mode().Perm()&0111 != 0 {
			return p, nil
		}
	}
	return "", errors.New("executable not found in safe system PATH")
}
func Start(config Config) (*Process, error) {
	path, err := executable(config.Command)
	if err != nil {
		return nil, err
	}
	if config.Cwd == "" || !filepath.IsAbs(config.Cwd) || len(config.Args) > 256 || len(config.Env) > 128 {
		return nil, errors.New("absolute cwd and bounded process configuration required")
	}
	total := 0
	for _, arg := range config.Args {
		total += len(arg)
		if strings.ContainsRune(arg, 0) {
			return nil, errors.New("invalid process argument")
		}
	}
	if total > 64<<10 {
		return nil, errors.New("process arguments exceed 64 KiB")
	}
	values := map[string]string{"PATH": SafePath, "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"}
	for key, value := range config.Env {
		if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || strings.ContainsRune(value, 0) || len(value) > 8192 {
			return nil, errors.New("invalid explicit environment")
		}
		values[key] = value
		total += len(key) + len(value)
	}
	if total > 128<<10 {
		return nil, errors.New("explicit environment exceeds limit")
	}
	cmd := exec.Command(path, append([]string(nil), config.Args...)...)
	fileCount := len(config.ReadFiles) + len(config.DirectoryFiles)
	if config.StdinFile != nil {
		fileCount++
	}
	if fileCount > 8 {
		return nil, errors.New("at most 8 explicit regular/directory read descriptors allowed")
	}
	for _, file := range config.ReadFiles {
		if file == nil {
			return nil, errors.New("nil granted file descriptor")
		}
		info, err := file.Stat()
		if err != nil || !info.Mode().IsRegular() {
			return nil, errors.New("granted read descriptor must be an open regular file")
		}
		if err = validateReadFile(file); err != nil {
			return nil, err
		}
	}
	for _, file := range config.DirectoryFiles {
		if file == nil {
			return nil, errors.New("nil pinned directory descriptor")
		}
		info, err := file.Stat()
		if err != nil || !info.IsDir() {
			return nil, errors.New("pinned directory descriptor must be an open directory")
		}
		if err = validateReadFile(file); err != nil {
			return nil, err
		}
	}
	cmd.ExtraFiles = append(append([]*os.File(nil), config.ReadFiles...), config.DirectoryFiles...)
	cmd.Dir = config.Cwd
	keys := make([]string, 0, len(values))
	for k := range values {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		cmd.Env = append(cmd.Env, k+"="+values[k])
	}
	if err = configure(cmd); err != nil {
		return nil, err
	}
	var in io.WriteCloser = closedInput{}
	if config.StdinFile != nil {
		info, statErr := config.StdinFile.Stat()
		if statErr != nil || !info.Mode().IsRegular() {
			return nil, errors.New("stdin file must be an open regular file")
		}
		if err := validateReadFile(config.StdinFile); err != nil {
			return nil, err
		}
		cmd.Stdin = config.StdinFile
	} else {
		in, err = cmd.StdinPipe()
		if err != nil {
			return nil, err
		}
	}
	out, err := cmd.StdoutPipe()
	if err != nil {
		in.Close()
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		in.Close()
		out.Close()
		return nil, err
	}
	if err = cmd.Start(); err != nil {
		in.Close()
		out.Close()
		stderr.Close()
		return nil, errors.New("owned executable could not start")
	}
	return &Process{Stdin: in, Stdout: out, Stderr: stderr, cmd: cmd, closing: make(chan struct{}), done: make(chan struct{})}, nil
}
func (p *Process) BeginClose() {
	p.once.Do(func() {
		p.observe.Lock()
		close(p.closing)
		// No Wait has reaped the leader: group identity cannot have been recycled.
		killErr := terminate(p.cmd)
		if killErr != nil {
			killErr = fmt.Errorf("terminate owned group: %w", killErr)
		}
		p.Stdin.Close()
		p.Stdout.Close()
		p.Stderr.Close()
		p.observe.Unlock()
		go func() {
			waitErr := p.cmd.Wait()
			var exit *exec.ExitError
			if errors.As(waitErr, &exit) {
				waitErr = nil
			}
			if waitErr != nil {
				waitErr = fmt.Errorf("reap owned leader: %w", waitErr)
			}
			p.err = errors.Join(killErr, waitErr)
			close(p.done)
		}()
	})
}
func (p *Process) Done() <-chan struct{} { return p.done }

// CleanupComplete is true only after both owned group termination and the one
// leader reap succeeded. EOF or a returned observer error is not this evidence.
func (p *Process) CleanupComplete() bool {
	select {
	case <-p.done:
		return p.err == nil
	default:
		return false
	}
}

// Wait observes natural leader exit without reaping it, then kills remaining
// owned descendants before the one and only reap. EOF alone is not proof of exit.
func (p *Process) Wait(ctx context.Context) error {
	timer := time.NewTicker(10 * time.Millisecond)
	defer timer.Stop()
	for {
		select {
		case <-p.done:
			return p.err
		case <-ctx.Done():
			return p.cleanupFailure(ctx.Err())
		default:
		}
		// Serialize the no-reap observation with the transition into closing.
		// Once closing starts only the unique owner may observe/reap this PID.
		p.observe.Lock()
		select {
		case <-p.closing:
			p.observe.Unlock()
			return p.Close(ctx)
		default:
		}
		ended, err := exited(p.cmd.Process.Pid)
		p.observe.Unlock()
		if err != nil {
			return p.cleanupFailure(err)
		}
		if ended {
			return p.Close(ctx)
		}
		select {
		case <-p.done:
			return p.err
		case <-ctx.Done():
			return p.cleanupFailure(ctx.Err())
		case <-timer.C:
		}
	}
}
func (p *Process) cleanupFailure(cause error) error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	return errors.Join(cause, p.Close(ctx))
}

// ExitCode is available only after Close has reaped the leader. It preserves
// the actual exit result even though lifecycle Close treats termination as expected.
func (p *Process) ExitCode() (int, bool) {
	select {
	case <-p.done:
		if p.cmd.ProcessState == nil {
			return -1, false
		}
		return p.cmd.ProcessState.ExitCode(), true
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
