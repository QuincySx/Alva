// INPUT: Real durable Hub service, actual native page and authenticated diagnostic HTTP client
// OUTPUT: Cold session creation/draft/capture and credential-pane refusal through the actual UI owner
// POS: [L7 Hub product] Headless native acceptance creates only temporary local facts and never runs an engine.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"encoding/json"
	"image/png"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/egoist/mygo/ui"
)

func TestDiagnosticHubUsesActualSessionOwnerAndProtectsCredentialPanes(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 8*time.Second)
	defer cancel()
	service, requests := fixture(t)
	page := NewNativePage(service)
	page.ctx = ctx
	tester := ui.NewTester(page.Build, 1240, 850)
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
	page.gatewayCredentials = ""
	page.pane = "settings"
	pump(func() error {
		_, err := client.Capture(ctx, []byte(`{"view_id":"hub"}`))
		if err == nil {
			t.Error("credential editor was captured")
		}
		return nil
	})
	if captures != 1 || service.Runtime.HasActive() {
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
