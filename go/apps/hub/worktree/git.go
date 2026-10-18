// INPUT: Explicit approved absolute Git executable, private product root and bounded fixed argument vectors
// OUTPUT: Manager native process ownership, bounded Git command capture and retryable terminal Close
// POS: [L7 Hub product] Concrete product worktree host never uses shell, ambient Git configuration, network or user credentials
package worktree

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"
	"time"

	private "github.com/QuincySx/Alva/go/platform/storage/sqlite"
	"github.com/QuincySx/Alva/go/platform/system/process"
)

type Config struct {
	GitExecutable, Root string
	ApprovedHostAccess  bool
}
type Manager struct {
	config        Config
	mu            sync.Mutex
	closed        bool
	children      map[*process.Process]struct{}
	active        int
	idle          chan struct{}
	life          context.Context
	cancel        context.CancelFunc
	root          *os.Root
	rootInfo      os.FileInfo
	directories   map[string]os.FileInfo
	rootClosed    bool
	operationGate chan struct{}
}
type capture struct {
	data   []byte
	err    error
	stream int
}

func New(config Config) (*Manager, error) {
	if !config.ApprovedHostAccess || !filepath.IsAbs(config.GitExecutable) {
		return nil, errors.New("explicit approved absolute Git executable required")
	}
	info, err := os.Stat(config.GitExecutable)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0111 == 0 {
		return nil, errors.New("explicit Git executable unavailable")
	}
	config.Root, err = private.EnsurePrivateDir(config.Root)
	if err != nil {
		return nil, err
	}
	directories := map[string]os.FileInfo{}
	for _, name := range []string{"home", "hooks", "trees", "evidence"} {
		if _, err = private.EnsurePrivateDir(filepath.Join(config.Root, name)); err != nil {
			return nil, err
		}
		directories[name], err = os.Lstat(filepath.Join(config.Root, name))
		if err != nil {
			return nil, err
		}
	}
	root, err := os.OpenRoot(config.Root)
	if err != nil {
		return nil, err
	}
	rootFile, err := root.Open(".")
	if err != nil {
		root.Close()
		return nil, err
	}
	rootInfo, err := rootFile.Stat()
	err = errors.Join(err, rootFile.Close())
	if err != nil {
		root.Close()
		return nil, err
	}
	life, cancel := context.WithCancel(context.Background())
	idle := make(chan struct{})
	close(idle)
	return &Manager{config: config, children: map[*process.Process]struct{}{}, life: life, cancel: cancel, idle: idle, root: root, rootInfo: rootInfo, directories: directories, operationGate: make(chan struct{}, 1)}, nil
}
func (m *Manager) checkRoot() error {
	info, err := os.Lstat(m.config.Root)
	if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || !os.SameFile(info, m.rootInfo) {
		return errors.New("private managed root identity changed")
	}
	for name, expected := range m.directories {
		actual, err := m.root.Lstat(name)
		if err != nil || !actual.IsDir() || actual.Mode()&os.ModeSymlink != 0 || !os.SameFile(actual, expected) {
			return errors.New("private managed directory identity changed")
		}
	}
	return nil
}
func (m *Manager) begin() (func(), error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.closed {
		return nil, errors.New("worktree Git owner is closed")
	}
	if m.active >= 8 {
		return nil, errors.New("worktree operation budget exceeded")
	}
	if m.active == 0 {
		m.idle = make(chan struct{})
	}
	m.active++
	return func() {
		m.mu.Lock()
		m.active--
		if m.active == 0 {
			close(m.idle)
		}
		m.mu.Unlock()
	}, nil
}
func (m *Manager) run(ctx context.Context, cwd string, args []string, env map[string]string, input []byte, maximum int) ([]byte, error) {
	return m.runExit(ctx, cwd, args, env, input, maximum, false)
}
func (m *Manager) runExit(ctx context.Context, cwd string, args []string, env map[string]string, input []byte, maximum int, difference bool) ([]byte, error) {
	if maximum < 1 || maximum > 128<<20 || len(input) > 16<<20 {
		return nil, errors.New("bounded Git capture/input budget required")
	}
	end, err := m.begin()
	if err != nil {
		return nil, err
	}
	defer end()
	if err = m.checkRoot(); err != nil {
		return nil, err
	}
	owned, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	stopOwner := context.AfterFunc(m.life, cancel)
	defer stopOwner()
	if m.life.Err() != nil {
		cancel()
	}
	if err = owned.Err(); err != nil {
		return nil, err
	}
	settings := map[string]string{"HOME": filepath.Join(m.config.Root, "home"), "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null", "GIT_TERMINAL_PROMPT": "0", "GIT_NO_REPLACE_OBJECTS": "1", "GIT_ALLOW_PROTOCOL": "file", "GIT_OPTIONAL_LOCKS": "0"}
	for k, v := range env {
		settings[k] = v
	}
	prefix := []string{"-c", "core.hooksPath=" + filepath.Join(m.config.Root, "hooks"), "-c", "core.fsmonitor=false", "-c", "core.untrackedCache=false", "-c", "core.attributesFile=/dev/null", "-c", "credential.helper=", "-c", "protocol.allow=never", "-c", "maintenance.auto=false", "-c", "gc.auto=0"}
	// Recheck the actual canonical cwd before and after process start. This is
	// not atomic fchdir: Darwin /dev/fd directory paths cannot be used as cwd.
	// File snapshot/apply IO separately uses genuinely pinned parent handles.
	directory, err := os.OpenRoot(cwd)
	if err != nil {
		return nil, err
	}
	defer directory.Close()
	pinned, err := directory.Open(".")
	if err != nil {
		return nil, err
	}
	defer pinned.Close()
	actual, err := pinned.Stat()
	named, nameErr := os.Lstat(cwd)
	if err != nil || nameErr != nil || named.Mode()&os.ModeSymlink != 0 || !os.SameFile(actual, named) {
		return nil, errors.New("Git cwd identity changed while pinning")
	}
	child, err := process.Start(process.Config{Command: m.config.GitExecutable, Cwd: cwd, Args: append(prefix, args...), Env: settings})
	if err != nil {
		return nil, err
	}
	m.mu.Lock()
	m.children[child] = struct{}{}
	m.mu.Unlock()
	var identityErr error
	latest, latestErr := os.Lstat(cwd)
	if latestErr != nil || latest.Mode()&os.ModeSymlink != 0 || !os.SameFile(actual, latest) {
		identityErr = errors.New("Git cwd changed during process start; inspect possible operation, do not replay")
		cancel()
	}
	stop := context.AfterFunc(owned, child.BeginClose)
	defer stop()
	parts := make(chan capture, 3)
	go func() {
		// A no-input command may legitimately close stdin immediately. A zero
		// byte pipe Write can still report EPIPE; it is not a missing input or
		// capture failure and must not spuriously kill an otherwise valid Git.
		var e error
		if len(input) > 0 {
			_, e = child.Stdin.Write(input)
		}
		e = errors.Join(e, child.Stdin.Close())
		parts <- capture{err: e, stream: 2}
	}()
	read := func(r io.Reader, stream, budget int) {
		data, e := io.ReadAll(io.LimitReader(r, int64(budget)+1))
		if len(data) > budget {
			data = nil
			e = errors.Join(e, errors.New("Git output budget exceeded"))
			child.BeginClose()
		}
		parts <- capture{data, e, stream}
	}
	go read(child.Stdout, 0, maximum)
	go read(child.Stderr, 1, 64<<10)
	var output []byte
	var failure error = identityErr
	for i := 0; i < 3; i++ {
		part := <-parts
		if part.stream == 0 {
			output = part.data
		}
		if part.err != nil && !errors.Is(part.err, os.ErrClosed) {
			failure = errors.Join(failure, errors.New("Git capture/write failed"))
			child.BeginClose()
		}
	}
	if owned.Err() != nil {
		failure = errors.Join(failure, owned.Err())
	}
	if failure == nil {
		failure = child.Wait(owned)
	}
	cleanup, finish := context.WithTimeout(context.Background(), 5*time.Second)
	defer finish()
	failure = errors.Join(failure, child.Close(cleanup))
	if child.CleanupComplete() {
		m.mu.Lock()
		delete(m.children, child)
		m.mu.Unlock()
	} else {
		failure = errors.Join(failure, errors.New("Git physical cleanup incomplete; owner retained for Close retry"))
	}
	code, exited := child.ExitCode()
	if !exited || (code != 0 && !(difference && code == 1)) {
		failure = errors.Join(failure, errors.New("Git command did not exit successfully"))
	}
	if failure != nil {
		return nil, failure
	}
	return output, nil
}
func (m *Manager) Close(ctx context.Context) error {
	m.mu.Lock()
	m.closed = true
	m.cancel()
	idle := m.idle
	children := make([]*process.Process, 0, len(m.children))
	for child := range m.children {
		children = append(children, child)
	}
	m.mu.Unlock()
	var err error
	for _, child := range children {
		err = errors.Join(err, child.Close(ctx))
		if child.CleanupComplete() {
			m.mu.Lock()
			delete(m.children, child)
			m.mu.Unlock()
		}
	}
	select {
	case <-idle:
	case <-ctx.Done():
		return errors.Join(err, ctx.Err())
	}
	m.mu.Lock()
	unfinished := m.active != 0 || len(m.children) != 0
	if !unfinished && !m.rootClosed {
		closeErr := m.root.Close()
		err = errors.Join(err, closeErr)
		m.rootClosed = closeErr == nil
	}
	m.mu.Unlock()
	if !m.CleanupComplete() {
		return errors.Join(err, errors.New("worktree physical cleanup incomplete; exact children retained for explicit Close retry"))
	}
	return err
}
func (m *Manager) CleanupComplete() bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.closed && m.active == 0 && len(m.children) == 0 && m.rootClosed
}
