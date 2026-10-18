// INPUT: Explicit numeric loopback Hub/UI URL, separate token fd/private file and owned stdio streams
// OUTPUT: Default-read-only Devtools MCP with explicit UI controls and generic private-error handling
// POS: [L7 entrypoint] Thin diagnostic transport owner with no Agent execution or configuration discovery.
package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"runtime"
	"strings"
	"syscall"

	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/apps/hub/devtools"
	"github.com/QuincySx/Alva/platform/system/secretinput"
)

func main() {
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	if err := run(ctx, os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil && !errors.Is(err, context.Canceled) {
		fmt.Fprintln(os.Stderr, "ALVA devtools: configuration or MCP session failed")
		os.Exit(1)
	}
}
func run(ctx context.Context, args []string, input io.ReadCloser, output io.WriteCloser, diagnostic io.Writer) error {
	flags := flag.NewFlagSet("alva-devtools-mcp", flag.ContinueOnError)
	flags.SetOutput(diagnostic)
	address := flags.String("hub", "", "explicit canonical numeric loopback Hub HTTP address")
	uiAddress := flags.String("ui", "", "explicit independent native UI diagnostic HTTP address; mutually exclusive with --hub")
	uiControls := flags.Bool("ui-controls", false, "operator enables declared UI control tools; the actual UI server still enforces controls/execution policy")
	fd := flags.Int("token-fd", 0, "explicit read-only pipe/file fd3..10 containing only the Hub bearer token; stdin remains MCP")
	tokenFile := flags.String("token-file", "", "explicit existing owner-only regular token file (portable alternative to Unix fd3); never discovered")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 || (*fd != 0 && (*fd < 3 || *fd > 10)) || (*fd == 0) == (*tokenFile == "") || (*address == "") == (*uiAddress == "") || *uiControls && *uiAddress == "" {
		return errors.New("choose one explicit Hub/UI address and one token fd3..10 or private token file")
	}
	if runtime.GOOS == "windows" && *fd != 0 {
		return errors.New("Unix token-fd is unsupported on Windows; use explicit --token-file")
	}
	var file *os.File
	var err error
	if *tokenFile != "" {
		file, err = secretinput.OpenPrivateFile(*tokenFile)
		if err != nil {
			return errors.New("explicit private token file unavailable")
		}
	} else {
		file = os.NewFile(uintptr(*fd), "explicit-devtools-token")
	}
	if file == nil {
		return errors.New("token descriptor unavailable")
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() && info.Mode()&os.ModeNamedPipe == 0 {
		return errors.New("token descriptor must be an explicit regular file or pipe")
	}
	raw, err := secretinput.Read(ctx, file, 4096)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("token descriptor could not be read")
	}
	if *uiAddress != "" {
		client, err := debug.NewClient(debug.ClientConfig{Address: *uiAddress, Token: strings.TrimSpace(string(raw)), AllowControls: *uiControls})
		if err != nil {
			return errors.New("explicit UI diagnostic address/token invalid")
		}
		return client.Serve(ctx, input, output)
	}
	client, err := devtools.New(devtools.Config{Address: *address, Token: strings.TrimSpace(string(raw))})
	if err != nil {
		return errors.New("explicit Hub address/token invalid")
	}
	return client.Serve(ctx, input, output)
}
