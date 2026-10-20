// INPUT: context, encoding/json, owned native/process, system/privatefs and engine
// OUTPUT: Adapter for explicit Claude/Codex/Pi JSONL processes with bounded private journals
// POS: [integration / native external-engine adapter] Native vendor harness bridge with no shell, ambient configuration or fabricated sessions
package process

import (
	"bufio"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
	owned "github.com/QuincySx/Alva/platform/system/process"
)

type Config struct {
	Bindings           storage.BindingStore
	Vendor             string
	Command            string
	PrefixArgs         []string
	Env                map[string]string
	StateDirectory     string
	ApprovedHostAccess bool
	Timeout            time.Duration
	MaxJournalBytes    int64
}
type Adapter struct {
	config      Config
	mu          sync.Mutex
	active      map[string]context.CancelFunc
	closeMu     sync.Mutex
	storeClosed bool
	lifetime    context.Context
	stop        context.CancelFunc
	wg          sync.WaitGroup
	closed      bool
	closeOnce   sync.Once
	closedDone  chan struct{}
}

var nativeID = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)

// New takes ownership of Bindings only on success. The supplying assembly
// retains it on error; the adapter never selects or substitutes a store.
func New(config Config) (*Adapter, error) {
	if config.Vendor != "codex" && config.Vendor != "claude" && config.Vendor != "pi" {
		return nil, errors.New("unknown JSONL engine vendor")
	}
	if !config.ApprovedHostAccess {
		return nil, errors.New("external engine requires explicit host/account access approval")
	}
	if storage.Missing(config.Bindings) {
		return nil, errors.New("external engine requires identity storage supplied by assembly")
	}
	if config.Timeout == 0 {
		config.Timeout = 10 * time.Minute
	}
	if config.Timeout < time.Second || config.Timeout > 30*time.Minute {
		return nil, errors.New("engine timeout must be 1 second..30 minutes")
	}
	if config.MaxJournalBytes == 0 {
		config.MaxJournalBytes = 32 << 20
	}
	if config.MaxJournalBytes < 4096 || config.MaxJournalBytes > 64<<20 {
		return nil, errors.New("engine journal budget must be 4 KiB..64 MiB")
	}
	dir, err := private.EnsureDir(config.StateDirectory)
	if err != nil {
		return nil, err
	}
	config.StateDirectory = dir
	config.PrefixArgs = append([]string(nil), config.PrefixArgs...)
	env := make(map[string]string)
	for key, value := range config.Env {
		if reservedPlatformHome(key) {
			return nil, errors.New("external engine private profile environment cannot be overridden")
		}
		env[key] = value
	}
	config.Env = env
	lifetime, stop := context.WithCancel(context.Background())
	return &Adapter{config: config, active: make(map[string]context.CancelFunc), lifetime: lifetime, stop: stop, closedDone: make(chan struct{})}, nil
}
func (a *Adapter) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true}
}
func (*Adapter) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (a *Adapter) Cancel(_ context.Context, id string) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	cancel, ok := a.active[id]
	if !ok {
		return engine.ErrSessionNotFound
	}
	cancel()
	return nil
}
func (a *Adapter) args(request engine.Request, runID string) ([]string, error) {
	if request.ResumeSession != "" && !nativeID.MatchString(request.ResumeSession) {
		return nil, errors.New("native session identity is not a safe opaque ID")
	}
	if request.MaxTurns < 0 || request.MaxTurns > 1024 || len(request.SystemPrompt) > 8<<10 || !utf8.ValidString(request.SystemPrompt) || strings.ContainsRune(request.SystemPrompt, '\x00') {
		return nil, errors.New("engine request requires0..1024 turns and at most8KiB UTF-8 system prompt")
	}
	if a.config.Vendor != "claude" && (request.SystemPrompt != "" || request.MaxTurns != 0) {
		return nil, fmt.Errorf("%w: vendor system/turn settings not configured", engine.ErrUnsupported)
	}
	args := append([]string(nil), a.config.PrefixArgs...)
	switch a.config.Vendor {
	case "codex":
		args = append(args, "exec")
		if request.ResumeSession != "" {
			args = append(args, "resume", request.ResumeSession)
		}
		args = append(args, "--json", "--skip-git-repo-check")
		if request.ResumeSession == "" {
			args = append(args, "--sandbox", "read-only", "-C", request.Workspace)
		}
		if request.Model != "" && request.Model != "engine-default" {
			args = append(args, "-m", request.Model)
		}
		args = append(args, "-")
	case "claude":
		args = append(args, "-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "dontAsk", "--safe-mode", "--include-partial-messages")
		if request.SystemPrompt != "" {
			args = append(args, "--append-system-prompt", request.SystemPrompt)
		}
		if request.MaxTurns != 0 {
			args = append(args, "--max-turns", strconv.Itoa(request.MaxTurns))
		}
		if request.ResumeSession != "" {
			args = append(args, "--resume", request.ResumeSession)
		}
		if request.Model != "" && request.Model != "engine-default" {
			args = append(args, "--model", request.Model)
		}
	case "pi":
		args = append(args, "--print", "--mode", "json", "--session-dir", filepath.Join(a.config.StateDirectory, "sessions"), "--tools", "read,grep,find,ls", "--no-extensions", "--no-mcp", "--no-skills", "--no-prompt-templates", "--no-themes", "--no-context-files", "--no-approve", "--offline")
		if request.ResumeSession != "" {
			path, err := a.piSession(request)
			if err != nil {
				return nil, err
			}
			args = append(args, "--session", path)
		} else {
			path, err := a.piFreshSession(runID)
			if err != nil {
				return nil, err
			}
			args = append(args, "--session", path)
		}
		if request.Model != "" && request.Model != "engine-default" {
			args = append(args, "--model", request.Model)
		}
		if request.Provider != "" && request.Provider != "cli-account" && request.Provider != "engine-default" {
			args = append(args, "--provider", request.Provider)
		}
	}
	return args, nil
}

type journal struct {
	mu          sync.Mutex
	file        *os.File
	used, limit int64
	secrets     []string
	failure     error
}

func (j *journal) write(direction string, value string, complete ...bool) error {
	j.mu.Lock()
	defer j.mu.Unlock()
	if j.failure != nil {
		return j.failure
	}
	frame := map[string]any{"direction": direction, "text": value}
	if len(complete) > 0 {
		frame["complete"] = complete[0]
	}
	data, err := json.Marshal(frame)
	if err != nil {
		j.failure = err
		return err
	}
	data, err = redaction.RedactStreamFrame(data, j.secrets...)
	if err != nil {
		j.failure = err
		return err
	}
	if j.used+int64(len(data))+1 > j.limit {
		j.failure = errors.New("engine journal byte budget exceeded; execution stopped")
		return j.failure
	}
	if _, err = j.file.Write(append(data, '\n')); err != nil {
		j.failure = err
		return err
	}
	j.used += int64(len(data)) + 1
	return nil
}
func (a *Adapter) Execute(parent context.Context, request engine.Request, sink func(engine.Event) error) error {
	if err := parent.Err(); err != nil {
		return err
	}
	if sink == nil || request.OwnerID == "" || len(request.OwnerID) > 256 || len(request.Prompt) > 64<<10 || strings.TrimSpace(request.Prompt) == "" || !filepath.IsAbs(request.Workspace) {
		return errors.New("bounded prompt, absolute cwd and event sink required")
	}
	canonical, err := filepath.EvalSymlinks(request.Workspace)
	if err != nil {
		return err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return errors.New("external engine workspace must be an existing real directory")
	}
	request.Workspace = canonical
	a.mu.Lock()
	if a.closed {
		a.mu.Unlock()
		return errors.New("external engine adapter is closed")
	}
	a.wg.Add(1)
	a.mu.Unlock()
	defer a.wg.Done()
	if request.ResumeSession != "" {
		if err := a.authorizeResume(parent, request); err != nil {
			return err
		}
	}
	var identity [16]byte
	if _, err := rand.Read(identity[:]); err != nil {
		return err
	}
	runID := "engine-" + hex.EncodeToString(identity[:])
	args, err := a.args(request, runID)
	if err != nil {
		return err
	}
	dir, err := private.EnsureDir(filepath.Join(a.config.StateDirectory, "journals"))
	if err != nil {
		return err
	}
	file, err := os.OpenFile(filepath.Join(dir, runID+".jsonl"), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	defer file.Close()
	j := &journal{file: file, limit: a.config.MaxJournalBytes}
	j.secrets = redaction.EnvironmentSecrets(a.config.Env)
	ctx, cancel := context.WithTimeout(parent, a.config.Timeout)
	defer cancel()
	detach := context.AfterFunc(a.lifetime, cancel)
	defer detach()
	mapper := newMapper(a.config.Vendor, "engine-journal://"+runID)
	contract := engine.StreamContract{}
	emit := func(event engine.Event) error {
		if err := contract.Accept(event); err != nil {
			return err
		}
		return sink(event)
	}
	env := make(map[string]string)
	for key, value := range a.config.Env {
		env[key] = value
	}
	// Private HOME blocks accidental reads of existing CLI accounts/configuration;
	// credentials must be supplied explicitly through the approved environment.
	home, err := private.EnsureDir(filepath.Join(a.config.StateDirectory, "home"))
	if err != nil {
		return err
	}
	env["HOME"] = home
	env["CODEX_HOME"] = filepath.Join(home, "codex")
	env["CLAUDE_CONFIG_DIR"] = filepath.Join(home, "claude")
	env["PI_CODING_AGENT_DIR"] = filepath.Join(home, "pi")
	platformEnv, err := platformHome(home)
	if err != nil {
		return err
	}
	for key, value := range platformEnv {
		env[key] = value
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	proc, err := owned.Start(owned.Config{Command: a.config.Command, Args: args, Cwd: request.Workspace, Env: env})
	if err != nil {
		if emitErr := emit(engine.Event{Kind: engine.Error, Text: "Approved external engine could not start", Failure: engine.FailureStartup}); emitErr != nil {
			return emitErr
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
	}
	defer func() {
		cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()
		_ = proc.Close(cleanup)
	}()
	closeDone := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			proc.BeginClose()
		case <-closeDone:
		}
	}()
	defer close(closeDone)
	ioErrors := make(chan error, 2)
	go func() {
		if err := j.write("stdin", request.Prompt); err != nil {
			ioErrors <- err
			proc.BeginClose()
			return
		}
		_, err := io.WriteString(proc.Stdin, request.Prompt+"\n")
		proc.Stdin.Close()
		ioErrors <- err
	}()
	go func() {
		readErr := owned.ReadLines(proc.Stderr, 1<<20, func(line owned.StreamLine) error {
			if line.Truncated {
				// A partial credential at the limit cannot be safely retained.
				return j.write("stderr", "[stderr line exceeded 1 MiB; body omitted]")
			}
			return j.write("stderr", string(line.Data), line.Complete)
		})
		if readErr != nil {
			proc.BeginClose()
		}
		ioErrors <- readErr
	}()
	var runErr error
	scanner := bufio.NewScanner(proc.Stdout)
	scanner.Buffer(make([]byte, 4096), 1<<20)
	aliases := make(map[string]bool)
	defer func() {
		a.mu.Lock()
		for id := range aliases {
			delete(a.active, id)
		}
		a.mu.Unlock()
	}()
	for scanner.Scan() {
		line := scanner.Text()
		if err = j.write("stdout", line); err != nil {
			runErr = err
			break
		}
		events, mapErr := mapper.mapLine([]byte(line))
		if mapErr != nil {
			runErr = mapErr
			break
		}
		for _, event := range events {
			if event.Kind == engine.SessionStarted {
				if err = a.bindNative(ctx, request, event.SessionID); err != nil {
					runErr = err
					break
				}
				a.mu.Lock()
				if _, exists := a.active[event.SessionID]; exists {
					a.mu.Unlock()
					runErr = errors.New("external native session already running")
					break
				}
				a.active[event.SessionID] = cancel
				aliases[event.SessionID] = true
				a.mu.Unlock()
			}
			if err = emit(event); err != nil {
				runErr = err
				break
			}
		}
		if runErr != nil {
			break
		}
	}
	if scanner.Err() != nil && ctx.Err() == nil {
		runErr = errors.Join(runErr, errors.New("engine stdout frame read failed or exceeded 1 MiB"))
	}
	if runErr != nil {
		proc.BeginClose()
	}
	for i := 0; i < 2; i++ {
		select {
		case ioErr := <-ioErrors:
			if ctx.Err() == nil {
				runErr = errors.Join(runErr, ioErr)
			}
		case <-time.After(5 * time.Second):
			runErr = errors.Join(runErr, errors.New("engine I/O worker did not exit"))
		}
	}
	if runErr != nil {
		proc.BeginClose()
	}
	runErr = errors.Join(runErr, proc.Wait(ctx))
	if code, available := proc.ExitCode(); !available || code != 0 {
		if ctx.Err() == nil {
			runErr = errors.Join(runErr, errors.New("external engine did not exit successfully"))
		}
	}
	j.mu.Lock()
	runErr = errors.Join(runErr, j.failure)
	j.mu.Unlock()
	if err = file.Sync(); err != nil {
		runErr = errors.Join(runErr, errors.New("engine journal could not durably flush"))
	}
	if ctx.Err() != nil {
		runErr = errors.Join(runErr, ctx.Err())
	}
	if !mapper.done && runErr == nil {
		runErr = errors.New("engine process ended without a native result")
	}
	if mapper.failure != "" {
		runErr = errors.Join(runErr, errors.New(mapper.failure))
	}
	if mapper.pending != "" {
		runErr = errors.Join(runErr, errors.New(mapper.pending))
	}
	for id, name := range mapper.tools {
		if err = emit(engine.Event{Kind: engine.ToolEnd, ID: id, Name: name, IsError: true, Text: "External process ended before tool result", Reference: mapper.reference()}); err != nil {
			return errors.Join(runErr, err)
		}
	}
	outcome := engine.Succeeded
	if runErr != nil {
		outcome = engine.Failed
		kind := engine.FailureExecution
		if ctx.Err() != nil && pureCancellation(runErr) {
			outcome = engine.Cancelled
			kind = engine.FailureCancelled
		}
		if err = emit(engine.Event{Kind: engine.Error, Text: "External engine failed or was stopped; inspect its private journal reference", Failure: kind, Reference: mapper.reference()}); err != nil {
			return errors.Join(runErr, err)
		}
	}
	return emit(engine.Event{Kind: engine.Completed, SessionID: mapper.session, Outcome: outcome, Usage: &mapper.usage, Reference: mapper.reference()})
}

func pureCancellation(err error) bool {
	if err == nil {
		return true
	}
	if err == context.Canceled || err == context.DeadlineExceeded {
		return true
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !pureCancellation(child) {
				return false
			}
		}
		return true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return pureCancellation(wrapped.Unwrap())
	}
	return false
}
