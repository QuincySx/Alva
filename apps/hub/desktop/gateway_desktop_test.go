// INPUT: Headless native controls and an actual embeddable loopback Gateway
// OUTPUT: Explicit start/stop controls retain copyable credentials without invoking the model
// POS: [L7 Hub product] Native Gateway control acceptance does not imply OS visual verification.
//go:build desktop

package hubdesktop

import (
	"context"
	"github.com/QuincySx/Alva/apps/gateway"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"
)

func TestNativeEmbeddedGatewayStartsOnlyOnExplicitConsent(t *testing.T) {
	s, _ := fixture(t)
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); http.Error(w, "unexpected", 500) }))
	defer upstream.Close()
	local, err := gateway.NewLocal(map[string]gateway.Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "explicit"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close(context.Background())
	s.Gateway = local
	session, err := s.Create(context.Background(), "gateway", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(context.Background(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1400, 1400)
	if err = tester.Click("Session settings"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if err = tester.Click("Local model API settings"); err != nil {
		t.Fatal(err)
	}
	if local.Snapshot().Running {
		t.Fatal("pane observation started Gateway")
	}
	if err = tester.Click("Approve model account usage and start local API"); err != nil {
		t.Fatal(err)
	}
	settle := func() {
		t.Helper()
		deadline := time.Now().Add(3 * time.Second)
		for time.Now().Before(deadline) {
			tester.Frame()
			if !p.gatewayBusy {
				return
			}
			time.Sleep(time.Millisecond)
		}
		t.Fatal("gateway action did not settle")
	}
	settle()
	if !local.Snapshot().Running || p.gatewayCredentials == "" || calls.Load() != 0 {
		t.Fatal("explicit start failed or called model", p.problem)
	}
	if err = tester.Click("Stop local model API and cancel active requests"); err != nil {
		t.Fatal(err)
	}
	settle()
	if local.Snapshot().Running || p.gatewayCredentials != "" || calls.Load() != 0 {
		t.Fatal("stop retained listener/secret or invoked model")
	}
}
