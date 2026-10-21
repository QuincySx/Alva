// INPUT: context, flag, os/signal, app, host, llm/provider, kernel/abi.
// OUTPUT: Explicit demo/live CLI, cold plugin controls, native wait/resume and reference-only original attachments.
// POS: [L6 application] Native reference harness entry independent of the desktop and Hub product store.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"runtime"
	"strings"
	"syscall"
	"time"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage"
	memorysqlite "github.com/QuincySx/Alva/platform/storage/memory"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/secretinput"
	"github.com/QuincySx/Alva/platform/system/terminaltext"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func main() {
	if handled, err := runWASMShellTrampoline(os.Args[1:]); handled {
		if err != nil {
			fmt.Fprintln(os.Stderr, "private host shell setup failed")
			os.Exit(125)
		}
		return
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	if err := run(ctx, os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run(ctx context.Context, args []string, input io.Reader, output, diagnostics io.Writer) (runErr error) {
	flags := flag.NewFlagSet("alva", flag.ContinueOnError)
	flags.SetOutput(diagnostics)
	demo := flags.Bool("demo", false, "use the deterministic local model/tool fixture (default)")
	live := flags.Bool("live", false, "explicitly opt in to the configured remote model")
	endpoint := flags.String("endpoint", "", "full protocol endpoint; no automatic discovery")
	providerKind := flags.String("provider", string(provider.OpenAIChatKind), "explicit protocol: openai_chat, openai_responses, anthropic or gemini")
	model := flags.String("model", "", "explicit remote model identifier")
	keyStdin := flags.Bool("api-key-stdin", false, "read an optional live API key from stdin (never argv/environment)")
	keyFD := flags.Int("api-key-fd", 0, "optional separate explicit regular/pipe fd3..10 for live API key; stdin remains REPL")
	keyFile := flags.String("api-key-file", "", "explicit existing owner-only regular API key file; portable separate source for REPL")
	repl := flags.Bool("repl", false, "explicit direct harness multi-turn REPL; does not use Hub or EngineRuntime")
	prompt := flags.String("prompt", "", "new user input; live mode requires an explicit prompt")
	dataDir := flags.String("data-dir", "", "explicit private native store directory; no legacy import")
	sessionID := flags.String("session", "", "resume an existing Go native CLI session in --data-dir")
	listSessions := flags.Bool("sessions", false, "list at most 100 owned native CLI sessions without starting an Agent")
	history := flags.Bool("history", false, "read one bounded committed-record metadata page, without restoring runtime")
	search := flags.String("search", "", "literal record-JSON search with --history")
	after := flags.Uint64("after-record", 0, "exclusive committed record cursor for --history")
	highWater := flags.Uint64("high-water", 0, "fixed snapshot high-water for --history")
	workspace := flags.String("workspace", "", "explicit execution workspace (does not itself grant shell/network tools)")
	persistentMemory := flags.Bool("memory", false, "explicit persistent workspace memory Plugin; requires --live --workspace --data-dir")
	permissionMode := flags.String("permission-mode", "ask", "explicit workspace policy: ask (noninteractive writes fail closed), accept_edits, or plan")
	var attachments attachmentFlags
	var sandbox sandboxOptions
	var plugins cliPluginOptions
	var waits cliDurableOptions
	compatConfig := flags.String("compat-config", "", "opt in to a project-relative compatibility JSON file; requires --workspace, native only")
	inspectCompat := flags.String("inspect-compat", "", "inspect selected rule/Skill/Agent sources and fingerprint without constructing an Agent")
	sandbox.flags(flags)
	plugins.flags(flags)
	waits.flags(flags)
	flags.Var(&attachments, "attach", "explicit original file saved as a session reference; requires --data-dir; repeat at most eight times")
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	if flags.NArg() != 0 {
		return errors.New("positional arguments are not supported; use --prompt")
	}
	if *inspectCompat != "" {
		var invalid bool
		flags.Visit(func(f *flag.Flag) {
			if f.Name != "workspace" && f.Name != "inspect-compat" {
				invalid = true
			}
		})
		if invalid || *workspace == "" {
			return errors.New("--inspect-compat accepts only an explicit --workspace")
		}
		return inspectCLICompatibility(ctx, *workspace, *inspectCompat, output)
	}
	if *compatConfig != "" && (*workspace == "" || sandbox.kind != "" || !validCLICompatibilityPath(*compatConfig)) {
		return errors.New("--compat-config requires an explicit workspace and confined project-relative config; whole-agent WASM is unsupported")
	}
	if *demo && *live {
		return errors.New("choose either --demo or --live")
	}
	if *keyFile != "" && (*keyStdin || *keyFD != 0) {
		return errors.New("choose one explicit API key source")
	}
	if runtime.GOOS == "windows" && *keyFD != 0 {
		return errors.New("Unix api-key-fd is unsupported on Windows; use explicit --api-key-file or --api-key-stdin")
	}
	if *keyFD != 0 && (*keyFD < 3 || *keyFD > 10) || *keyStdin && *keyFD != 0 || *repl && (*keyStdin || sandbox.kind != "") || *repl && len(attachments) > 0 && strings.TrimSpace(*prompt) == "" {
		return errors.New("REPL uses native harness input and a separate key fd; WASM cannot share it, initial attachments require an explicit prompt")
	}
	var interactive *directREPL
	if *repl {
		var err error
		interactive, err = newDirectREPL(input, diagnostics)
		if err != nil {
			return err
		}
	}
	if err := sandbox.validate(*workspace, *persistentMemory, *keyStdin); err != nil {
		return err
	}
	mode := security.PermissionMode(*permissionMode)
	if mode != security.Ask && mode != security.AcceptEdits && mode != security.Plan {
		return errors.New("unknown permission mode")
	}
	if *persistentMemory && (!*live || *workspace == "" || *dataDir == "") {
		return errors.New("--memory requires explicit --live, --workspace and --data-dir")
	}
	if mode != security.Ask && ((!*live && sandbox.kind != "wasm") || *workspace == "") {
		return errors.New("non-default permission mode requires --live and --workspace")
	}
	if !*live && (*endpoint != "" || *model != "" || *keyStdin || *keyFD != 0 || *keyFile != "" || *providerKind != string(provider.OpenAIChatKind)) {
		return errors.New("remote settings require explicit --live")
	}
	if *dataDir == "" && (*sessionID != "" || *listSessions || *history || len(attachments) > 0) {
		return errors.New("native session, history and attachments require explicit --data-dir")
	}
	if *search != "" && !*history {
		return errors.New("--search requires --history")
	}
	if *listSessions && *history {
		return errors.New("choose --sessions or --history")
	}

	if err := plugins.validate(*dataDir, *sessionID); err != nil {
		return err
	}
	if err := waits.validate(*dataDir, *sessionID, *prompt, sandbox.kind, *repl, len(attachments)); err != nil {
		return err
	}
	if plugins.cold() && (waits.cold() || waits.enabled || waits.resume) || waits.cold() && (*listSessions || *history) || plugins.cold() && (*listSessions || *history) {
		return errors.New("choose one cold metadata/control operation")
	}
	metadataOnly := *listSessions || *history || plugins.cold() || waits.cold()
	if metadataOnly && (waits.enabled || waits.resume || *live || *demo || *repl || *prompt != "" || *workspace != "" || len(attachments) > 0 || *persistentMemory || mode != security.Ask || sandbox.kind != "" || *compatConfig != "") {
		return errors.New("metadata commands cannot also request execution, attachments or provider access")
	}
	if !*history && (*after != 0 || *highWater != 0) {
		return errors.New("record cursors require --history")
	}
	if *listSessions && *sessionID != "" {
		return errors.New("--sessions does not accept --session")
	}
	var liveConfig provider.Config
	var liveModel abi.LanguageModel
	if *live {
		if *endpoint == "" || *model == "" || *prompt == "" && !*repl && !waits.resume {
			return errors.New("--live requires explicit --endpoint, --model and --prompt, --repl or --resume-wait")
		}
		key := ""
		if *keyStdin || *keyFD != 0 || *keyFile != "" {
			keyInput := input
			if *keyFD != 0 || *keyFile != "" {
				var file *os.File
				if *keyFile != "" {
					var err error
					file, err = secretinput.OpenPrivateFile(*keyFile)
					if err != nil {
						return errors.New("explicit private API key file unavailable")
					}
				} else {
					file = os.NewFile(uintptr(*keyFD), "explicit-direct-cli-key")
				}
				if file == nil {
					return errors.New("key descriptor unavailable")
				}
				defer file.Close()
				info, err := file.Stat()
				if err != nil || !info.Mode().IsRegular() && info.Mode()&os.ModeNamedPipe == 0 {
					return errors.New("key descriptor must be an explicit regular file or pipe")
				}
				keyInput = file
			}
			data, err := secretinput.Read(ctx, keyInput, 8192)
			if err != nil {
				if ctx.Err() != nil {
					return ctx.Err()
				}
				return errors.New("could not read explicit stdin API key")
			}
			if len(data) > 8192 {
				return errors.New("stdin API key exceeds 8 KiB")
			}
			key = strings.TrimSuffix(strings.TrimSuffix(string(data), "\n"), "\r")
		}
		liveConfig = provider.Config{Endpoint: *endpoint, Model: *model, APIKey: key}
		var err error
		liveModel, err = provider.Build(provider.Kind(*providerKind), liveConfig)
		if err != nil {
			return err
		}
	}
	var repository storage.Repository
	var native storage.Session
	var owned *cliNativeRuntime
	defer func() {
		if owned != nil {
			cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			closeErr := owned.Close(cleanup)
			cancel()
			if !owned.CleanupComplete() {
				runErr = errors.Join(runErr, &cliCleanupError{cause: closeErr, runtime: owned, repository: repository})
				return
			}
			if closeErr != nil {
				runErr = errors.Join(runErr, closeErr)
			}
		}
		if repository != nil {
			if closeErr := repository.Close(); closeErr != nil {
				runErr = errors.Join(runErr, closeErr)
			}
		}
	}()
	if *dataDir != "" {
		var err error
		repository, err = session.Open(ctx, *dataDir)
		if err != nil {
			return err
		}
		if *listSessions {
			return printSessions(ctx, repository, output)
		}
		if *history {
			return printHistory(ctx, repository, *sessionID, *search, *after, *highWater, output)
		}
		native, err = openCLISession(ctx, repository, *sessionID)
		if err != nil {
			return err
		}
		fmt.Fprintln(diagnostics, "Native Go session:", native.ID(), "(durable typed records; original artifact bytes retained)")
	}
	if plugins.cold() {
		return controlCLIPlugins(ctx, repository, native, *dataDir, plugins, output)
	}
	if waits.cold() {
		return controlCLIWait(ctx, repository, native, waits, output)
	}
	if sandbox.kind == "wasm" {
		if native != nil {
			if err := validateCLIWorkerSession(ctx, repository, native); err != nil {
				return err
			}
		}
		if *prompt == "" && !*repl {
			*prompt = "List the explicitly granted workspace"
		}
		return runWASM(ctx, sandbox, *workspace, mode, liveModel, *prompt, attachments, repository, native, input, output, diagnostics)
	}
	if *live {
		fmt.Fprintln(diagnostics, "Live mode: explicit endpoint; no legacy configuration or ambient credentials.")
	} else {
		if *prompt == "" && !*repl && !waits.resume {
			*prompt = "Hello Alva"
		}
		fmt.Fprintln(diagnostics, "Local demo fixture; no network/model account. Persistence only with explicit --data-dir.")
	}
	var handler security.ApprovalHandler
	if interactive != nil {
		handler = interactive
	}
	route, _ := json.Marshal(struct {
		Live                      bool
		Provider, Endpoint, Model string
	}{*live, *providerKind, *endpoint, *model})
	config := cliNativeConfig{Model: liveModel, Repository: repository, Native: native, DataDirectory: *dataDir, Workspace: *workspace, Mode: mode, Memory: *persistentMemory, Approval: handler, Durable: waits.enabled || waits.resume, Resume: waits.resume, RouteHash: cliSHA(route), Diagnostics: diagnostics}
	config.CompatibilityFile = *compatConfig
	if *persistentMemory {
		config.OpenMemory = func(ctx context.Context) (storage.MemoryStore, error) { return memorysqlite.Open(ctx, *dataDir) }
		fmt.Fprintln(diagnostics, "Explicit persistent memory enabled for this canonical workspace; no scan/extraction or embedding network starts automatically.")
	}
	var err error
	owned, err = newCLINative(ctx, config)
	if err != nil {
		return err
	}
	runner := owned
	if *live && *workspace != "" {
		fmt.Fprintln(diagnostics, "Explicit workspace file tools enabled; permission mode:", mode, ". Approval is per actual call; shell/network are not granted.")
	}
	if waits.resume {
		return runCLIDurable(ctx, owned, nil, output, diagnostics)
	}
	if len(attachments) > 0 {
		messages, err := attachmentMessages(ctx, repository, native, *prompt, attachments)
		if err != nil {
			return err
		}
		if err := runInput(ctx, runner, messages, output); err != nil {
			return err
		}
		if interactive != nil && !cliSuspended(runner) {
			return interactive.run(ctx, runner, owned.log.ID(), output)
		}
		return nil
	}
	if interactive != nil {
		if strings.TrimSpace(*prompt) != "" {
			if err := runRunner(ctx, runner, *prompt, output); err != nil {
				return err
			}
		}
		if cliSuspended(runner) {
			return nil
		}
		return interactive.run(ctx, runner, owned.log.ID(), output)
	}
	return runRunner(ctx, runner, *prompt, output)
}

func runRunner(ctx context.Context, runner harness.Runner, prompt string, output io.Writer) (runErr error) {
	return runInput(ctx, runner, []wire.Message{wire.TextMessage(wire.RoleUser, prompt)}, output)
}

func runInput(ctx context.Context, runner harness.Runner, messages []wire.Message, output io.Writer) (runErr error) {
	if native, ok := runner.(*cliNativeRuntime); ok && native.config.Durable {
		return runCLIDurable(ctx, native, messages, output, native.config.Diagnostics)
	}
	controller := host.NewController(runner, 256)
	defer func() {
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if closeErr := controller.Close(shutdown); closeErr != nil {
			runErr = errors.Join(runErr, fmt.Errorf("root shutdown did not finish within 5 seconds: %w", closeErr))
		}
	}()
	id, err := controller.Start(ctx, messages)
	if err != nil {
		return err
	}
	return observeRun(ctx, controller, id, output)
}

func observeRun(ctx context.Context, controller *host.Controller, id string, output io.Writer) error {
	var rendered strings.Builder
	var events <-chan host.RecordedEvent
	var unsubscribe func()
	defer func() {
		if unsubscribe != nil {
			unsubscribe()
		}
	}()
	for {
		if err := ctx.Err(); err != nil {
			_ = controller.Stop(id)
			return err
		}
		if events == nil {
			snapshot, err := controller.Snapshot(id)
			if err != nil {
				return err
			}
			tail := ""
			if !strings.HasPrefix(snapshot.Text, rendered.String()) {
				// Streaming is draft evidence. AfterModel may legitimately replace
				// it or remove a tool-preface; never replay or mislabel the draft.
				if err := writeString(output, "\n[Committed response replaces the streamed draft]\n"); err != nil {
					_ = controller.Stop(id)
					return err
				}
				rendered.Reset()
			}
			tail = snapshot.Text[rendered.Len():]
			if err := writeString(output, tail); err != nil {
				_ = controller.Stop(id)
				return err
			}
			rendered.WriteString(tail)
			if snapshot.Status != host.RunRunning {
				if err := writeString(output, "\n"); err != nil {
					return err
				}
				return terminal(snapshot)
			}
			events, unsubscribe, err = controller.Subscribe(id, snapshot.HighWater)
			if errors.Is(err, host.ErrHistoryExpired) {
				events = nil
				continue
			}
			if err != nil {
				return err
			}
		}
		select {
		case <-ctx.Done():
			_ = controller.Stop(id)
			return ctx.Err()
		case event, open := <-events:
			if !open {
				unsubscribe()
				unsubscribe = nil
				events = nil
				continue
			}
			if event.Event.Kind == abi.EventTextDelta {
				if err := writeString(output, event.Event.Text); err != nil {
					_ = controller.Stop(id)
					return err
				}
				rendered.WriteString(event.Event.Text)
			}
			if event.Status != "" {
				unsubscribe()
				unsubscribe = nil
				events = nil
			}
		}
	}
}

func writeString(output io.Writer, text string) error {
	if text == "" {
		return nil
	}
	text = terminaltext.Escape(text)
	n, err := io.WriteString(output, text)
	if err == nil && n != len(text) {
		return io.ErrShortWrite
	}
	return err
}

func terminal(snapshot host.RunSnapshot) error {
	switch snapshot.Status {
	case host.RunCompleted:
		return nil
	case host.RunCancelled:
		return context.Canceled
	case host.RunFailed:
		if snapshot.Error != "" {
			return errors.New(snapshot.Error)
		}
		return errors.New("run failed")
	default:
		return errors.New("run did not reach a terminal state")
	}
}
