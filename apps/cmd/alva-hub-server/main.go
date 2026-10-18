// INPUT: Explicit loopback listener/data directory, stdin secrets and optional approved native engine routes
// OUTPUT: Authenticated headless Hub HTTP/SSE process using bootstrap.Open
// POS: [L7 Hub product] Server resource owner; no React assets, credential discovery or duplicate business assembly
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/netip"
	"os"
	"os/signal"
	"strconv"
	"syscall"
	"time"

	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/server"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/QuincySx/Alva/platform/system/secretinput"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func main() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	if err := run(ctx, os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "ALVA Hub server:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context, args []string, input io.Reader, output, diagnostic io.Writer) (result error) {
	flags := flag.NewFlagSet("alva-hub-server", flag.ContinueOnError)
	flags.SetOutput(diagnostic)
	directory := flags.String("data-dir", "", "explicit owner-only directory for new Go databases; no Rust import")
	listen := flags.String("listen", "127.0.0.1:0", "numeric loopback IP:port only; 0 chooses an ephemeral port")
	newToken := flags.Bool("new-token", false, "print a new 256-bit base64url token and exit without opening any application")
	secretsStdin := flags.Bool("secrets-stdin", false, "required: read strict JSON {token,api_key?} from stdin; never save or print these secrets")
	live := flags.Bool("live", false, "use an explicitly configured model route; only Send starts it")
	demo := flags.Bool("demo", false, "local deterministic fixture (default)")
	endpoint := flags.String("endpoint", "", "full protocol endpoint required with --live")
	model := flags.String("model", "", "model ID required with --live")
	kind := flags.String("provider-kind", string(provider.OpenAIChatKind), "openai_chat, openai_responses, anthropic or gemini")
	imageCapable := flags.Bool("image-capable", false, "operator declaration: exactly this configured live provider/model supports images; never inferred")
	claude := flags.String("claude-command", "", "explicit native Claude executable, started only on Send")
	codex := flags.String("codex-command", "", "explicit native Codex executable, started only on Send")
	pi := flags.String("pi-command", "", "explicit native Pi executable, started only on Send")
	approved := flags.Bool("external-host-approved", false, "acknowledge selected executables' host/account access, not automatic fallback")
	gitExecutable := flags.String("git-executable", "", "optional explicit absolute Git executable for managed Worker worktrees")
	gitApproved := flags.Bool("git-host-approved", false, "approve the explicit Git host snapshot; provision/adopt still need independent consent")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 {
		return errors.New("positional arguments are not accepted")
	}
	if *newToken {
		if len(args) != 1 {
			return errors.New("--new-token cannot be combined with server options")
		}
		token, err := loopback.NewToken()
		if err != nil {
			return err
		}
		_, err = fmt.Fprintln(output, token)
		return err
	}
	if *directory == "" || !*secretsStdin {
		return errors.New("--data-dir and --secrets-stdin are required")
	}
	if *live && *demo {
		return errors.New("choose --live or --demo")
	}
	if !*live && (*endpoint != "" || *model != "" || *imageCapable) {
		return errors.New("provider options require --live")
	}
	if *live && (*endpoint == "" || *model == "") {
		return errors.New("--live requires --endpoint and --model")
	}
	host, port, err := net.SplitHostPort(*listen)
	if err != nil {
		return errors.New("numeric loopback IP:port required")
	}
	ip, err := netip.ParseAddr(host)
	n, portErr := strconv.Atoi(port)
	if err != nil || !ip.IsLoopback() || ip.Zone() != "" || portErr != nil || n < 0 || n > 65535 || net.JoinHostPort(ip.String(), strconv.Itoa(n)) != *listen {
		return errors.New("canonical numeric loopback IP:port required")
	}
	data, err := secretinput.Read(ctx, input, 16<<10)
	if err != nil {
		return err
	}
	if !wire.ValidJSON(data) {
		return errors.New("stdin secrets must be a bounded valid JSON object")
	}
	secrets, err := parseSecrets(data)
	if err != nil {
		return err
	}
	if !*live && secrets.APIKey != "" {
		return errors.New("api_key requires --live")
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	listener, err := net.Listen("tcp", *listen)
	if err != nil {
		return errors.New("loopback listener could not be opened")
	}
	defer listener.Close()
	transportConfig := server.Config{Authority: listener.Addr().String(), Token: secrets.Token}
	if err = transportConfig.Validate(); err != nil {
		return err
	}
	config := bootstrap.Config{DataDirectory: *directory, Demo: !*live, ProviderKind: provider.Kind(*kind), Provider: provider.Config{Endpoint: *endpoint, Model: *model, APIKey: secrets.APIKey}, ImageCapable: *imageCapable, GitExecutable: *gitExecutable, GitHostAccessApproved: *gitApproved}
	for _, entry := range []struct{ vendor, command string }{{"claude", *claude}, {"codex", *codex}, {"pi", *pi}} {
		if entry.command != "" {
			config.External = append(config.External, bootstrap.ExternalEngine{Vendor: entry.vendor, Command: entry.command, ApprovedHostAccess: *approved})
		}
	}
	application, err := bootstrap.Open(ctx, config)
	if err != nil {
		return err
	}
	defer func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		result = errors.Join(result, application.Close(cleanup))
	}()
	handler, err := server.New(application.Service, transportConfig)
	if err != nil {
		return err
	}
	defer handler.Close()
	if application.Service.Automations != nil {
		if err = application.Service.Automations.Start(); err != nil {
			return err
		}
	}
	httpServer := &http.Server{Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 30 * time.Second, MaxHeaderBytes: 32 << 10}
	done := make(chan error, 1)
	go func() { done <- httpServer.Serve(listener) }()
	// Report only the non-secret route; caller already owns the stdin token.
	_, reportErr := fmt.Fprintf(output, "ALVA Hub server http://%s (bearer authentication required; demo=%t)\n", listener.Addr(), !*live)
	serveExited := false
	if reportErr == nil {
		select {
		case err = <-done:
			serveExited = true
			if !errors.Is(err, http.ErrServerClosed) {
				result = err
			}
		case <-ctx.Done():
		}
	}
	_ = handler.Close() // End SSE observers before graceful HTTP drain; no Turn Stop here.
	shutdown, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	shutdownErr := httpServer.Shutdown(shutdown)
	if shutdownErr != nil {
		shutdownErr = errors.Join(shutdownErr, httpServer.Close())
	}
	if !serveExited {
		select {
		case serveErr := <-done:
			if !errors.Is(serveErr, http.ErrServerClosed) {
				shutdownErr = errors.Join(shutdownErr, serveErr)
			}
		case <-shutdown.Done():
			shutdownErr = errors.Join(shutdownErr, shutdown.Err())
		}
	}
	return errors.Join(result, reportErr, shutdownErr)
}

type secretValues struct{ Token, APIKey string }

func parseSecrets(data []byte) (secretValues, error) {
	invalid := errors.New("stdin secrets must be an exact object with unique token and optional bounded api_key strings")
	d := json.NewDecoder(bytes.NewReader(data))
	first, err := d.Token()
	if err != nil || first != json.Delim('{') {
		return secretValues{}, invalid
	}
	seen := map[string]bool{}
	var value secretValues
	for d.More() {
		raw, err := d.Token()
		if err != nil {
			return secretValues{}, invalid
		}
		key, ok := raw.(string)
		if !ok || seen[key] || (key != "token" && key != "api_key") {
			return secretValues{}, invalid
		}
		seen[key] = true
		raw, err = d.Token()
		text, ok := raw.(string)
		if err != nil || !ok || len(text) > 8192 {
			return secretValues{}, invalid
		}
		if key == "token" {
			value.Token = text
		} else {
			value.APIKey = text
		}
	}
	if final, err := d.Token(); err != nil || final != json.Delim('}') || !seen["token"] {
		return secretValues{}, invalid
	}
	if d.Decode(new(any)) != io.EOF {
		return secretValues{}, invalid
	}
	return value, nil
}
