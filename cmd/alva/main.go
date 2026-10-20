// INPUT: context, flag, os/signal, app, host, llm/provider, kernel/abi.
// OUTPUT: Explicit demo/live CLI with native resume, bounded history and reference-only original attachments.
// POS: [L6 application] Native reference harness entry independent of the desktop and Hub product store.
package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"time"

	filetools "github.com/QuincySx/Alva/extension/files"
	memoryplugin "github.com/QuincySx/Alva/extension/memory"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/host"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/platform/storage"
	memorysqlite "github.com/QuincySx/Alva/platform/storage/memory"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/secretinput"
	"github.com/QuincySx/Alva/platform/system/terminaltext"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/memory"
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
	sandbox.flags(flags)
	flags.Var(&attachments, "attach", "explicit original file saved as a session reference; requires --data-dir; repeat at most eight times")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 {
		return errors.New("positional arguments are not supported; use --prompt")
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

	metadataOnly := *listSessions || *history
	if metadataOnly && (*live || *demo || *repl || *prompt != "" || *workspace != "" || len(attachments) > 0 || *persistentMemory || mode != security.Ask || sandbox.kind != "") {
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
		if *endpoint == "" || *model == "" || *prompt == "" && !*repl {
			return errors.New("--live requires explicit --endpoint, --model and --prompt or --repl")
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
	if *dataDir != "" {
		var err error
		repository, err = session.Open(ctx, *dataDir)
		if err != nil {
			return err
		}
		defer func() { runErr = errors.Join(runErr, repository.Close()) }()
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
	var log abi.ConversationLog
	if native != nil {
		log = native
	} else {
		log = core.NewMemorySession("cli-volatile")
	}
	if sandbox.kind == "wasm" {
		if *prompt == "" && !*repl {
			*prompt = "List the explicitly granted workspace"
		}
		return runWASM(ctx, sandbox, *workspace, mode, liveModel, *prompt, attachments, repository, native, input, output, diagnostics)
	}
	var builder *harness.Builder
	var err error
	if *live {
		builder = harness.NewBuilder(liveModel, log)
		fmt.Fprintln(diagnostics, "Live mode: explicit endpoint; no legacy configuration or ambient credentials.")
	} else {
		if *prompt == "" && !*repl {
			*prompt = "Hello Alva"
		}
		builder = harness.NewDemoBuilder(log)
		fmt.Fprintln(diagnostics, "Local demo fixture; no network/model account. Persistence only with explicit --data-dir.")
	}
	if err != nil {
		return err
	}
	var plugins []core.Plugin
	if native != nil {
		plugins, err = harness.SessionPlugins(ctx, repository, cliOwner, native.ID())
		if err != nil {
			return err
		}
	}
	if *workspace != "" {
		absolute, err := filepath.Abs(*workspace)
		if err != nil {
			return err
		}
		builder.Workspace(absolute)
		if *live {
			files, err := workspacefs.OpenWorkspace(absolute)
			if err != nil {
				return err
			}
			defer func() { runErr = errors.Join(runErr, files.Close()) }()
			var handler security.ApprovalHandler
			if interactive != nil {
				handler = interactive
			}
			guard, err := security.NewGuard(absolute, nil, files, security.NewPermissionManager(mode, handler))
			if err != nil {
				return err
			}
			var backend agentcontext.ReferenceBackend
			if native != nil {
				backend, err = repository.BoundReferences(ctx, cliOwner, native.ID())
			} else {
				backend, err = agentcontext.NewMemoryReferences(log.ID(), 20<<20)
				if err == nil {
					plugins = append(plugins, harness.NewContextRetrievalPlugin(log.ID(), backend))
				}
			}
			if err != nil {
				return err
			}
			tools := filetools.FileTools(files, guard, backend, log.ID())
			for _, tool := range tools {
				builder.Tool(tool)
			}
			if *persistentMemory {
				store, err := memorysqlite.Open(ctx, *dataDir)
				if err != nil {
					return err
				}
				defer func() { runErr = errors.Join(runErr, store.Close()) }()
				canonical, err := filepath.EvalSymlinks(absolute)
				if err != nil {
					return err
				}
				hash := sha256.Sum256([]byte(canonical))
				memoryBackend, err := store.Bind("cli-workspace-" + hex.EncodeToString(hash[:]))
				if err != nil {
					return err
				}
				plugin, err := memoryplugin.New(memoryplugin.Config{SessionID: log.ID(), Service: memory.NewService(memoryBackend, memory.NoopEmbeddingProvider{}), References: backend, Guard: guard, Workspace: files, Cwd: absolute})
				if err != nil {
					return err
				}
				plugins = append(plugins, plugin)
				tools = append(tools, plugin.Tools()...)
				fmt.Fprintln(diagnostics, "Explicit persistent memory enabled for this canonical workspace; no scan/extraction or embedding network starts automatically.")
			}
			builder.Plugin(harness.NewSecurityPlugin(guard, tools, plugins...))
			fmt.Fprintln(diagnostics, "Explicit workspace file tools enabled; permission mode:", mode, ". REPL asks per actual call; noninteractive Ask writes fail closed. Shell/network are not granted.")
		}
	}
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	runner, err := builder.Build(ctx)
	if err != nil {
		return err
	}
	if len(attachments) > 0 {
		messages, err := attachmentMessages(ctx, repository, native, *prompt, attachments)
		if err != nil {
			return err
		}
		if err := runInput(ctx, runner, messages, output); err != nil {
			return err
		}
		if interactive != nil {
			return interactive.run(ctx, runner, log.ID(), output)
		}
		return nil
	}
	if interactive != nil {
		if strings.TrimSpace(*prompt) != "" {
			if err := runRunner(ctx, runner, *prompt, output); err != nil {
				return err
			}
		}
		return interactive.run(ctx, runner, log.ID(), output)
	}
	return runRunner(ctx, runner, *prompt, output)
}

func runRunner(ctx context.Context, runner harness.Runner, prompt string, output io.Writer) (runErr error) {
	return runInput(ctx, runner, []wire.Message{wire.TextMessage(wire.RoleUser, prompt)}, output)
}

func runInput(ctx context.Context, runner harness.Runner, messages []wire.Message, output io.Writer) (runErr error) {
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
