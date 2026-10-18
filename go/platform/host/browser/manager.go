// INPUT: context, encoding/json, errors, fmt, io, os, filepath, strings, sync, time, native/process and protocol/browser.
// OUTPUT: Config, Launch, IOEvent, ObserverError, Manager and explicit browser start/status/client/stop/cleanup lifecycle.
// POS: [platform / native execution host] Native browser lifetime uses only approved executable/cwd/env, a new private profile and actual owned-process cleanup.
package browser

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	protocol "github.com/QuincySx/Alva/go/integration/protocol/browser"
	"github.com/QuincySx/Alva/go/platform/system/process"
)

type IOEvent struct {
	InstanceID, Stream string
	Frame              json.RawMessage
}

// ObserverError keeps the private storage cause available to error inspection
// without echoing credentials or full foreign journal errors to a public UI.
type ObserverError struct{ Err error }

func (*ObserverError) Error() string   { return "browser process observer failed" }
func (e *ObserverError) Unwrap() error { return e.Err }

type Config struct {
	Executable, Cwd, TempRoot string
	Env                       map[string]string
	Headless                  bool
	Proxy                     string
	StartupTimeout            time.Duration
	Observe                   func(context.Context, IOEvent) error
}
type Launch struct {
	InstanceID, Executable, Cwd string
	Headless                    bool
}
type Status struct {
	ID             string `json:"id"`
	Running        bool   `json:"running"`
	Headless       bool   `json:"headless"`
	Reaped         bool   `json:"reaped"`
	ProfileRemoved bool   `json:"profile_removed"`
}
type instance struct {
	id, dir  string
	p        *process.Process
	client   *protocol.Client
	wg       sync.WaitGroup
	drained  chan struct{}
	mu       sync.Mutex
	observe  sync.Mutex
	err      error
	bytes    int
	endpoint chan string
	ready    chan struct{}
	life     context.Context
	cancel   context.CancelFunc
	cleanup  chan struct{}
}
type Manager struct {
	mu        sync.Mutex
	config    Config
	instances map[string]*instance
	closed    bool
}

func New(config Config) (*Manager, error) {
	if !filepath.IsAbs(config.Executable) || !filepath.IsAbs(config.Cwd) || !filepath.IsAbs(config.TempRoot) || config.Observe == nil {
		return nil, errors.New("explicit absolute browser executable/cwd/private temp root and observer required")
	}
	info, err := os.Stat(config.Executable)
	if err != nil || !info.Mode().IsRegular() || (runtime.GOOS != "windows" && info.Mode().Perm()&0111 == 0) || (runtime.GOOS == "windows" && !strings.EqualFold(filepath.Ext(config.Executable), ".exe")) {
		return nil, errors.New("explicit approved browser executable unavailable")
	}
	if err = checkPrivate(config.TempRoot); err != nil {
		return nil, err
	}
	if config.StartupTimeout == 0 {
		config.StartupTimeout = 20 * time.Second
	}
	if config.StartupTimeout <= 0 || config.StartupTimeout > time.Minute {
		return nil, errors.New("browser startup timeout must be within one minute")
	}
	if config.Proxy != "" {
		u, err := url.Parse(config.Proxy)
		if err != nil || len(config.Proxy) > 2048 || u.User != nil || u.Host == "" || (u.Scheme != "http" && u.Scheme != "https" && u.Scheme != "socks5") || u.Path != "" || u.RawQuery != "" || u.Fragment != "" {
			return nil, errors.New("explicit approved proxy without embedded credentials required")
		}
	}
	environment := map[string]string{}
	for key, value := range config.Env {
		name := key
		if runtime.GOOS == "windows" {
			name = strings.ToUpper(name)
		}
		if name == "HOME" || name == "USERPROFILE" || name == "XDG_CONFIG_HOME" || name == "XDG_CACHE_HOME" || name == "TMPDIR" || name == "CHROME_USER_DATA_DIR" || (runtime.GOOS == "windows" && (name == "APPDATA" || name == "LOCALAPPDATA" || name == "TEMP" || name == "TMP" || name == "HOMEDRIVE" || name == "HOMEPATH")) {
			return nil, errors.New("browser private home/profile environment cannot be overridden")
		}
		environment[key] = value
	}
	config.Env = environment
	return &Manager{config: config, instances: map[string]*instance{}}, nil
}
func validID(id string) bool {
	if id == "" || len(id) > 64 {
		return false
	}
	for _, ch := range id {
		if !(ch >= 'a' && ch <= 'z' || ch >= 'A' && ch <= 'Z' || ch >= '0' && ch <= '9' || strings.ContainsRune("_-.", ch)) {
			return false
		}
	}
	return id != "." && id != ".."
}
func (m *Manager) Start(ctx context.Context, id string, before func(context.Context, Launch) error) (Status, error) {
	if !validID(id) || before == nil {
		return Status{}, errors.New("valid browser id and prior durable-risk callback required")
	}
	m.mu.Lock()
	if m.closed || len(m.instances) >= 4 {
		m.mu.Unlock()
		return Status{}, errors.New("browser manager closed or four-instance budget exceeded")
	}
	if _, ok := m.instances[id]; ok {
		m.mu.Unlock()
		return Status{}, errors.New("browser instance already exists")
	}
	if err := ctx.Err(); err != nil {
		m.mu.Unlock()
		return Status{}, err
	}
	if err := checkPrivate(m.config.TempRoot); err != nil {
		m.mu.Unlock()
		return Status{}, err
	}
	dir, err := os.MkdirTemp(m.config.TempRoot, "alva-browser-")
	if err != nil {
		m.mu.Unlock()
		return Status{}, err
	}
	life, lifeCancel := context.WithCancel(context.Background())
	i := &instance{id: id, dir: dir, endpoint: make(chan string, 1), drained: make(chan struct{}), ready: make(chan struct{}), life: life, cancel: lifeCancel, cleanup: make(chan struct{}, 1)}
	m.instances[id] = i
	m.mu.Unlock()
	defer close(i.ready)
	startup, cancel := context.WithTimeout(ctx, m.config.StartupTimeout)
	defer cancel()
	stopLife := context.AfterFunc(life, cancel)
	defer stopLife()
	removeUnstarted := func(cause error) (Status, error) {
		lifeCancel()
		removeErr := os.RemoveAll(dir)
		if removeErr == nil {
			m.remove(i)
		}
		return Status{ID: id, Headless: m.config.Headless, ProfileRemoved: removeErr == nil}, errors.Join(cause, removeErr)
	}
	if err = before(startup, Launch{InstanceID: id, Executable: m.config.Executable, Cwd: m.config.Cwd, Headless: m.config.Headless}); err != nil {
		return removeUnstarted(err)
	}
	if err = startup.Err(); err != nil {
		return removeUnstarted(err)
	}
	for _, name := range []string{"home", "tmp"} {
		if err = os.Mkdir(filepath.Join(dir, name), 0700); err != nil {
			return removeUnstarted(err)
		}
	}
	env := map[string]string{"HOME": filepath.Join(dir, "home"), "USERPROFILE": filepath.Join(dir, "home"), "XDG_CONFIG_HOME": filepath.Join(dir, "home"), "XDG_CACHE_HOME": filepath.Join(dir, "home"), "TMPDIR": filepath.Join(dir, "tmp")}
	if runtime.GOOS == "windows" {
		env["APPDATA"], env["LOCALAPPDATA"] = filepath.Join(dir, "home"), filepath.Join(dir, "home")
		env["TEMP"], env["TMP"] = filepath.Join(dir, "tmp"), filepath.Join(dir, "tmp")
		env["HOMEDRIVE"], env["HOMEPATH"] = filepath.VolumeName(env["USERPROFILE"]), strings.TrimPrefix(env["USERPROFILE"], filepath.VolumeName(env["USERPROFILE"]))
	}
	for key, value := range m.config.Env {
		env[key] = value
	}
	args := []string{"--remote-debugging-address=127.0.0.1", "--remote-debugging-port=0", "--user-data-dir=" + filepath.Join(dir, "profile"), "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--disable-sync", "--disable-default-apps", "--disable-background-networking", "--password-store=basic", "--use-mock-keychain", "--window-size=1280,900"}
	if m.config.Headless {
		args = append(args, "--headless=new")
	}
	if m.config.Proxy != "" {
		args = append(args, "--proxy-server="+m.config.Proxy)
	}
	args = append(args, "about:blank")
	i.p, err = process.Start(process.Config{Command: m.config.Executable, Args: args, Cwd: m.config.Cwd, Env: env})
	if err != nil {
		return removeUnstarted(err)
	}
	context.AfterFunc(life, i.p.BeginClose)
	i.wg.Add(3)
	go m.read(i, "stdout", i.p.Stdout)
	go m.read(i, "stderr", i.p.Stderr)
	go func() {
		defer i.wg.Done()
		if waitErr := i.p.Wait(context.Background()); waitErr != nil {
			i.mu.Lock()
			i.err = errors.Join(i.err, waitErr)
			i.mu.Unlock()
		}
	}()
	go func() { i.wg.Wait(); close(i.drained) }()
	select {
	case endpoint := <-i.endpoint:
		i.client, err = protocol.Connect(startup, endpoint)
	case <-i.p.Done():
		i.mu.Lock()
		err = errors.Join(errors.New("owned browser process ended before CDP became ready"), i.err)
		i.mu.Unlock()
	case <-startup.Done():
		err = startup.Err()
	}
	if err == nil {
		i.mu.Lock()
		err = i.err
		i.mu.Unlock()
	}
	if err != nil {
		cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer end()
		status, closeErr := m.stopInstance(cleanup, i)
		return status, errors.Join(err, closeErr)
	}
	return Status{ID: id, Running: true, Headless: m.config.Headless}, nil
}
func (m *Manager) read(i *instance, stream string, reader io.Reader) {
	defer i.wg.Done()
	var observerErr error
	err := process.ReadLines(reader, 1<<20, func(line process.StreamLine) error {
		i.mu.Lock()
		i.bytes += len(line.Data)
		over := i.bytes > 4<<20
		i.mu.Unlock()
		if over {
			return errors.New("browser process transcript exceeded 4 MiB; stopped")
		}
		frame := map[string]any{"complete": line.Complete, "truncated": line.Truncated}
		if utf8.Valid(line.Data) {
			frame["text"] = string(line.Data)
		} else {
			frame["base64"] = base64.StdEncoding.EncodeToString(line.Data)
		}
		encoded, _ := json.Marshal(frame)
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		i.observe.Lock()
		observeErr := m.config.Observe(ctx, IOEvent{InstanceID: i.id, Stream: stream, Frame: encoded})
		i.observe.Unlock()
		if observeErr != nil {
			observerErr = &ObserverError{Err: observeErr}
			return observerErr
		}
		if stream == "stderr" {
			text := strings.TrimSpace(string(line.Data))
			if strings.HasPrefix(text, "DevTools listening on ") {
				endpoint := strings.TrimPrefix(text, "DevTools listening on ")
				if err := protocol.ValidateEndpoint(endpoint); err != nil {
					return err
				}
				select {
				case i.endpoint <- endpoint:
				default:
				}
			}
		}
		return nil
	})
	if observerErr != nil || err != nil && !errors.Is(err, os.ErrClosed) {
		i.mu.Lock()
		i.err = errors.Join(i.err, err)
		i.mu.Unlock()
		i.p.BeginClose()
	}
}
func (m *Manager) Client(id string) (*protocol.Client, error) {
	m.mu.Lock()
	i := m.instances[id]
	m.mu.Unlock()
	if i == nil {
		return nil, errors.New("browser instance not running")
	}
	select {
	case <-i.ready:
	default:
		return nil, errors.New("browser instance startup pending")
	}
	if i.client == nil || i.p == nil {
		return nil, errors.New("browser instance not running")
	}
	i.mu.Lock()
	err := i.err
	i.mu.Unlock()
	if err != nil {
		return nil, err
	}
	select {
	case <-i.p.Done():
		return nil, errors.New("browser process ended")
	default:
	}
	return i.client, nil
}
func (m *Manager) Status() []Status {
	m.mu.Lock()
	defer m.mu.Unlock()
	var result []Status
	for id, i := range m.instances {
		select {
		case <-i.ready:
		default:
			result = append(result, Status{ID: id, Headless: m.config.Headless})
			continue
		}
		if i.p == nil {
			result = append(result, Status{ID: id, Headless: m.config.Headless})
			continue
		}
		running := i.client != nil
		select {
		case <-i.p.Done():
			running = false
		default:
		}
		result = append(result, Status{ID: id, Running: running, Headless: m.config.Headless, Reaped: i.p.CleanupComplete()})
	}
	sort.Slice(result, func(a, b int) bool { return result[a].ID < result[b].ID })
	return result
}
func (m *Manager) Stop(ctx context.Context, id string) (Status, error) {
	m.mu.Lock()
	i := m.instances[id]
	m.mu.Unlock()
	if i == nil {
		return Status{}, errors.New("browser instance not found")
	}
	i.cancel()
	select {
	case <-i.ready:
	case <-ctx.Done():
		return Status{ID: id, Headless: m.config.Headless}, ctx.Err()
	}
	return m.stopInstance(ctx, i)
}
func (m *Manager) remove(i *instance) {
	m.mu.Lock()
	if m.instances[i.id] == i {
		delete(m.instances, i.id)
	}
	m.mu.Unlock()
}
func (m *Manager) stopInstance(ctx context.Context, i *instance) (Status, error) {
	status := Status{ID: i.id, Headless: m.config.Headless}
	i.cancel()
	select {
	case i.cleanup <- struct{}{}:
	case <-ctx.Done():
		return status, ctx.Err()
	}
	defer func() { <-i.cleanup }()
	if i.p == nil {
		removeErr := os.RemoveAll(i.dir)
		status.ProfileRemoved = removeErr == nil
		if removeErr == nil {
			m.remove(i)
		}
		return status, removeErr
	}
	// Stop the owned process before CDP cancellation; library target-close
	// requests cannot keep a hostile/unresponsive browser alive forever.
	err := i.p.Close(ctx)
	status.Reaped = i.p.CleanupComplete()
	if !status.Reaped {
		return status, errors.Join(errors.New("browser cleanup has not completed"), err)
	}
	if i.client != nil {
		i.client.Close()
	}
	select {
	case <-i.drained:
	case <-ctx.Done():
		return status, errors.Join(err, ctx.Err())
	}
	i.mu.Lock()
	err = errors.Join(err, i.err)
	i.mu.Unlock()
	removeErr := os.RemoveAll(i.dir)
	status.ProfileRemoved = removeErr == nil
	if removeErr == nil {
		m.remove(i)
	}
	return status, errors.Join(err, removeErr)
}
func (m *Manager) Close(ctx context.Context) error {
	m.mu.Lock()
	m.closed = true
	instances := make([]*instance, 0, len(m.instances))
	for _, i := range m.instances {
		instances = append(instances, i)
	}
	m.mu.Unlock()
	for _, i := range instances {
		i.cancel()
	}
	var err error
	for _, i := range instances {
		select {
		case <-i.ready:
		case <-ctx.Done():
			return errors.Join(err, ctx.Err())
		}
		_, stopErr := m.stopInstance(ctx, i)
		err = errors.Join(err, stopErr)
	}
	return err
}
func (m *Manager) String() string {
	return fmt.Sprintf("explicit owned browser manager (%s)", m.config.Executable)
}

// CleanupComplete reports physical resources, independently of a retained
// observer/journal error returned by Close. Entries leave this map only after
// actual process reap, protocol/I/O exit and successful private-profile removal.
func (m *Manager) CleanupComplete() bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.instances) == 0
}
