// INPUT: Explicit numeric loopback address and strict stdin route/token/key configuration
// OUTPUT: Independent authenticated model Gateway process with owned HTTP/upstream shutdown
// POS: [L6 application] L6 entrypoint uses canonical provider/Magpie codec; no Hub, account discovery or config-file migration
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

	"github.com/QuincySx/Alva/apps/gateway"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/QuincySx/Alva/platform/system/secretinput"
)

type routeInput struct {
	Kind            provider.Kind `json:"kind"`
	Endpoint        string        `json:"endpoint"`
	Model           string        `json:"model"`
	APIKey          string        `json:"api_key,omitempty"`
	MaxOutputTokens int           `json:"max_output_tokens,omitempty"`
}
type inputConfig struct {
	Token  string                `json:"token"`
	Routes map[string]routeInput `json:"routes"`
}

func main() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	if err := run(ctx, os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "ALVA Gateway:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context, args []string, input io.Reader, output, diagnostic io.Writer) (result error) {
	flags := flag.NewFlagSet("alva-gateway", flag.ContinueOnError)
	flags.SetOutput(diagnostic)
	listen := flags.String("listen", "127.0.0.1:0", "canonical numeric loopback IP:port only; 0 chooses an ephemeral port")
	secrets := flags.Bool("secrets-stdin", false, "required strict JSON {token,routes}; keys/token never enter argv or persistent configuration")
	newToken := flags.Bool("new-token", false, "print a random 256-bit bearer token and exit without networking")
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
	if !*secrets {
		return errors.New("--secrets-stdin required; no credential or routing configuration is discovered")
	}
	host, port, err := net.SplitHostPort(*listen)
	if err != nil {
		return errors.New("numeric loopback IP:port required")
	}
	ip, ipErr := netip.ParseAddr(host)
	n, portErr := strconv.Atoi(port)
	if ipErr != nil || !ip.IsLoopback() || ip.Zone() != "" || portErr != nil || n < 0 || n > 65535 || net.JoinHostPort(ip.String(), strconv.Itoa(n)) != *listen {
		return errors.New("canonical numeric loopback listener required")
	}
	data, err := secretinput.Read(ctx, input, 64<<10)
	if err != nil {
		return err
	}
	var config inputConfig
	if httpjson.ValidateObject(data, &config) != nil {
		return errors.New("stdin must contain strict token/routes JSON with no duplicate or unknown fields")
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&config) != nil || decoder.Decode(new(any)) != io.EOF {
		return errors.New("invalid token/routes configuration")
	}
	if _, err = loopback.New("127.0.0.1:1", config.Token); err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	listener, err := net.Listen("tcp", *listen)
	if err != nil {
		return errors.New("loopback listener unavailable")
	}
	defer listener.Close()
	routes := make(map[string]gateway.Route, len(config.Routes))
	for alias, route := range config.Routes {
		routes[alias] = gateway.Route{Kind: route.Kind, Provider: provider.Config{Endpoint: route.Endpoint, Model: route.Model, APIKey: route.APIKey, MaxOutputTokens: route.MaxOutputTokens}}
	}
	handler, err := gateway.New(gateway.Config{Authority: listener.Addr().String(), Token: config.Token, Routes: routes})
	if err != nil {
		return err
	}
	defer func() {
		cleanup, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		result = errors.Join(result, handler.Close(cleanup))
	}()
	httpServer := &http.Server{Handler: handler, ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 30 * time.Second, IdleTimeout: 30 * time.Second, MaxHeaderBytes: 32 << 10}
	done := make(chan error, 1)
	go func() { done <- httpServer.Serve(listener) }()
	_, reportErr := fmt.Fprintf(output, "ALVA Gateway http://%s (bearer authentication required; %d explicit routes)\n", listener.Addr(), len(routes))
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
	} else {
		result = reportErr
	}
	cleanup, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	ownerErr := handler.Close(cleanup)
	shutdownErr := httpServer.Shutdown(cleanup)
	if shutdownErr != nil {
		shutdownErr = errors.Join(shutdownErr, httpServer.Close())
	}
	if !serveExited {
		select {
		case err = <-done:
			if !errors.Is(err, http.ErrServerClosed) {
				shutdownErr = errors.Join(shutdownErr, err)
			}
		case <-cleanup.Done():
			shutdownErr = errors.Join(shutdownErr, cleanup.Err())
		}
	}
	return errors.Join(result, ownerErr, shutdownErr)
}
