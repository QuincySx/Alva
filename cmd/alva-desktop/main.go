// INPUT:  flags, cancellable explicit stdin credential, app harness, native host/controller, desktop
// OUTPUT: desktop-tagged self-drawn ALVA with optional durable direct sessions and independent diagnostics
// POS: [L6 application] Starts the direct harness application without Hub/engine or ambient configuration discovery.
//go:build desktop

package main

import (
	"context"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"syscall"
	"time"

	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/apps/direct"
	"github.com/QuincySx/Alva/apps/direct/desktop"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/secretinput"
	"github.com/QuincySx/Alva/sdk/agent/core"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "ALVA native:", err)
		os.Exit(1)
	}
}

func run() (result error) {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	startup, err := prepareDesktop(ctx, os.Args[1:], os.Stdin, os.Stderr)
	startupErr := ctx.Err()
	// Signal interception belongs only to cancellable startup. Once the native
	// event loop starts, restore the application's ordinary signal behavior.
	stop()
	if err != nil {
		return err
	}
	defer func() {
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		result = errors.Join(result, startup.close(shutdown))
	}()
	if startupErr != nil {
		return startupErr
	}
	if startup.diagnostics != nil {
		address, err := startup.diagnostics.Start(startup.address)
		if err != nil {
			return err
		}
		fmt.Fprintln(os.Stderr, "ALVA UI diagnostics:", address)
	}
	if startup.service != nil {
		return desktop.RunPersistent(startup.service, startup.diagnostics, startup.initialSession)
	}
	return desktop.RunWithDebug(startup.controller, startup.demo, startup.route, startup.diagnostics)
}

type desktopStartup struct {
	controller     *host.Controller
	service        *direct.Service
	initialSession string
	demo           bool
	route          string
	diagnostics    *debug.Server
	address        string
}

func (s *desktopStartup) close(ctx context.Context) error {
	if s.diagnostics != nil {
		if err := s.diagnostics.Close(ctx); err != nil {
			return err
		}
	}
	if s.service != nil {
		return s.service.Close(ctx)
	}
	return s.controller.Close(ctx)
}

// prepareDesktop is cold: it cannot open a window or submit a model request.
func prepareDesktop(ctx context.Context, args []string, input io.Reader, diagnostic io.Writer) (*desktopStartup, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	flags := flag.NewFlagSet("alva-desktop", flag.ContinueOnError)
	flags.SetOutput(diagnostic)
	var debugOptions debug.Options
	debugOptions.Bind(flags)
	live := flags.Bool("live", false, "use an explicitly configured model service; only Send makes requests")
	demo := flags.Bool("demo", false, "use local deterministic demonstration (the default)")
	endpoint := flags.String("endpoint", "", "explicit full protocol endpoint, required with --live")
	kind := flags.String("provider-kind", string(provider.OpenAIChatKind), "explicit protocol: openai_chat/openai_responses/anthropic/gemini")
	model := flags.String("model", "", "explicit model ID, required with --live")
	dataDirectory := flags.String("data-dir", "", "explicit absolute native direct data directory; enables cold session/history/Inspector and durable resume")
	selected := flags.String("session", "", "select an existing direct native session with --data-dir; never auto-Send")
	workspace := flags.String("workspace", "", "explicit absolute project for newly created durable sessions; live file writes require actual once approvals")
	keyStdin := flags.Bool("api-key-stdin", false, "read an explicit API key from a cancellable pipe or regular file; never environment or saved config")
	if err := flags.Parse(args); err != nil {
		return nil, err
	}
	if flags.NArg() != 0 {
		return nil, fmt.Errorf("unexpected positional arguments")
	}
	if *live && *demo {
		return nil, fmt.Errorf("choose --demo or --live, not both")
	}
	if !*live && (*endpoint != "" || *model != "" || *keyStdin || *kind != string(provider.OpenAIChatKind)) {
		return nil, fmt.Errorf("provider options require --live; demo does not contact a model service")
	}
	if *live && (*endpoint == "" || *model == "") {
		return nil, errors.New("--live requires --endpoint and --model")
	}
	switch provider.Kind(*kind) {
	case provider.OpenAIChatKind, provider.OpenAIResponsesKind, provider.AnthropicKind, provider.GeminiKind:
	default:
		return nil, errors.New("explicit supported provider-kind required")
	}
	if *dataDirectory != "" && !filepath.IsAbs(*dataDirectory) || *workspace != "" && !filepath.IsAbs(*workspace) || *dataDirectory == "" && (*workspace != "" || *selected != "") {
		return nil, errors.New("durable selection/project requires explicit absolute --data-dir and --workspace")
	}
	if *selected != "" {
		decoded, err := hex.DecodeString(*selected)
		if err != nil || len(decoded) != 16 {
			return nil, errors.New("direct --session must be an existing opaque32-hex native ID")
		}
	}
	if err := debugOptions.Validate(*keyStdin); err != nil {
		return nil, err
	}
	startupSecrets, readErr := debugOptions.Read(ctx, input)
	if readErr != nil {
		return nil, readErr
	}
	if !*live && startupSecrets.APIKey != "" {
		return nil, errors.New("api_key requires --live")
	}
	var runner harness.Runner
	var err error
	apiKey := startupSecrets.APIKey
	if *live {
		if *keyStdin {
			data, readErr := secretinput.Read(ctx, input, 8192)
			if readErr != nil {
				return nil, readErr
			}
			apiKey = strings.TrimRight(string(data), "\r\n")
		}
	}
	diagnostics, err := debugOptions.New(startupSecrets, apiKey)
	if err != nil {
		return nil, err
	}
	closeDiagnostic := func() {
		if diagnostics != nil {
			_ = diagnostics.Close(context.Background())
		}
	}
	if *dataDirectory != "" {
		repository, err := session.Open(ctx, *dataDirectory)
		if err != nil {
			closeDiagnostic()
			return nil, err
		}
		service, err := direct.Open(ctx, direct.Config{Storage: repository, DataDirectory: *dataDirectory, Workspace: *workspace, Demo: !*live, ProviderKind: provider.Kind(*kind), Provider: provider.Config{Endpoint: *endpoint, Model: *model, APIKey: apiKey}})
		if err != nil {
			closeDiagnostic()
			return nil, errors.Join(err, repository.Close())
		}
		if *selected != "" {
			if _, err := service.Metadata(ctx, *selected); err != nil {
				closeDiagnostic()
				return nil, errors.Join(err, service.Close(context.Background()))
			}
		}
		if err := ctx.Err(); err != nil {
			closeDiagnostic()
			return nil, errors.Join(err, service.Close(context.Background()))
		}
		return &desktopStartup{service: service, initialSession: *selected, demo: !*live, route: *model, diagnostics: diagnostics, address: debugOptions.Address}, nil
	}
	if *live {
		actualModel, buildErr := provider.Build(provider.Kind(*kind), provider.Config{Endpoint: *endpoint, Model: *model, APIKey: apiKey})
		err = buildErr
		if err == nil {
			runner, err = harness.NewBuilder(actualModel, core.NewMemorySession("direct-live")).Build(ctx)
		}
	} else {
		runner, err = harness.NewDemo(ctx)
	}
	if err != nil {
		closeDiagnostic()
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		closeDiagnostic()
		return nil, err
	}
	return &desktopStartup{controller: host.NewController(runner, 256), demo: !*live, route: *model, diagnostics: diagnostics, address: debugOptions.Address}, nil
}
