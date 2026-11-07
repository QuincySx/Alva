// INPUT: Explicit flags or installed bundle profile, private data/CLI configuration directories, cancellable credentials and bootstrap stores
// OUTPUT: Separate desktop-tagged Go native Hub executable
// POS: [L7 Hub product] Product assembly above EngineRuntime; leaves the direct alva-desktop harness entrance independent
//go:build desktop

package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/QuincySx/Alva/apps/debug"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/secretinput"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "ALVA Hub native:", err)
		os.Exit(1)
	}
}
func run() (result error) {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	executable, err := os.Executable()
	if err != nil {
		stop()
		return err
	}
	startup, err := prepareDesktopFromExecutable(ctx, executable, os.Args[1:], os.Stdin, os.Stderr)
	startupErr := ctx.Err()
	// Restore ordinary native event-loop signal behavior after startup instead
	// of silently consuming termination signals throughout the window lifetime.
	stop()
	if err != nil {
		return err
	}
	defer func() {
		shutdown, cancel := context.WithTimeout(context.Background(), 10*time.Second)
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
	return runDesktop(startup.application.Service, startup.diagnostics)
}

type desktopStartup struct {
	application *bootstrap.Application
	diagnostics *debug.Server
	address     string
}

func (s *desktopStartup) close(ctx context.Context) error {
	if s.diagnostics != nil {
		if err := s.diagnostics.Close(ctx); err != nil {
			return err
		}
	}
	return s.application.Close(ctx)
}

// prepareDesktop owns startup only; it cannot open windows or start Agent work.
func prepareDesktop(ctx context.Context, args []string, input io.Reader, diagnostic io.Writer) (*desktopStartup, error) {
	return prepareDesktopProfile(ctx, desktopLaunchProfile{Arguments: args}, input, diagnostic)
}

func prepareDesktopProfile(ctx context.Context, launch desktopLaunchProfile, input io.Reader, diagnostic io.Writer) (*desktopStartup, error) {
	args := launch.Arguments
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	flags := flag.NewFlagSet("alva-hub-desktop", flag.ContinueOnError)
	flags.SetOutput(diagnostic)
	var debugOptions debug.Options
	debugOptions.Bind(flags)
	directory := flags.String("data-dir", defaultDataDirectory(), "required explicit owner-only directory for new Go product/native databases; no Rust data import")
	live := flags.Bool("live", false, "use an explicit model service; no model calls occur before Send")
	demo := flags.Bool("demo", false, "local deterministic demonstration (default)")
	endpoint := flags.String("endpoint", "", "full OpenAI-chat-compatible URL required with --live")
	model := flags.String("model", "", "model ID required with --live")
	kind := flags.String("provider-kind", string(provider.OpenAIChatKind), "explicit protocol: openai_chat, openai_responses, anthropic or gemini")
	keyStdin := flags.Bool("api-key-stdin", false, "read optional API key from a cancellable pipe or regular file, never saved config/environment")
	imageCapable := flags.Bool("image-capable", false, "operator declares this exact live provider/model route supports image input; not inferred, not a media enable switch")
	claudeCommand := flags.String("claude-command", "", "explicit Claude executable; not started until a selected branch is sent")
	codexCommand := flags.String("codex-command", "", "explicit Codex executable; not started until a selected branch is sent")
	codexTransport := flags.String("codex-transport", "app-server", "Codex app-server (default) or explicit exec transport; no protocol fallback")
	codexSandbox := flags.String("codex-sandbox", "workspace-write", "Codex app-server read-only or workspace-write in the saved cwd; network disabled")
	codexApproval := flags.String("codex-approval-policy", "on-request", "Codex app-server on-request, untrusted or explicit never; independent of Alva permissions")
	claudeConfig := flags.String("claude-config-dir", "", "optional absolute existing Claude configuration directory; reuses the complete CLI configuration")
	codexConfig := flags.String("codex-config-dir", "", "optional absolute existing Codex configuration directory; reuses the complete CLI configuration")
	piCommand := flags.String("pi-command", "", "explicit Pi executable; exact private native session resume only")
	piConfig := flags.String("pi-config-dir", "", "optional absolute existing Pi agent configuration directory; reuses the complete CLI configuration")
	piTransport := flags.String("pi-transport", "rpc", "Pi rpc with product tool approvals (default), or explicit read-only exec transport")
	externalApproved := flags.Bool("external-host-approved", false, "acknowledge explicit external executable host/account access; does not authorize automatic fallback")
	gitExecutable := flags.String("git-executable", "", "optional explicit absolute Git for managed Worker worktrees; no discovery or startup execution")
	gitApproved := flags.Bool("git-host-approved", false, "approve this explicit Git host snapshot; each isolation/adoption still requires its own user action")
	if err := flags.Parse(args); err != nil {
		return nil, err
	}
	if err := validateMode(*live); err != nil {
		return nil, err
	}
	if flags.NArg() != 0 || *directory == "" {
		return nil, errors.New("--data-dir is required; positional arguments are not accepted")
	}
	if *live && *demo {
		return nil, errors.New("choose --live or --demo")
	}
	if !*live && (*endpoint != "" || *model != "" || *keyStdin || *imageCapable) {
		return nil, errors.New("provider options require --live")
	}
	if *live && (*endpoint == "" || *model == "") {
		return nil, errors.New("--live requires --endpoint and --model")
	}
	if *claudeConfig != "" && *claudeCommand == "" || *codexConfig != "" && *codexCommand == "" || *piConfig != "" && *piCommand == "" {
		return nil, errors.New("--claude-config-dir/--codex-config-dir/--pi-config-dir require their corresponding --claude-command/--codex-command/--pi-command")
	}
	var codexFlags, codexPolicyFlags, piFlags bool
	flags.Visit(func(f *flag.Flag) {
		switch f.Name {
		case "pi-transport":
			piFlags = true
		case "codex-transport":
			codexFlags = true
		case "codex-sandbox", "codex-approval-policy":
			codexFlags, codexPolicyFlags = true, true
		}
	})
	if piFlags && *piCommand == "" || *piTransport != "rpc" && *piTransport != "exec" {
		return nil, errors.New("Pi transport must be rpc or exec and requires its corresponding --pi-command")
	}
	if codexFlags && *codexCommand == "" {
		return nil, errors.New("Codex transport/policy flags require their corresponding --codex-command")
	}
	if *codexTransport != "app-server" && *codexTransport != "exec" || *codexSandbox != "read-only" && *codexSandbox != "workspace-write" || *codexApproval != "on-request" && *codexApproval != "untrusted" && *codexApproval != "never" {
		return nil, errors.New("invalid explicit Codex transport or native policy")
	}
	if *codexTransport == "exec" && codexPolicyFlags {
		return nil, errors.New("Codex native policy flags require app-server transport")
	}
	external := externalSnapshots(*claudeCommand, *codexCommand, *piCommand, *claudeConfig, *codexConfig, *piConfig, *codexTransport, *codexSandbox, *codexApproval, *piTransport, *externalApproved)
	if err := applyDesktopLaunchEnvironment(external, launch.ExternalEnvironment); err != nil {
		return nil, err
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
	config := provider.Config{Endpoint: *endpoint, Model: *model, APIKey: startupSecrets.APIKey}
	if *live {
		if *keyStdin {
			data, err := secretinput.Read(ctx, input, 8192)
			if err != nil {
				return nil, err
			}
			config.APIKey = strings.TrimRight(string(data), "\r\n")
		}
		// Validate the route without making a network request or discovering credentials.
		if _, err := provider.Build(provider.Kind(*kind), config); err != nil {
			return nil, err
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	diagnostics, err := debugOptions.New(startupSecrets, config.APIKey)
	if err != nil {
		return nil, err
	}
	application, err := bootstrap.Open(ctx, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, DataDirectory: *directory, Demo: !*live, ProviderKind: provider.Kind(*kind), Provider: config, ImageCapable: *imageCapable, External: external, GitExecutable: *gitExecutable, GitHostAccessApproved: *gitApproved})
	if err != nil {
		if application != nil {
			cleanup, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			err = errors.Join(err, application.Close(cleanup))
			cancel()
		}
		if diagnostics != nil {
			err = errors.Join(err, diagnostics.Close(context.Background()))
		}
		return nil, err
	}
	return &desktopStartup{application: application, diagnostics: diagnostics, address: debugOptions.Address}, nil
}

func externalSnapshots(claude, codex, pi, claudeConfig, codexConfig, piConfig, codexTransport, codexSandbox, codexApproval, piTransport string, approved bool) []bootstrap.ExternalEngine {
	var out []bootstrap.ExternalEngine
	for _, entry := range []struct{ vendor, command, configuration string }{{"claude", claude, claudeConfig}, {"codex", codex, codexConfig}, {"pi", pi, piConfig}} {
		if entry.command != "" {
			external := bootstrap.ExternalEngine{Vendor: entry.vendor, Command: entry.command, CLIConfigDirectory: entry.configuration, ApprovedHostAccess: approved}
			if entry.vendor == "pi" {
				external.Transport = piTransport
			}
			if entry.vendor == "codex" {
				external.Transport = codexTransport
				if codexTransport == "app-server" {
					external.CodexSandbox = map[string]string{"read-only": "readOnly", "workspace-write": "workspaceWrite"}[codexSandbox]
					external.CodexApprovalPolicy = map[string]string{"on-request": "onRequest", "untrusted": "unlessTrusted", "never": "never"}[codexApproval]
				}
			}
			out = append(out, external)
		}
	}
	return out
}
