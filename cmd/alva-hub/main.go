// INPUT: Explicit Hub CLI/profile arguments, separately granted API-key fd, owned stdin and process signals
// OUTPUT: Independent alva-hub one-shot/REPL, cold diagnostics and durable resume/manual handoff
// POS: [L7 CLI entry] Thin shared bootstrap/product consumer; direct alva entry remains engine-free.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/secretinput"
	"github.com/QuincySx/Alva/platform/system/terminaltext"
)

func main() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	if err := run(ctx, os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "ALVA Hub CLI:", terminaltext.Escape(err.Error()))
		code := 1
		if errors.Is(err, context.Canceled) {
			code = 130
		}
		if errors.Is(err, context.DeadlineExceeded) {
			code = 124
		}
		os.Exit(code)
	}
}

type cli struct {
	app                *bootstrap.Application
	input              *commandInput
	output, diagnostic io.Writer
	session            string
	timeout            time.Duration
	ask                bool
}

func run(ctx context.Context, args []string, input io.ReadCloser, output, diagnostic io.Writer) (result error) {
	if input == nil || output == nil || diagnostic == nil {
		return errors.New("owned input and output/diagnostic streams required")
	}
	flags := flag.NewFlagSet("alva-hub", flag.ContinueOnError)
	flags.SetOutput(diagnostic)
	directory := flags.String("data-dir", "", "required explicit private Go product/native directory; no Rust import")
	selected := flags.String("session", "", "resume this actual durable product session; never a native ID")
	workspace := flags.String("workspace", "", "explicit workspace for a new session; resume may not retarget cwd")
	title := flags.String("title", "", "title for an explicitly created session (default: first line of the first prompt)")
	prompt := flags.String("prompt", "", "new prompt; one-shot or initial REPL turn")
	repl := flags.Bool("repl", false, "explicit multi-turn REPL; commands /session /sessions /engine /history /quit")
	engineID := flags.String("engine", "", "explicit configured alva/claude/codex/pi route; resume keeps its branch unless provided")
	list := flags.Bool("sessions", false, "cold list of at most64 durable product sessions; no creation/execution")
	history := flags.Bool("history", false, "cold bounded history page for --session")
	after := flags.Uint64("after", 0, "exclusive history sequence cursor")
	handoff := flags.String("handoff", "", "cold read of an exact saved handoff in --session")
	live := flags.Bool("live", false, "explicit model service; no call before prompt")
	demo := flags.Bool("demo", false, "local deterministic fixture (default)")
	endpoint := flags.String("endpoint", "", "explicit full model endpoint with --live")
	model := flags.String("model", "", "explicit model ID with --live")
	kind := flags.String("provider-kind", string(provider.OpenAIChatKind), "openai_chat, openai_responses, anthropic or gemini")
	keyFD := flags.Int("api-key-fd", 0, "separate explicitly granted regular/pipe fd3..10; stdin remains REPL")
	keyFile := flags.String("api-key-file", "", "explicit existing owner-only regular API key file; portable alternative to Unix fd3")
	permission := flags.String("permission", "deny", "deny (default), or ask with --repl; no automatic allow/bypass")
	timeout := flags.Duration("timeout", 30*time.Minute, "per-Turn deadline1s..30m; cancellation joins actual execution")
	claude := flags.String("claude-command", "", "explicit Claude executable")
	codex := flags.String("codex-command", "", "explicit Codex executable")
	codexTransport := flags.String("codex-transport", "app-server", "Codex app-server (default) or explicit exec transport; no protocol fallback")
	codexSandbox := flags.String("codex-sandbox", "workspace-write", "Codex app-server read-only or workspace-write in the saved cwd; network disabled")
	codexApproval := flags.String("codex-approval-policy", "on-request", "Codex app-server on-request, untrusted or explicit never; independent of Alva permissions")
	claudeConfig := flags.String("claude-config-dir", "", "optional absolute existing Claude configuration directory; reuses the complete CLI configuration")
	codexConfig := flags.String("codex-config-dir", "", "optional absolute existing Codex configuration directory; reuses the complete CLI configuration")
	pi := flags.String("pi-command", "", "explicit Pi executable")
	piConfig := flags.String("pi-config-dir", "", "optional absolute existing Pi agent configuration directory; reuses the complete CLI configuration")
	piTransport := flags.String("pi-transport", "rpc", "Pi rpc with product tool approvals (default), or explicit read-only exec transport")
	approved := flags.Bool("external-host-approved", false, "approve explicitly selected native executable/account access; not fallback or bypass")
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	cold := 0
	for _, v := range []bool{*list, *history, *handoff != ""} {
		if v {
			cold++
		}
	}
	if flags.NArg() != 0 || *directory == "" || cold > 1 || *timeout < time.Second || *timeout > 30*time.Minute || *live && *demo || *permission != "deny" && *permission != "ask" || *permission == "ask" && !*repl {
		return errors.New("invalid explicit Hub invocation; see --help")
	}
	if cold > 0 && (*repl || *prompt != "" || *workspace != "" || *engineID != "") {
		return errors.New("cold commands cannot also execute, create or switch a session")
	}
	if (*history || *handoff != "") && *selected == "" || cold == 0 && !*repl && strings.TrimSpace(*prompt) == "" {
		return errors.New("provide a prompt/--repl or an explicit cold command with required --session")
	}
	if !*history && *after != 0 {
		return errors.New("--after requires --history")
	}
	if !*live && (*endpoint != "" || *model != "" || *keyFD != 0 || *keyFile != "") || *live && (*endpoint == "" || *model == "") {
		return errors.New("provider endpoint/model/key require explicit --live")
	}
	if *keyFile != "" && *keyFD != 0 {
		return errors.New("choose one explicit API key source")
	}
	if runtime.GOOS == "windows" && *keyFD != 0 {
		return errors.New("Unix api-key-fd is unsupported on Windows; use explicit --api-key-file")
	}
	if *engineID != "" && *engineID != "alva" && *engineID != "claude" && *engineID != "codex" && *engineID != "pi" {
		return errors.New("unknown configured engine")
	}
	if *claudeConfig != "" && *claude == "" || *codexConfig != "" && *codex == "" || *piConfig != "" && *pi == "" {
		return errors.New("--claude-config-dir/--codex-config-dir/--pi-config-dir require their corresponding --claude-command/--codex-command/--pi-command")
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
	if piFlags && *pi == "" || *piTransport != "rpc" && *piTransport != "exec" {
		return errors.New("Pi transport must be rpc or exec and requires its corresponding --pi-command")
	}
	if codexFlags && *codex == "" {
		return errors.New("Codex transport/policy flags require their corresponding --codex-command")
	}
	if *codexTransport != "app-server" && *codexTransport != "exec" || *codexSandbox != "read-only" && *codexSandbox != "workspace-write" || *codexApproval != "on-request" && *codexApproval != "untrusted" && *codexApproval != "never" {
		return errors.New("invalid explicit Codex transport or native policy")
	}
	if *codexTransport == "exec" && codexPolicyFlags {
		return errors.New("Codex native policy flags require app-server transport")
	}
	if len(*prompt) > 32<<10 || !utf8.ValidString(*prompt) {
		return errors.New("prompt exceeds32KiB or is not UTF-8")
	}
	config := bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, DataDirectory: *directory, Demo: !*live, ProviderKind: provider.Kind(*kind), Provider: provider.Config{Endpoint: *endpoint, Model: *model}}
	if *keyFD != 0 || *keyFile != "" {
		if *keyFD != 0 && (*keyFD < 3 || *keyFD > 10) {
			return errors.New("explicit API key fd must be3..10")
		}
		var file *os.File
		if *keyFile != "" {
			var err error
			file, err = secretinput.OpenPrivateFile(*keyFile)
			if err != nil {
				return errors.New("explicit private API key file unavailable")
			}
		} else {
			file = os.NewFile(uintptr(*keyFD), "explicit-hub-cli-api-key")
		}
		if file == nil {
			return errors.New("key descriptor unavailable")
		}
		defer file.Close()
		info, err := file.Stat()
		if err != nil || !info.Mode().IsRegular() && info.Mode()&os.ModeNamedPipe == 0 {
			return errors.New("key descriptor must be an explicit regular file or pipe")
		}
		key, err := secretinput.Read(ctx, file, 4096)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return errors.New("explicit key descriptor read failed")
		}
		config.Provider.APIKey = strings.TrimSpace(string(key))
	}
	for _, v := range []struct{ vendor, command, configuration string }{{"claude", *claude, *claudeConfig}, {"codex", *codex, *codexConfig}, {"pi", *pi, *piConfig}} {
		if v.command != "" {
			external := bootstrap.ExternalEngine{Vendor: v.vendor, Command: v.command, CLIConfigDirectory: v.configuration, ApprovedHostAccess: *approved}
			if v.vendor == "claude" {
				// Permission prompts reach the approval card over stream-json.
				external.Transport = "stream-json"
			}
			if v.vendor == "pi" {
				external.Transport = *piTransport
			}
			if v.vendor == "codex" {
				external.Transport = *codexTransport
				if *codexTransport == "app-server" {
					external.CodexSandbox = map[string]string{"read-only": "readOnly", "workspace-write": "workspaceWrite"}[*codexSandbox]
					external.CodexApprovalPolicy = map[string]string{"on-request": "onRequest", "untrusted": "unlessTrusted", "never": "never"}[*codexApproval]
				}
			}
			config.External = append(config.External, external)
		}
	}
	if ctx.Err() != nil {
		return ctx.Err()
	}
	app, err := bootstrap.Open(ctx, config)
	if app != nil {
		defer func() {
			cleanup, cancel := context.WithTimeout(context.Background(), 10*time.Second)
			defer cancel()
			result = errors.Join(result, app.Close(cleanup))
		}()
	}
	if err != nil {
		// A backend may surface its own interrupted operation while opening.
		// Keep the owner's cancellation identity without interpreting backend errors.
		if ctx.Err() != nil {
			return errors.Join(err, ctx.Err())
		}
		return err
	}
	c := &cli{app: app, input: newInput(input), output: output, diagnostic: diagnostic, session: *selected, timeout: *timeout, ask: *permission == "ask"}
	defer c.input.close()
	if *engineID != "" {
		found := false
		for _, target := range app.Targets {
			found = found || target.EngineID == *engineID
		}
		if !found {
			return errors.New("selected engine requires its explicit configured executable and host approval")
		}
	}
	if *list {
		page, err := app.Service.Conversations.Page(ctx, "", hub.SessionCursor{}, 64)
		if err != nil {
			return err
		}
		return writeJSON(ctx, output, page)
	}
	if *history {
		page, err := app.Service.Conversations.History(ctx, *selected, *after, 128)
		if err != nil {
			return err
		}
		return writeJSON(ctx, output, page)
	}
	if *handoff != "" {
		value, err := app.Service.Conversations.Handoff(ctx, *selected, *handoff)
		if err != nil {
			return err
		}
		return writeJSON(ctx, output, value)
	}
	if c.session != "" {
		detail, err := app.Service.Conversations.Detail(ctx, c.session)
		if err != nil {
			return err
		}
		if *workspace != "" {
			canonical, err := filepath.EvalSymlinks(*workspace)
			if err != nil || canonical != detail.Session.Workspace {
				return errors.New("resume workspace must equal the saved cwd; cannot retarget native history")
			}
		}
	} else if strings.TrimSpace(*prompt) != "" {
		created, err := createCLISession(ctx, app, product.SessionTitle(*title, *prompt), *workspace)
		if err != nil {
			return err
		}
		c.session = created.ID
	}
	if *engineID != "" && c.session != "" {
		if err := c.switchEngine(ctx, *engineID); err != nil {
			return err
		}
	}
	if c.session != "" {
		if _, err := fmt.Fprintln(diagnostic, "Hub session:", c.session); err != nil {
			return err
		}
	}
	if strings.TrimSpace(*prompt) != "" {
		if err := c.turn(ctx, *prompt); err != nil {
			return err
		}
	}
	if !*repl {
		return nil
	}
	for {
		if _, err := fmt.Fprint(diagnostic, "alva-hub> "); err != nil {
			return err
		}
		line, err := c.input.line(ctx)
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
		line = strings.TrimSpace(line)
		if line == "" {
			continue
		}
		if line == "/quit" || line == "/exit" || line == "/q" {
			return nil
		}
		if strings.HasPrefix(line, "/") {
			if err := c.command(ctx, line); err != nil {
				return err
			}
			continue
		}
		if c.session == "" {
			created, err := createCLISession(ctx, app, product.SessionTitle(*title, line), *workspace)
			if err != nil {
				return err
			}
			c.session = created.ID
			if *engineID != "" {
				if err := c.switchEngine(ctx, *engineID); err != nil {
					return err
				}
			}
			if _, err := fmt.Fprintln(diagnostic, "Hub session:", c.session); err != nil {
				return err
			}
		}
		if err := c.turn(ctx, line); err != nil {
			return err
		}
	}
}

func (c *cli) switchEngine(ctx context.Context, name string) error {
	for _, target := range c.app.Targets {
		if target.EngineID == name {
			_, err := c.app.Service.SwitchTarget(ctx, c.session, target, "Explicit Hub CLI engine selection")
			return err
		}
	}
	return errors.New("engine not present in the explicit configured executable/provider routes")
}
func (c *cli) command(ctx context.Context, line string) error {
	parts := strings.Fields(line)
	switch parts[0] {
	case "/help":
		if len(parts) != 1 {
			return errors.New("/help takes no arguments")
		}
		_, err := fmt.Fprintln(c.diagnostic, "/session /sessions /engine alva|claude|codex|pi /history [after] /rename <title> /pin /unpin /delete <session-id> /quit; prompts execute only on explicit input")
		return err
	case "/session":
		if len(parts) != 1 {
			return errors.New("/session takes no arguments")
		}
		_, err := fmt.Fprintln(c.diagnostic, "Hub session:", c.session)
		return err
	case "/sessions":
		if len(parts) != 1 {
			return errors.New("/sessions takes no arguments")
		}
		page, err := c.app.Service.Conversations.Page(ctx, "", hub.SessionCursor{}, 64)
		if err != nil {
			return err
		}
		return writeJSON(ctx, c.diagnostic, page)
	case "/rename":
		title := strings.TrimSpace(strings.TrimPrefix(line, "/rename"))
		if title == "" || c.session == "" {
			return errors.New("/rename requires a title and an existing session")
		}
		return c.app.Service.Conversations.Rename(ctx, c.session, title)
	case "/pin", "/unpin":
		if len(parts) != 1 || c.session == "" {
			return errors.New(parts[0] + " takes no arguments and needs an existing session")
		}
		return c.app.Service.Conversations.SetPinned(ctx, c.session, parts[0] == "/pin")
	case "/delete":
		// Soft delete names the session explicitly so a typo cannot hide the
		// current one; a running session is refused by the product.
		if len(parts) != 2 {
			return errors.New("/delete requires exactly one explicit session id")
		}
		if err := c.app.Service.Conversations.Delete(ctx, parts[1]); err != nil {
			return err
		}
		if parts[1] == c.session {
			c.session = ""
		}
		_, err := fmt.Fprintln(c.diagnostic, "Deleted (hidden; data kept):", parts[1])
		return err
	case "/engine":
		if len(parts) != 2 || c.session == "" {
			return errors.New("/engine requires one configured engine and an existing session")
		}
		return c.switchEngine(ctx, parts[1])
	case "/history":
		if len(parts) > 2 || c.session == "" {
			return errors.New("/history requires a session and optional sequence")
		}
		var after uint64
		if len(parts) == 2 {
			value, err := strconv.ParseUint(parts[1], 10, 64)
			if err != nil || strconv.FormatUint(value, 10) != parts[1] {
				return errors.New("invalid history sequence")
			}
			after = value
		}
		page, err := c.app.Service.Conversations.History(ctx, c.session, after, 128)
		if err != nil {
			return err
		}
		return writeJSON(ctx, c.diagnostic, page)
	default:
		return errors.New("unknown Hub command; see /help")
	}
}

// createCLISession creates a session for this process and turns durable waits
// off for it. The product default (on) parks a Turn at its first human
// decision and ends the run so it can survive a restart; this CLI answers
// approvals inline in the same process and reads the final answer when the run
// ends, so a parked Waiting run would end the one-shot with no answer. Desktop
// and server sessions keep the product default.
func createCLISession(ctx context.Context, app *bootstrap.Application, title, workspace string) (hub.Session, error) {
	created, err := app.Service.Create(ctx, title, workspace)
	if err != nil {
		return created, err
	}
	if err = app.Service.EnableDurableWaits(ctx, created.ID, false); err != nil && !errors.Is(err, engine.ErrUnsupported) {
		return created, err
	}
	return created, nil
}
