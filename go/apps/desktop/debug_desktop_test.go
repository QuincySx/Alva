// INPUT: Actual MyGo Tester/composer callbacks and authenticated diagnostic HTTP transport
// OUTPUT: UI-thread-bound state/action/capture, at-most-once Send and truthful Stop regressions
// POS: [L6 application] Headless native view acceptance never opens an OS window or invokes a model account.
//go:build desktop

package desktop

import (
	"bytes"
	"context"
	"encoding/json"
	"image/png"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/debug"
	"github.com/QuincySx/Alva/go/platform/system/loopback"
	"github.com/egoist/mygo/ui"
)

func TestDiagnosticActionsUseActualNativeComposerAndCallbacks(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	token, _ := loopback.NewToken()
	server, err := debug.New(debug.Config{Token: token, AllowControls: true, AllowExecution: true})
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close(context.Background())
	m := NewModel(true)
	v := NewNativeView(m)
	sends, stops := 0, 0
	v.Send = func() {
		sends++
		prompt, err := m.Prompt()
		if err != nil {
			t.Fatal(err)
		}
		m.Admitted("native-run", prompt)
	}
	v.Stop = func() {
		if m.RequestStop() != "" {
			stops++
		}
	}
	tester := ui.NewTester(v.Build, 900, 720)
	queue := make(chan func(), 1)
	owner, methods, err := v.Diagnostic(ctx, func(fn func()) { queue <- fn }, func() ([]byte, error) {
		var b bytes.Buffer
		err := png.Encode(&b, tester.Image())
		return b.Bytes(), err
	}, nil)
	if err != nil {
		t.Fatal(err)
	}
	v.diagnosticFrame, v.diagnosticChanged = owner.TrackRendering(func() {})
	tester.Frame()
	unregister, err := server.Register("direct", "AlvaDirectWindow", methods, owner)
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
			case err := <-done:
				if err != nil {
					t.Fatal(err)
				}
				return
			case <-ctx.Done():
				t.Fatal("native diagnostic did not join")
			}
		}
	}
	read := func() debug.Snapshot {
		t.Helper()
		var snapshot debug.Snapshot
		pump(func() error {
			raw, err := client.Inspect(ctx, []byte(`{"action":"state","view_id":"direct"}`))
			if err != nil {
				return err
			}
			return json.Unmarshal(raw, &snapshot)
		})
		return snapshot
	}
	action := func(id, method string, args any) debug.ActionResult {
		t.Helper()
		initial := read()
		raw, _ := json.Marshal(args)
		body, _ := json.Marshal(debug.Action{OperationID: id, ViewID: "direct", Revision: initial.Revision, Method: method, Args: raw})
		var result debug.ActionResult
		pump(func() error { var err error; result, err = client.Action(ctx, body); return err })
		return result
	}
	if result := action("draft", "set_draft", map[string]string{"text": "中文 unsent draft"}); result.Status != "applied" || m.Draft != "中文 unsent draft" || sends != 0 {
		t.Fatal(result, m.Draft, sends)
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type(" + native typing")
	if m.Draft != "中文 unsent draft + native typing" {
		t.Fatal("diagnostic draft did not reach the actual native input")
	}
	initial := read()
	raw, _ := json.Marshal(debug.Action{OperationID: "send-once", ViewID: "direct", Revision: initial.Revision, Method: "send", Args: json.RawMessage(`{}`)})
	for range 2 {
		pump(func() error {
			result, err := client.Action(ctx, raw)
			if err == nil && result.Status != "applied" {
				t.Errorf("Send rejected: %v", result)
			}
			return err
		})
	}
	if sends != 1 || m.ActiveRunID != "native-run" {
		t.Fatal("diagnostics bypassed or replayed the normal Send", sends)
	}
	if result := action("disabled-draft", "set_draft", map[string]string{"text": "must not edit active input"}); result.Status != "rejected" || m.Draft != "" {
		t.Fatal("disabled composer was edited", result)
	}
	if result := action("disabled-send", "send", map[string]any{}); result.Status != "rejected" || sends != 1 {
		t.Fatal("disabled Send was invoked", result)
	}
	if result := action("stop", "stop", map[string]any{}); result.Status != "applied" || stops != 1 || m.Status != Stopping {
		t.Fatal("Stop did not wait for acknowledgement", result)
	}
	if result := action("disabled-stop", "stop", map[string]any{}); result.Status != "rejected" || stops != 1 {
		t.Fatal("disabled Stop was invoked", result)
	}
	pump(func() error {
		raw, err := client.Capture(ctx, []byte(`{"view_id":"direct"}`))
		if err != nil {
			return err
		}
		image, err := png.Decode(bytes.NewReader(raw))
		if err == nil && (image.Bounds().Dx() != 900 || image.Bounds().Dy() != 720) {
			t.Error("capture is not actual tester viewport")
		}
		return err
	})
}
