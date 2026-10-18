// INPUT: Explicit desktop flags and cancellable exact startup-secret JSON from the current stdin
// OUTPUT: Options, StartupSecrets and cold diagnostic Server construction without a listener
// POS: [L6 diagnostics] Shared startup boundary never discovers, prints, persists or places credentials in argv.
package debug

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"io"
	"net/netip"

	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/QuincySx/Alva/platform/system/secretinput"
)

type Options struct {
	Address                           string
	Controls, Execution, SecretsStdin bool
}
type StartupSecrets struct {
	APIKey     string `json:"api_key,omitempty"`
	DebugToken string `json:"debug_token,omitempty"`
}

func (o *Options) Bind(flags *flag.FlagSet) {
	flags.StringVar(&o.Address, "debug-listen", "", "explicit numeric-loopback UI diagnostic listener, e.g. 127.0.0.1:0; disabled by default")
	flags.BoolVar(&o.Controls, "debug-controls", false, "operator enables declared UI mutations, log-level changes and diagnostic-only shutdown")
	flags.BoolVar(&o.Execution, "debug-execution", false, "operator additionally permits declared Send actions; normal product billing and tool approvals still apply")
	flags.BoolVar(&o.SecretsStdin, "startup-secrets-stdin", false, "read exact JSON {api_key?,debug_token?} from a bounded cancellable stdin pipe/file; conflicts with --api-key-stdin")
}
func (o Options) Validate(keyStdin bool) error {
	if o.SecretsStdin && keyStdin {
		return errors.New("choose raw API key stdin or exact startup secret JSON")
	}
	if o.Execution && !o.Controls {
		return errors.New("--debug-execution also requires --debug-controls")
	}
	if o.Address == "" {
		if o.Controls || o.Execution {
			return errors.New("diagnostic controls require an explicit listener")
		}
		return nil
	}
	address, err := netip.ParseAddrPort(o.Address)
	if err != nil || !address.Addr().IsLoopback() || address.Addr().Zone() != "" || address.String() != o.Address {
		return errors.New("canonical numeric loopback diagnostic address required")
	}
	if !o.SecretsStdin {
		return errors.New("--debug-listen requires independent debug_token via --startup-secrets-stdin")
	}
	return nil
}
func (o Options) Read(ctx context.Context, input io.Reader) (StartupSecrets, error) {
	if !o.SecretsStdin {
		return StartupSecrets{}, nil
	}
	raw, err := secretinput.Read(ctx, input, 32<<10)
	if err != nil {
		return StartupSecrets{}, err
	}
	var value StartupSecrets
	if err := httpjson.ValidateObject(raw, &value); err != nil {
		return StartupSecrets{}, errors.New("startup secrets require exact unique api_key/debug_token fields")
	}
	if err := json.Unmarshal(raw, &value); err != nil || len(value.APIKey) > 8192 {
		return StartupSecrets{}, errors.New("invalid bounded startup secret strings")
	}
	if o.Address == "" && value.DebugToken != "" {
		return StartupSecrets{}, errors.New("debug_token requires an explicit diagnostic listener")
	}
	if o.Address != "" {
		if _, err := loopback.New("127.0.0.1:1", value.DebugToken); err != nil {
			return StartupSecrets{}, errors.New("independent 256-bit diagnostic token required")
		}
	}
	return value, nil
}
func (o Options) New(value StartupSecrets, apiKey string) (*Server, error) {
	if o.Address == "" {
		return nil, nil
	}
	return New(Config{Token: value.DebugToken, Secrets: []string{apiKey}, AllowControls: o.Controls, AllowExecution: o.Execution})
}
