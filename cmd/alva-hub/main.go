// INPUT: Explicit Hub CLI arguments, separately granted API-key fd, owned stdin and process signals
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
	title := flags.String("title", "CLI session", "title for an explicitly created session")
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
	pi := flags.String("pi-command", "", "explicit Pi executable")
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
	for _, v := range []struct{ vendor, command string }{{"claude", *claude}, {"codex", *codex}, {"pi", *pi}} {
		if v.command != "" {
			config.External = append(config.External, bootstrap.ExternalEngine{Vendor: v.vendor, Command: v.command, ApprovedHostAccess: *approved})
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
		page, err := app.Store.PageSessions(ctx, "", hub.SessionCursor{}, 64)
		if err != nil {
			return err
		}
		return writeJSON(ctx, output, page)
	}
	if *history {
		page, err := app.Store.History(ctx, *selected, *after, 128)
		if err != nil {
			return err
		}
		return writeJSON(ctx, output, page)
	}
	if *handoff != "" {
		value, err := app.Store.ReadHandoff(ctx, *selected, *handoff)
		if err != nil {
			return err
		}
		return writeJSON(ctx, output, value)
	}
	if c.session != "" {
		detail, err := app.Store.Detail(ctx, c.session)
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
		created, err := app.Service.Create(ctx, *title, *workspace)
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
			created, err := app.Service.Create(ctx, *title, *workspace)
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
			_, err := c.app.Store.Switch(ctx, c.session, target, "Explicit Hub CLI engine selection")
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
		_, err := fmt.Fprintln(c.diagnostic, "/session /sessions /engine alva|claude|codex|pi /history [after] /quit; prompts execute only on explicit input")
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
		page, err := c.app.Store.PageSessions(ctx, "", hub.SessionCursor{}, 64)
		if err != nil {
			return err
		}
		return writeJSON(ctx, c.diagnostic, page)
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
		page, err := c.app.Store.History(ctx, c.session, after, 128)
		if err != nil {
			return err
		}
		return writeJSON(ctx, c.diagnostic, page)
	default:
		return errors.New("unknown Hub command; see /help")
	}
}
