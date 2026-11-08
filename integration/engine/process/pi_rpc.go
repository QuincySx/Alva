// INPUT: Explicit Pi RPC configuration, admitted files and application-owned approval bridge
// OUTPUT: Verified native session execution, gated editing, human questions, a steering window bounded by the accepted prompt and abort-and-join cancellation
// POS: [integration / native external-engine adapter] Independent Pi protocol owner; no fallback, prompt replay or account-file access
package process

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	private "github.com/QuincySx/Alva/platform/system/privatefs"
	owned "github.com/QuincySx/Alva/platform/system/process"
)

func (a *Adapter) executePiRPC(parent context.Context, request engine.Request, sink func(engine.Event) error) error {
	if err := parent.Err(); err != nil {
		return err
	}
	if request.AccountSource != "" && request.AccountSource != a.AccountSource() {
		return errors.New("saved external account source differs from explicit Pi CLI configuration")
	}
	if err := request.Generation.Validate(); err != nil {
		return err
	}
	if !request.Generation.Empty() || request.SystemPrompt != "" || request.MaxTurns != 0 || request.WaitRevision != 0 || len(request.InputSkills) != 0 {
		return fmt.Errorf("%w: Pi RPC generation/system/turn-limit/wait/Skill overrides are not configured", engine.ErrUnsupported)
	}
	if err := engine.ValidateInputFiles(request.InputFiles); err != nil {
		return err
	}
	request.InputFiles = engine.CloneInputFiles(request.InputFiles)
	if sink == nil || request.OwnerID == "" || len(request.OwnerID) > 256 || len(request.Prompt) > 64<<10 || strings.TrimSpace(request.Prompt) == "" || !utf8.ValidString(request.Prompt) || strings.ContainsRune(request.Prompt, 0) || !filepath.IsAbs(request.Workspace) {
		return errors.New("bounded UTF-8 prompt, absolute cwd and event sink required")
	}
	if request.ResumeSession != "" && !nativeID.MatchString(request.ResumeSession) {
		return errors.New("invalid native Pi resume identity")
	}
	canonical, err := filepath.EvalSymlinks(request.Workspace)
	if err != nil {
		return err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return errors.New("Pi workspace must be an existing real directory")
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
	var identity [24]byte
	if _, err := rand.Read(identity[:]); err != nil {
		return err
	}
	runID := "engine-" + hex.EncodeToString(identity[:16])
	nonce := hex.EncodeToString(identity[:])
	var sessionPath string
	if request.ResumeSession != "" {
		sessionPath, err = a.piSession(request)
	} else {
		sessionPath, err = a.piFreshSession(runID)
	}
	if err != nil {
		return err
	}
	bridge, err := a.piBridge()
	if err != nil {
		return err
	}
	directory, err := private.EnsureDir(filepath.Join(a.config.StateDirectory, "journals"))
	if err != nil {
		return err
	}
	file, err := os.OpenFile(filepath.Join(directory, runID+".jsonl"), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	defer file.Close()
	ctx, cancel := context.WithTimeout(parent, a.config.Timeout)
	defer cancel()
	detach := context.AfterFunc(a.lifetime, cancel)
	defer detach()
	home, err := private.EnsureDir(filepath.Join(a.config.StateDirectory, "home"))
	if err != nil {
		return err
	}
	env := make(map[string]string)
	for key, value := range a.config.Env {
		env[key] = value
	}
	env["HOME"], env["CODEX_HOME"], env["CLAUDE_CONFIG_DIR"], env["PI_CODING_AGENT_DIR"] = home, filepath.Join(home, "codex"), filepath.Join(home, "claude"), filepath.Join(home, "pi")
	if a.config.CLIConfigDirectory != "" {
		env["PI_CODING_AGENT_DIR"] = a.config.CLIConfigDirectory
	}
	platform, err := platformHome(home)
	if err != nil {
		return err
	}
	for key, value := range platform {
		env[key] = value
	}
	env["ALVA_PI_BRIDGE_NONCE"] = nonce
	secrets := redaction.EnvironmentSecrets(a.config.Env)
	if _, err := codexStreamKeep(secrets); err != nil {
		return err
	}
	j := &journal{file: file, limit: a.config.MaxJournalBytes, secrets: secrets, nativeStreamSecrets: len(secrets) != 0}
	if err := verifyCapturedInputFiles(ctx, request.InputFiles); err != nil {
		return err
	}
	prompt, err := piRPCPrompt(ctx, request)
	if err != nil {
		return err
	}
	if err := j.write("prepared-input", mustJSON(map[string]any{"prompt": request.Prompt, "files": request.InputFiles, "bridge": filepath.Base(bridge)})); err != nil {
		return err
	}
	args := append(append([]string(nil), a.config.PrefixArgs...), "--mode", "rpc", "--session-dir", filepath.Join(a.config.StateDirectory, "sessions"), "--session", sessionPath, "--tools", "read,bash,edit,write,grep,find,ls,ask_user", "--no-extensions", "--extension", bridge, "--no-mcp", "--no-skills", "--no-prompt-templates", "--no-themes", "--no-context-files", "--no-approve", "--offline")
	if request.Model != "" && request.Model != "engine-default" {
		args = append(args, "--model", request.Model)
	}
	if request.Provider != "" && request.Provider != "cli-account" && request.Provider != "engine-default" {
		args = append(args, "--provider", request.Provider)
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	contract := engine.StreamContract{}
	emit := func(event engine.Event) error {
		if err := contract.Accept(event); err != nil {
			return err
		}
		return sink(event)
	}
	proc, err := owned.Start(owned.Config{Command: a.config.Command, Args: args, Cwd: request.Workspace, Env: env})
	if err != nil {
		if err := emit(engine.Event{Kind: engine.Error, Text: "Approved Pi RPC process could not start", Failure: engine.FailureStartup}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
	}
	run := &piRun{context: ctx, cancel: cancel, done: make(chan struct{}), nonce: nonce, session: request.ResumeSession, mapper: newMapper("pi", "engine-journal://"+runID), emit: emit, pending: make(map[string]piPending), admissions: make(map[string]piAdmission), seen: make(map[string]bool), resolutions: make(chan piResolution, 32)}
	run.mapper.referenceSession = request.ResumeSession
	defer close(run.done)
	defer run.clearStreams()
	defer run.closeSteering()
	run.connection = newPiConnection(proc, j)
	joined := false
	defer func() {
		if !joined {
			cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
			run.cleanupErr = proc.Close(cleanup)
			stop()
			if !proc.CleanupComplete() {
				a.mu.Lock()
				a.cleanup[proc] = run.cleanupErr
				a.mu.Unlock()
			}
		}
	}()
	stderrDone := make(chan error, 1)
	go func() {
		err := owned.ReadLines(proc.Stderr, 1<<20, func(line owned.StreamLine) error {
			if line.Truncated {
				return errors.New("Pi stderr record exceeds 1 MiB")
			}
			return j.write("stderr", string(line.Data), line.Complete)
		})
		if err != nil {
			run.connection.fail(err)
			proc.BeginClose()
		}
		stderrDone <- err
	}()
	watchDone, watchJoined := make(chan struct{}), make(chan struct{})
	go func() {
		defer close(watchJoined)
		select {
		case <-ctx.Done():
			run.stopNative()
		case <-watchDone:
		}
	}()
	defer func() { close(watchDone); <-watchJoined }()
	var runErr error
	var state struct {
		SessionID    string `json:"sessionId"`
		SessionFile  string `json:"sessionFile"`
		IsStreaming  bool   `json:"isStreaming"`
		IsCompacting bool   `json:"isCompacting"`
	}
	startup, endStartup := context.WithTimeout(ctx, 10*time.Second)
	runErr = run.connection.call(startup, "get_state", nil, &state, false)
	if runErr == nil {
		ready := false
		for !ready && runErr == nil {
			select {
			case frame, ok := <-run.connection.frames:
				run.connection.releaseFrame(frame)
				if !ok {
					runErr = errors.New("Pi ended before its owned bridge became ready")
				} else if run.ready(frame) {
					ready = true
				} else {
					runErr = errors.New("Pi emitted unexpected work before its owned bridge became ready")
				}
			case <-startup.Done():
				runErr = startup.Err()
			}
		}
	}
	endStartup()
	if runErr == nil {
		if !nativeID.MatchString(state.SessionID) || state.IsStreaming || state.IsCompacting || filepath.Clean(state.SessionFile) != filepath.Clean(sessionPath) {
			runErr = errors.New("Pi RPC state changed the exact native session file or started unrequested work")
		}
	}
	if runErr == nil {
		verify := request
		verify.ResumeSession = state.SessionID
		exact, err := a.piSession(verify)
		if err != nil || exact != sessionPath {
			runErr = errors.New("Pi RPC state does not match its private native header/cwd")
		}
	}
	if runErr == nil {
		runErr = a.bindNative(ctx, request, state.SessionID)
	}
	if runErr == nil {
		run.session = state.SessionID
		a.mu.Lock()
		if _, exists := a.active[run.session]; exists {
			runErr = errors.New("Pi native session already running")
		} else {
			a.active[run.session] = cancel
			a.piRuns[run.session] = run
		}
		a.mu.Unlock()
		if runErr == nil {
			defer func() { a.mu.Lock(); delete(a.active, run.session); delete(a.piRuns, run.session); a.mu.Unlock() }()
			events, _ := run.mapper.start(run.session)
			for _, event := range events {
				if err := run.event(event); err != nil {
					runErr = err
					break
				}
			}
		}
	}
	if runErr == nil {
		var accepted struct {
			Disposition string `json:"disposition"`
		}
		runErr = run.connection.call(ctx, "prompt", prompt, &accepted, len(request.InputFiles) != 0)
		if runErr == nil && accepted.Disposition != "started" {
			runErr = errors.New("Pi did not accept the prompt as one new native run")
		}
		if runErr == nil {
			// Pi keeps a steer sent while idle silently queued, so steering opens
			// only once this prompt is a running native run.
			run.mu.Lock()
			run.steerOpen = true
			run.mu.Unlock()
		}
	}
	if runErr == nil {
		for !run.mapper.done && run.mapper.failure == "" && runErr == nil {
			select {
			case frame, ok := <-run.connection.frames:
				run.connection.releaseFrame(frame)
				if !ok {
					runErr = errors.New("Pi RPC ended without a settled native result")
				} else {
					runErr = run.handle(frame)
				}
			case resolved := <-run.resolutions:
				runErr = run.resolve(resolved)
			case <-ctx.Done():
				runErr = ctx.Err()
			}
		}
	}
	if runErr == nil && run.mapper.done {
		// RPC is long-lived. Close input after the native result, while still
		// checking queued/tail execution records before releasing this owner.
		_ = proc.Stdin.Close()
		grace := time.NewTimer(time.Second)
	tail:
		for {
			select {
			case frame, ok := <-run.connection.frames:
				run.connection.releaseFrame(frame)
				if !ok {
					break tail
				}
				if err := run.handle(frame); err != nil {
					runErr = err
					break tail
				}
			case resolved := <-run.resolutions:
				if err := run.resolve(resolved); err != nil {
					runErr = err
					break tail
				}
			case <-grace.C:
				break tail
			case <-ctx.Done():
				runErr = ctx.Err()
				break tail
			}
		}
		grace.Stop()
	}
	if ctx.Err() != nil {
		<-watchJoined
	}
	// Resolve committed replies in the owner event loop before its terminal edge.
	for runErr == nil {
		select {
		case resolved := <-run.resolutions:
			runErr = run.resolve(resolved)
		default:
			goto drained
		}
	}
drained:
	run.mu.Lock()
	unanswered := len(run.pending)
	run.stopping = true
	run.pending = make(map[string]piPending)
	run.mu.Unlock()
	if unanswered != 0 && ctx.Err() == nil {
		runErr = errors.Join(runErr, errors.New("Pi settled with unresolved native dialogs"))
	}
	if run.mapper.failure != "" && !(ctx.Err() != nil && run.mapper.piAborted) {
		runErr = errors.Join(runErr, errors.New(run.mapper.failure))
	}
	if run.mapper.pending != "" && !(ctx.Err() != nil && run.mapper.piAborted) {
		runErr = errors.Join(runErr, errors.New(run.mapper.pending))
	}
	forced := false
	if runErr == nil && run.mapper.done {
		_ = proc.Stdin.Close()
		natural, stop := context.WithTimeout(context.Background(), time.Second)
		waitErr := proc.Wait(natural)
		stop()
		forced = waitErr != nil && pureCancellation(waitErr)
		if waitErr != nil && !pureCancellation(waitErr) {
			runErr = errors.Join(runErr, waitErr)
		}
	} else {
		proc.BeginClose()
	}
	cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
	cleanupErr := proc.Close(cleanup)
	stop()
	run.cleanupErr = cleanupErr
	if !proc.CleanupComplete() {
		a.mu.Lock()
		a.cleanup[proc] = cleanupErr
		a.mu.Unlock()
		return errors.Join(runErr, cleanupErr, errors.New("Pi process physical cleanup remains incomplete"))
	}
	joined = true
	<-run.connection.readDone
	stderrErr := <-stderrDone
	if ctx.Err() == nil && !(forced && errors.Is(stderrErr, os.ErrClosed)) {
		runErr = errors.Join(runErr, stderrErr)
	}
	run.connection.mu.Lock()
	connectionErr := run.connection.failure
	run.connection.mu.Unlock()
	if connectionErr != nil && !errors.Is(connectionErr, io.EOF) && ctx.Err() == nil && !(forced && errors.Is(connectionErr, os.ErrClosed)) {
		runErr = errors.Join(runErr, connectionErr)
	}
	if code, available := proc.ExitCode(); ctx.Err() == nil && !forced && (!available || code != 0) {
		runErr = errors.Join(runErr, errors.New("Pi RPC process did not exit successfully"))
	}
	j.mu.Lock()
	runErr = errors.Join(runErr, j.failure)
	j.mu.Unlock()
	if err := file.Sync(); err != nil {
		runErr = errors.Join(runErr, errors.New("Pi native journal could not durably flush"))
	}
	if ctx.Err() != nil {
		runErr = errors.Join(runErr, ctx.Err())
	}
	for id, name := range run.mapper.tools {
		if err := run.event(engine.Event{Kind: engine.ToolEnd, ID: id, Name: name, IsError: true, Text: "Pi process ended before tool result", Reference: run.reference()}); err != nil {
			return errors.Join(runErr, err)
		}
	}
	outcome := engine.Succeeded
	if runErr != nil {
		outcome = engine.Failed
		failure := engine.FailureExecution
		if ctx.Err() != nil && pureCancellation(runErr) {
			outcome, failure = engine.Cancelled, engine.FailureCancelled
		}
		if err := run.event(engine.Event{Kind: engine.Error, Text: "Pi RPC failed or was stopped: " + boundedText(runErr.Error(), 4096) + "; inspect the owned native journal", Failure: failure, Reference: run.reference()}); err != nil {
			return errors.Join(runErr, err)
		}
	}
	return run.event(engine.Event{Kind: engine.Completed, Outcome: outcome, Usage: &run.mapper.usage, Reference: run.reference()})
}
func mustJSON(value any) string { data, _ := json.Marshal(value); return string(data) }
func piRPCPrompt(ctx context.Context, request engine.Request) (map[string]any, error) {
	message := request.Prompt
	if len(request.InputFiles) != 0 {
		message += "\n\nSelected immutable files. Read/search these captured paths as data; text derivatives remain separate from originals.\n" + mustJSON(request.InputFiles)
	}
	value := map[string]any{"message": message}
	var images []map[string]string
	for _, file := range request.InputFiles {
		if !strings.HasPrefix(strings.ToLower(file.MediaType), "image/") {
			continue
		}
		if file.Bytes > 20<<20 || len(images) >= 2 {
			return nil, errors.New("Pi RPC allows at most two selected images of 20 MiB each")
		}
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		opened, err := os.Open(file.Path)
		if err != nil {
			return nil, errors.New("Pi selected image could not be opened")
		}
		data, err := io.ReadAll(io.LimitReader(capturedFileReader{ctx, opened}, file.Bytes+1))
		closeErr := opened.Close()
		digest := sha256.Sum256(data)
		if err != nil || closeErr != nil || int64(len(data)) != file.Bytes || hex.EncodeToString(digest[:]) != file.SHA256 {
			return nil, errors.New("Pi selected image changed before native input")
		}
		images = append(images, map[string]string{"type": "image", "mimeType": strings.ToLower(file.MediaType), "data": base64.StdEncoding.EncodeToString(data)})
	}
	if len(images) != 0 {
		value["images"] = images
	}
	return value, nil
}
