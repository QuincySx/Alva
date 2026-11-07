// INPUT: Real durable Hub service, actual native page and authenticated diagnostic HTTP client
// OUTPUT: Actual route/composer/creation actions, independent gates and credential/body omission regressions
// POS: [L7 Hub product] Headless acceptance uses temporary stores and deterministic local engines without external accounts.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/apps/gateway"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/egoist/mygo/ui"
)

func TestDiagnosticHubUsesActualSessionOwnerAndProtectsCredentialPanes(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 8*time.Second)
	defer cancel()
	service, requests := fixture(t)
	page := NewNativePage(service)
	page.ctx = ctx
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	token, _ := loopback.NewToken()
	server, err := debug.New(debug.Config{Token: token, AllowControls: true})
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close(context.Background())
	queue := make(chan func(), 1)
	captures := 0
	owner, methods, err := page.Diagnostic(ctx, func(fn func()) { queue <- fn }, func() ([]byte, error) {
		captures++
		var b bytes.Buffer
		err := png.Encode(&b, tester.Image())
		return b.Bytes(), err
	}, nil)
	if err != nil {
		t.Fatal(err)
	}
	page.diagnosticFrame, page.diagnosticChanged = owner.TrackRendering(func() {})
	tester.Frame()
	unregister, err := server.Register("hub", "AlvaHubWindow", methods, owner)
	if err != nil {
		t.Fatal(err)
	}
	defer unregister()
	address, err := server.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	client, err := debug.NewClient(debug.ClientConfig{Address: address, Token: token, AllowControls: true})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	pump := func(call func() error) {
		t.Helper()
		done := make(chan error, 1)
		go func() { done <- call() }()
		for {
			select {
			case fn := <-queue:
				fn()
				tester.Frame()
			case fn := <-page.updates:
				fn()
				tester.Frame()
			case err := <-done:
				if err != nil {
					t.Fatal(err)
				}
				return
			case <-ctx.Done():
				t.Fatal("Hub diagnostic did not settle")
			}
		}
	}
	read := func() debug.Snapshot {
		t.Helper()
		var snapshot debug.Snapshot
		pump(func() error {
			raw, err := client.Inspect(ctx, []byte(`{"action":"state","view_id":"hub"}`))
			if err != nil {
				return err
			}
			return json.Unmarshal(raw, &snapshot)
		})
		return snapshot
	}
	action := func(id, method string, args any) {
		t.Helper()
		s := read()
		a, _ := json.Marshal(args)
		raw, _ := json.Marshal(debug.Action{OperationID: id, ViewID: "hub", Revision: s.Revision, Method: method, Args: a})
		pump(func() error {
			r, err := client.Action(ctx, raw)
			if err == nil && r.Status != "applied" {
				t.Errorf("actual UI action rejected: %v", r)
			}
			return err
		})
	}
	action("create", "create_session", map[string]string{"title": "Diagnostic local fixture"})
	for page.selected == "" || page.busy || page.sessionLoading || page.polling || page.chat.Busy {
		select {
		case fn := <-page.updates:
			fn()
			tester.Frame()
		case <-ctx.Done():
			t.Fatal("actual product create did not finish")
		}
	}
	if page.problem != "" {
		t.Fatal("actual session create failed")
	}
	action("draft", "set_draft", map[string]string{"text": "Hub unsent fixture"})
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type(" + native typing")
	if page.model.Draft != "Hub unsent fixture + native typing" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("diagnostic draft bypassed actual page or executed")
	}
	sessions, err := service.Store.ListSessions(ctx, 8)
	if err != nil || len(sessions) != 1 || sessions[0].ID != page.selected {
		t.Fatal("diagnostic create did not use durable product", err)
	}
	pump(func() error { _, err := client.Capture(ctx, []byte(`{"view_id":"hub"}`)); return err })
	if captures != 1 {
		t.Fatal("actual viewport callback missing")
	}
	local, err := gateway.NewLocal(map[string]gateway.Route{"fixture": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1", Model: "fixture"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer local.Close(context.Background())
	credentials, err := local.Start(ctx, true)
	if err != nil {
		t.Fatal(err)
	}
	service.Gateway = local
	page.observeGateway()
	page.pane = "gateway"
	page.gatewayGeneration = credentials.Generation
	page.gatewayCredentials = "synthetic-private-ui-value"
	page.problem = "synthetic-private-ui-value"
	page.configDraft = "synthetic-private-ui-value"
	snapshot := read()
	raw, _ := json.Marshal(snapshot)
	if bytes.Contains(raw, []byte("synthetic-private-ui-value")) {
		t.Fatal("credential-capable page state leaked")
	}
	pump(func() error {
		_, err := client.Capture(ctx, []byte(`{"view_id":"hub"}`))
		if err == nil {
			t.Error("one-time credential view was captured")
		}
		return nil
	})
	if err = local.Stop(ctx); err != nil {
		t.Fatal(err)
	}
	page.observeGateway()
	pump(func() error {
		_, err := client.Capture(ctx, []byte(`{"view_id":"hub"}`))
		if !errors.Is(err, debug.ErrFramePending) {
			t.Errorf("cross-window Gateway stop did not invalidate the actual rendered credential frame: %v", err)
		}
		return nil
	})
	if captures != 1 {
		t.Fatal("stale credential frame entered the window capture callback")
	}
	pump(func() error { _, err := client.Capture(ctx, []byte(`{"view_id":"hub"}`)); return err })
	if captures != 2 {
		t.Fatal("rendered stopped Gateway remained blocked by an expired credential")
	}
	page.gatewayCredentials = ""
	page.pane = "settings"
	pump(func() error {
		_, err := client.Capture(ctx, []byte(`{"view_id":"hub"}`))
		if err == nil {
			t.Error("credential editor was captured")
		}
		return nil
	})
	if captures != 2 || service.Runtime.HasActive() {
		t.Fatal("protected capture entered window/engine owner")
	}
	page.pane = ""
	page.chat.Busy = true
	initial := read()
	body, _ := json.Marshal(debug.Action{OperationID: "disabled-composer", ViewID: "hub", Revision: initial.Revision, Method: "set_draft", Args: json.RawMessage(`{"text":"must not edit loading input"}`)})
	pump(func() error {
		result, err := client.Action(ctx, body)
		if err == nil && result.Status != "rejected" {
			t.Error("loading composer was edited")
		}
		return err
	})
	if page.model.Draft != "Hub unsent fixture + native typing" {
		t.Fatal("disabled native input changed")
	}
}

type hubDiagnosticTest struct {
	page   *NativePage
	tester *ui.Tester
	client *debug.Client
	ctx    context.Context
	queue  chan func()
}

func newHubDiagnosticTest(t *testing.T, page *NativePage, controls, execution bool) *hubDiagnosticTest {
	t.Helper()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	t.Cleanup(cancel)
	page.ctx = ctx
	h := &hubDiagnosticTest{page: page, tester: ui.NewTester(workspaceStaticView(page), 1240, 850), ctx: ctx, queue: make(chan func(), 1)}
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	server, err := debug.New(debug.Config{Token: token, AllowControls: controls, AllowExecution: execution})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close(context.Background()) })
	owner, methods, err := page.Diagnostic(ctx, func(fn func()) { h.queue <- fn }, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	unregister, err := server.Register("hub", "AlvaHubWindow", methods, owner)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(unregister)
	address, err := server.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	h.client, err = debug.NewClient(debug.ClientConfig{Address: address, Token: token, AllowControls: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(h.client.Close)
	return h
}

func (h *hubDiagnosticTest) pump(t *testing.T, call func() error) {
	t.Helper()
	done := make(chan error, 1)
	go func() { done <- call() }()
	for {
		select {
		case fn := <-h.queue:
			fn()
			h.tester.Frame()
		case fn := <-h.page.updates:
			fn()
			h.tester.Frame()
		case err := <-done:
			if err != nil {
				t.Fatal(err)
			}
			return
		case <-h.ctx.Done():
			t.Fatal("Hub diagnostic did not settle")
		}
	}
}

func (h *hubDiagnosticTest) read(t *testing.T) debug.Snapshot {
	t.Helper()
	var snapshot debug.Snapshot
	h.pump(t, func() error {
		raw, err := h.client.Inspect(h.ctx, []byte(`{"action":"state","view_id":"hub"}`))
		if err != nil {
			return err
		}
		return json.Unmarshal(raw, &snapshot)
	})
	return snapshot
}

func (h *hubDiagnosticTest) action(t *testing.T, id, method string, args any) (debug.ActionResult, error) {
	t.Helper()
	snapshot := h.read(t)
	encoded, err := json.Marshal(args)
	if err != nil {
		t.Fatal(err)
	}
	body, err := json.Marshal(debug.Action{OperationID: id, ViewID: "hub", Revision: snapshot.Revision, Method: method, Args: encoded})
	if err != nil {
		t.Fatal(err)
	}
	var result debug.ActionResult
	var actionErr error
	h.pump(t, func() error {
		result, actionErr = h.client.Action(h.ctx, body)
		return nil
	})
	return result, actionErr
}

func TestDiagnosticHubSelectsOnlyCurrentRoutesThroughWorkspacePicker(t *testing.T) {
	service, _ := fixture(t)
	other := service.DefaultTarget
	other.EngineID, other.ModelID = "pi", "second-model"
	service.Targets = append(service.Targets, other)
	page := NewNativePage(service)
	h := newHubDiagnosticTest(t, page, true, true)
	apply := func(id, method string, args any, want string) {
		t.Helper()
		result, err := h.action(t, id, method, args)
		if err != nil || result.Status != want {
			t.Fatalf("%s: result=%+v err=%v", method, result, err)
		}
	}
	apply("home-draft", "set_draft", map[string]string{"text": "private unsent request body"}, "applied")
	snapshot := h.read(t)
	routes := snapshot.State["available_routes"].([]any)
	baseKey, otherKey := diagnosticRouteKey(service.DefaultTarget), diagnosticRouteKey(other)
	if len(routes) != 2 || snapshot.State["current_route_key"] != baseKey || snapshot.State["can_send"] != true || snapshot.State["can_set_draft"] != true {
		t.Fatal("home composer or exact routes missing", snapshot.State)
	}
	if route := routes[1].(map[string]any); route["route_key"] != otherKey || route["engine_id"] != other.EngineID || route["model_id"] != other.ModelID || route["available"] != true {
		t.Fatal("published route does not identify the actual configured choice", route)
	}
	raw, _ := json.Marshal(snapshot)
	if bytes.Contains(raw, []byte(page.model.Draft)) {
		t.Fatal("diagnostic snapshot exposed the request body")
	}
	apply("unknown-route", "select_route", map[string]string{"route_key": "unknown"}, "rejected")
	apply("untyped-route", "select_route", map[string]string{"engine_id": other.EngineID}, "rejected")
	if hub.RouteKey(page.workspaceComposerTarget()) != hub.RouteKey(service.DefaultTarget) {
		t.Fatal("invalid route changed the actual composer")
	}
	apply("home-route", "select_route", map[string]string{"route_key": otherKey}, "applied")
	native := NewNativePage(service)
	nativeTester := composerTester(native, 900)
	connectionClick(t, nativeTester, "Agent and model")
	connectionClick(t, nativeTester, "Use pi · second-model")
	if hub.RouteKey(page.workspaceComposerTarget()) != hub.RouteKey(native.workspaceComposerTarget()) || page.composer.targetOpen || page.model.Draft != "private unsent request body" || page.selected != "" {
		t.Fatal("diagnostic selection differs from actual native picker behavior")
	}
	service.Targets = service.Targets[:1]
	apply("removed-route", "select_route", map[string]string{"route_key": otherKey}, "rejected")
	if h.read(t).State["current_route_available"] != false {
		t.Fatal("removed current route remained available")
	}
	service.Targets = append(service.Targets, other)
	session, err := service.Create(t.Context(), "route owner", "")
	if err != nil {
		t.Fatal(err)
	}
	page.selectSession(session.ID)
	waitWorkspace(t, page, h.tester, func() bool {
		return page.detail.Session.ID == session.ID && !page.busy && !page.chat.Busy && !page.polling
	})
	apply("session-route", "select_route", map[string]string{"route_key": otherKey}, "applied")
	waitWorkspace(t, page, h.tester, func() bool {
		return !page.busy && !page.chat.Busy && !page.polling && hub.RouteKey(page.workspaceComposerTarget()) == hub.RouteKey(other)
	})
	detail, err := service.Store.Detail(t.Context(), session.ID)
	branch, branchErr := product.ActiveBranch(detail)
	if err != nil || branchErr != nil || len(detail.Branches) != 2 || hub.RouteKey(branch.Target) != hub.RouteKey(other) || service.Runtime.HasActive() {
		t.Fatal("route selection did not use durable normal Switch", err, branchErr)
	}
	page.model.Draft = "bound session draft"
	for _, state := range []string{"waiting", "running", "companions", "loading"} {
		original := page.detail
		switch state {
		case "waiting":
			page.detail.Session.Status = engine.Waiting
		case "running":
			page.model.ActiveRunID = session.ID
		case "companions":
			page.pane = "companions"
		case "loading":
			page.chat.Busy = true
		}
		apply("blocked-"+state, "select_route", map[string]string{"route_key": baseKey}, "rejected")
		if h.read(t).State["current_route_available"] != false || hub.RouteKey(page.workspaceComposerTarget()) != hub.RouteKey(other) {
			t.Fatal("disabled picker changed route or claimed availability")
		}
		apply("blocked-send-"+state, "send", struct{}{}, "rejected")
		want := "rejected"
		if state == "running" {
			want = "applied"
		}
		apply("draft-"+state, "set_draft", map[string]string{"text": "bound session draft"}, want)
		page.detail, page.model.ActiveRunID, page.pane, page.chat.Busy = original, "", "", false
	}
}

func TestDiagnosticHubRouteKeysOmitAccountAndEndpointValues(t *testing.T) {
	service, _ := fixture(t)
	target := service.DefaultTarget
	target.AccountSource = "diagnostic-private-account-marker"
	target.BillingRoute = "https://diagnostic-private-endpoint-marker.invalid/v1"
	service.DefaultTarget, service.Targets = target, []hub.Target{target}
	page := NewNativePage(service)
	h := newHubDiagnosticTest(t, page, true, false)
	snapshot := h.read(t)
	raw, err := json.Marshal(snapshot)
	if err != nil {
		t.Fatal(err)
	}
	for _, marker := range []string{target.AccountSource, target.BillingRoute, "diagnostic-private-endpoint-marker"} {
		if bytes.Contains(raw, []byte(marker)) {
			t.Fatal("diagnostic route leaked account or endpoint information")
		}
	}
	hash := sha256.Sum256([]byte(hub.RouteKey(target)))
	key := fmt.Sprintf("%x", hash)
	if snapshot.State["current_route_key"] != key || snapshot.State["available_routes"].([]any)[0].(map[string]any)["route_key"] != key {
		t.Fatal("diagnostic route key is not the opaque SHA256 identity")
	}
	if result, err := h.action(t, "opaque-route", "select_route", map[string]string{"route_key": key}); err != nil || result.Status != "applied" {
		t.Fatal("published opaque route key was not accepted", result, err)
	}
	if result, err := h.action(t, "raw-route", "select_route", map[string]string{"route_key": hub.RouteKey(target)}); err != nil || result.Status != "rejected" {
		t.Fatal("raw persisted route key was accepted by diagnostics", result, err)
	}
	if hub.RouteKey(page.workspaceComposerTarget()) != hub.RouteKey(target) {
		t.Fatal("diagnostic identity changed the persisted route")
	}
}

func TestDiagnosticHubLargeRouteCatalogKeepsSnapshotBounded(t *testing.T) {
	service, _ := fixture(t)
	service.Targets = nil
	for i := range 512 {
		target := service.DefaultTarget
		target.ModelID = fmt.Sprintf("catalog-%04d", i)
		service.Targets = append(service.Targets, target)
	}
	h := newHubDiagnosticTest(t, NewNativePage(service), true, false)
	snapshot := h.read(t)
	raw, err := json.Marshal(snapshot)
	if err != nil || len(raw) >= 128<<10 || snapshot.State["routes_truncated"] != true || snapshot.State["route_count"] != float64(512) || len(snapshot.State["available_routes"].([]any)) == 0 {
		t.Fatal("large catalog made diagnostic observation unavailable or hid truncation", err)
	}
}

func TestDiagnosticHubControlsAndExecutionRemainIndependent(t *testing.T) {
	for _, controls := range []bool{false, true} {
		t.Run(map[bool]string{false: "read-only", true: "controls-only"}[controls], func(t *testing.T) {
			service, _ := fixture(t)
			page := NewNativePage(service)
			page.model.Draft = "must not execute"
			h := newHubDiagnosticTest(t, page, controls, false)
			result, err := h.action(t, "route", "select_route", map[string]string{"route_key": diagnosticRouteKey(service.DefaultTarget)})
			if controls && (err != nil || result.Status != "applied") || !controls && err == nil {
				t.Fatal("route action ignored controls gate", result, err)
			}
			if _, err := h.action(t, "execute", "send", struct{}{}); err == nil {
				t.Fatal("Send ignored independent execution gate")
			}
			sessions, err := service.Store.ListSessions(t.Context(), 8)
			if err != nil || len(sessions) != 0 || page.selected != "" || page.model.Draft != "must not execute" || service.Runtime.HasActive() {
				t.Fatal("disabled diagnostics created work or changed draft", err)
			}
		})
	}
}

func TestDiagnosticHubHomeSendUsesActualImportAndAdmission(t *testing.T) {
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := app.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	wrapped := &workspaceDelayedArtifact{Repository: app.Service.Repository, entered: make(chan struct{}), release: make(chan struct{})}
	app.Service.Repository = wrapped
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	page := NewNativePage(app.Service)
	h := newHubDiagnosticTest(t, page, true, true)
	file := filepath.Join(t.TempDir(), "original.txt")
	if err = os.WriteFile(file, []byte("diagnostic first-send original"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.retainWorkspaceFiles("", []string{file})
	if result, err := h.action(t, "draft", "set_draft", map[string]string{"text": "请运行本地演示"}); err != nil || result.Status != "applied" {
		t.Fatal(result, err)
	}
	if result, err := h.action(t, "send", "send", struct{}{}); err != nil || result.Status != "applied" {
		t.Fatal(result, err)
	}
	awaitWorkspaceAdmissionBoundary(t, h.tester, wrapped.entered)
	if snapshot := h.read(t); snapshot.State["can_send"] != false || snapshot.State["current_route_available"] != false {
		t.Fatal("in-flight admission enabled sending or route changes")
	}
	if result, err := h.action(t, "blocked-send", "send", struct{}{}); err != nil || result.Status != "rejected" {
		t.Fatal("in-flight admission allowed a second Send", result, err)
	}
	if result, err := h.action(t, "next-draft", "set_draft", map[string]string{"text": "next native draft"}); err != nil || result.Status != "applied" {
		t.Fatal("busy composer cannot retain the next draft like the visible editor", result, err)
	}
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, page, h.tester, func() bool {
		return !page.busy && !page.polling && !app.Runtime.HasActive() && page.model.Status == chat.Completed
	})
	branch, err := product.ActiveBranch(page.detail)
	if err != nil {
		t.Fatal(err)
	}
	files, err := app.Repository.PageArtifacts(t.Context(), page.selected, branch.NativeSessionID, "input-attachment", "", 0, 0, 8)
	if err != nil || len(files.Entries) != 1 || page.workspacePendingFiles() {
		t.Fatal("home Send skipped normal import", err)
	}
	sessions, err := app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 1 || page.model.Draft != "next native draft" || strings.TrimSpace(page.problem) != "" {
		t.Fatal("diagnostic first Send lost lifecycle or later input", err)
	}
}
