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
	tester := ui.NewTester(workspaceStaticView(p), 1400, 1400)
	if err = workspaceTestMenu(tester, "会话设置"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if err = tester.Click("Gateway settings"); err != nil {
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

func TestNativeGatewayCredentialsExpireAcrossObserversAndClosedOwnerCannotRestart(t *testing.T) {
	s, _ := fixture(t)
	local, err := gateway.NewLocal(map[string]gateway.Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1", Model: "explicit"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close(context.Background())
	s.Gateway = local
	first, other := NewNativePage(s), NewNativePage(s)
	firstTester := ui.NewTester(first.buildGateway, 1000, 500)
	otherTester := ui.NewTester(other.buildGateway, 1000, 500)
	settle := func(p *NativePage, tester *ui.Tester) {
		t.Helper()
		deadline := time.Now().Add(3 * time.Second)
		for time.Now().Before(deadline) {
			select {
			case update := <-p.updates:
				update()
			default:
			}
			tester.Frame()
			if !p.gatewayBusy {
				return
			}
			time.Sleep(time.Millisecond)
		}
		t.Fatal("Gateway action did not return", p.problem)
	}
	if err = firstTester.Click("Approve model account usage and start local API"); err != nil {
		t.Fatal(err)
	}
	settle(first, firstTester)
	initialGeneration := first.gatewayGeneration
	if first.gatewayCredentials == "" || !firstTester.HasText("Local API credentials") {
		t.Fatal("explicit start did not expose this listener's one-time credentials")
	}
	other.poll()
	otherTester.Frame()
	if err = otherTester.Click("Stop local model API and cancel active requests"); err != nil {
		t.Fatal(err)
	}
	settle(other, otherTester)
	first.poll()
	firstTester.Frame()
	if first.gatewayCredentials != "" || first.gatewayGeneration != 0 || firstTester.HasText("Local API credentials") {
		t.Fatal("cross-window Stop retained an expired bearer")
	}
	if err = otherTester.Click("Approve model account usage and start local API"); err != nil {
		t.Fatal(err)
	}
	settle(other, otherTester)
	first.poll()
	firstTester.Frame()
	if other.gatewayGeneration <= initialGeneration || other.gatewayCredentials == "" || firstTester.HasText("Local API credentials") {
		t.Fatal("restart did not isolate one-time credentials to its initiating window")
	}
	if err = local.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	other.poll()
	otherTester.Frame()
	if other.gatewayCredentials != "" || !otherTester.HasText("本机模型 API 已关闭") || otherTester.HasText("Approve model account usage and start local API") {
		t.Fatal("closed owner retained credentials or advertised a Start it rejects")
	}
}

func TestNativeGatewayLateStartCallbackCannotPublishPreviousListenerCredentials(t *testing.T) {
	s, _ := fixture(t)
	local, err := gateway.NewLocal(map[string]gateway.Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1", Model: "explicit"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close(context.Background())
	s.Gateway = local
	p := NewNativePage(s)
	p.gatewayAction(true, 0)
	var admitted func()
	select {
	case admitted = <-p.updates:
	case <-time.After(3 * time.Second):
		t.Fatal("Start did not return to its observer")
	}
	previous := local.Snapshot().Generation
	if err = local.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	next, err := local.Start(t.Context(), true)
	if err != nil || next.Generation <= previous {
		t.Fatal("replacement listener did not start", err)
	}
	admitted()
	if p.gatewayBusy || p.gatewayCredentials != "" || p.gatewayGeneration != 0 {
		t.Fatal("late Start callback bound an old bearer to the replacement listener")
	}
}

func TestNativeGatewayRenderedStopCannotCancelReplacementListener(t *testing.T) {
	s, _ := fixture(t)
	local, err := gateway.NewLocal(map[string]gateway.Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1", Model: "explicit"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close(context.Background())
	s.Gateway = local
	previous, err := local.Start(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(p.buildGateway, 1000, 500)
	if err = local.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	next, err := local.Start(t.Context(), true)
	if err != nil || next.Generation <= previous.Generation {
		t.Fatal("explicit replacement did not start", err)
	}
	// Dispatch the Stop closure from the prior rendered controls before this
	// observer receives another frame or poll of the replacement generation.
	if err = tester.Click("Stop local model API and cancel active requests"); err != nil {
		t.Fatal(err)
	}
	select {
	case update := <-p.updates:
		update()
	case <-time.After(3 * time.Second):
		t.Fatal("rendered Stop did not settle")
	}
	if status := local.Snapshot(); !status.Running || status.Stopping || status.Generation != next.Generation || p.problem == "" {
		t.Fatal("stale rendered Stop canceled the replacement listener", status, p.problem)
	}
}

func TestNativeGatewayPressedStopCannotFollowPolledReplacement(t *testing.T) {
	s, _ := fixture(t)
	local, err := gateway.NewLocal(map[string]gateway.Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1", Model: "explicit"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close(context.Background())
	s.Gateway = local
	previous, err := local.Start(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(p.buildGateway, 1000, 500)
	button, found := tester.Find("Stop local model API and cancel active requests")
	if !found {
		t.Fatal("missing actual Stop control")
	}
	x, y := button.X+button.W/2, button.Y+button.H/2
	tester.Press(x, y)
	if p.gatewayBusy {
		t.Fatal("Stop dispatched before pointer release")
	}
	if err = local.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	next, err := local.Start(t.Context(), true)
	if err != nil || next.Generation <= previous.Generation {
		t.Fatal("replacement listener did not start", err)
	}
	// The normal observer clock may update the page between pointer-down and
	// pointer-up. The new listener must have a different control identity.
	p.poll()
	tester.Release(x, y)
	if p.gatewayBusy {
		select {
		case update := <-p.updates:
			update()
		case <-time.After(3 * time.Second):
			t.Fatal("pressed Stop did not settle")
		}
	}
	if status := local.Snapshot(); !status.Running || status.Stopping || status.Generation != next.Generation {
		t.Fatal("pressed old Stop canceled the polled replacement listener", status, p.problem)
	}
	// A new explicit gesture can stop the newly observed generation normally.
	if err = tester.Click("Stop local model API and cancel active requests"); err != nil {
		t.Fatal(err)
	}
	select {
	case update := <-p.updates:
		update()
	case <-time.After(3 * time.Second):
		t.Fatal("fresh Stop did not settle")
	}
	if local.Snapshot().Running || p.problem != "" {
		t.Fatal("new listener identity disabled a fresh explicit Stop", p.problem)
	}
}
