// INPUT: Explicit diagnostic startup flags, finite/cancellable secret inputs and canonical loopback configuration
// OUTPUT: Cold opt-in, exact secret JSON, separate execution grant and no ignored credential regressions
// POS: [L6 diagnostics] Startup validation never opens a listener, UI window, database or model connection.
package debug

import (
	"context"
	"errors"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/loopback"
)

func TestStartupRequiresExactIndependentTokenAndExplicitControlGrant(t *testing.T) {
	token, _ := loopback.NewToken()
	options := Options{Address: "127.0.0.1:0", SecretsStdin: true}
	if err := options.Validate(false); err != nil {
		t.Fatal(err)
	}
	secrets, err := options.Read(t.Context(), strings.NewReader(`{"debug_token":"`+token+`"}`))
	if err != nil {
		t.Fatal(err)
	}
	server, err := options.New(secrets, "")
	if err != nil || server.http != nil {
		t.Fatal("cold startup opened a listener", err)
	}
	server.Close(t.Context())
	for _, bad := range []Options{{Address: "0.0.0.0:0", SecretsStdin: true}, {Address: "127.0.0.1:0"}, {Controls: true}, {Address: "127.0.0.1:0", SecretsStdin: true, Execution: true}} {
		if err := bad.Validate(false); err == nil {
			t.Fatal("implicit/nonloopback authority accepted")
		}
	}
	if err := options.Validate(true); err == nil {
		t.Fatal("two stdin secret formats were accepted")
	}
	for _, raw := range []string{`{"debug_token":"` + token + `","debug_token":"` + token + `"}`, `{"debug_token":"` + token + `","unknown":true}`, `{"debug_token":"short"}`, `{"debug_token":null}`} {
		if _, err := options.Read(t.Context(), strings.NewReader(raw)); err == nil {
			t.Fatal("invalid private startup JSON accepted")
		}
	}
}

func TestStartupSecretJSONPendingPipeCancelsWithoutEOF(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	done := make(chan error, 1)
	go func() { _, err := (Options{Address: "127.0.0.1:0", SecretsStdin: true}).Read(ctx, reader); done <- err }()
	time.AfterFunc(20*time.Millisecond, cancel)
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		writer.Close()
		<-done
		t.Fatal("diagnostic startup waited for producer EOF")
	}
	if _, err := reader.Stat(); err != nil {
		t.Fatal("borrowed startup input was closed", err)
	}
}
