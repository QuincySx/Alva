// INPUT: Real authenticated loopback HTTP, registered local UI owner, bounded log and actual encoded PNG fixtures
// OUTPUT: Access, control opt-in, at-most-once operations, private logging, capture and transport-only shutdown acceptance
// POS: [L6 diagnostics] No model account, host screenshot command, user credentials or Agent runtime is used.
package debug

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"image"
	"image/color"
	"image/png"
	"io"
	"log/slog"
	"net"
	"net/http"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func debugHTTP(t *testing.T, s *Server, address, method, path, body string) (int, []byte) {
	t.Helper()
	request, err := http.NewRequestWithContext(t.Context(), method, address+path, strings.NewReader(body))
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Authorization", "Bearer "+s.config.Token)
	if body != "" {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := http.DefaultClient.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	raw, err := io.ReadAll(io.LimitReader(response.Body, 5<<20))
	if err != nil {
		t.Fatal(err)
	}
	return response.StatusCode, raw
}

func TestActualClientReportsPendingFrameWithoutReturningOldPixels(t *testing.T) {
	s := fixtureServer(t, false, false)
	captures := 0
	v, _ := NewView(t.Context(), func(fn func()) { fn() }, func() Snapshot { return fixtureSnapshot(0) }, nil, func() ([]byte, error) { captures++; return nil, nil })
	v.TrackRendering(func() {})
	unregister, err := s.Register("window", "Fixture", nil, v)
	if err != nil {
		t.Fatal(err)
	}
	defer unregister()
	address, err := s.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	client, err := NewClient(ClientConfig{Address: address, Token: s.config.Token})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	if _, err := client.Capture(t.Context(), []byte(`{"view_id":"window"}`)); !errors.Is(err, ErrFramePending) || captures != 0 {
		t.Fatal("HTTP/client hid stale-frame evidence or returned old pixels", err)
	}
}

func TestDiagnosticCloseJoinsPartialHeaderConnections(t *testing.T) {
	s := fixtureServer(t, false, false)
	address, err := s.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	connection, err := net.Dial("tcp", strings.TrimPrefix(address, "http://"))
	if err != nil {
		t.Fatal(err)
	}
	defer connection.Close()
	if _, err := io.WriteString(connection, "GET /v1/debug/health HTTP/1.1\r\nHost: "); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), time.Second)
	defer cancel()
	if err := s.Close(ctx); err != nil {
		t.Fatal("partial header survived diagnostic Close", err)
	}
	connection.SetReadDeadline(time.Now().Add(time.Second))
	if _, err := connection.Read(make([]byte, 1)); err == nil {
		t.Fatal("partial-header socket remains open")
	}
}

func TestLogBindingPreservesOriginalGroupAndSanitizesSecretKeys(t *testing.T) {
	s := fixtureServer(t, false, false)
	secret := "configured-key-fixture"
	s.logs.secrets = append(s.logs.secrets, secret)
	logger := s.Logger().With("before", 1).WithGroup("nested").With("bound", 2)
	logger.Info("group fixture", "later", 3)
	s.Logger().WithGroup(secret).Info("private group fixture", secret, "value")
	page, err := s.logs.page(LogQuery{})
	if err != nil || len(page.Records) != 2 {
		t.Fatal(err)
	}
	var data struct {
		Attributes map[string]any `json:"attributes"`
	}
	if err := json.Unmarshal(page.Records[0].Data, &data); err != nil {
		t.Fatal(err)
	}
	nested, _ := data.Attributes["nested"].(map[string]any)
	if data.Attributes["before"] != float64(1) || nested["bound"] != float64(2) || nested["later"] != float64(3) {
		t.Fatal("slog WithGroup moved prior attributes", data)
	}
	if bytes.Contains(page.Records[1].Data, []byte(secret)) {
		t.Fatal("configured secret leaked through log key/group")
	}
	s.Close(t.Context())
	s.Logger().Info("after diagnostic close")
	again, _ := s.logs.page(LogQuery{})
	if len(again.Records) != 2 {
		t.Fatal("closed diagnostic logger retained UI events")
	}
}

func TestHTTPAccessRealCaptureAndAtMostOnceAction(t *testing.T) {
	s := fixtureServer(t, true, false)
	var count atomic.Int32
	var encoded bytes.Buffer
	bitmap := image.NewRGBA(image.Rect(0, 0, 32, 16))
	bitmap.Set(3, 2, color.RGBA{R: 255, A: 255})
	if err := png.Encode(&encoded, bitmap); err != nil {
		t.Fatal(err)
	}
	v, _ := NewView(t.Context(), func(fn func()) { fn() }, func() Snapshot { return fixtureSnapshot(int(count.Load())) }, func(string, json.RawMessage) error { count.Add(1); return nil }, func() ([]byte, error) { return encoded.Bytes(), nil })
	remove, err := s.Register("window", "Fixture", []Method{{Name: "increment", Effect: "ui"}}, v)
	if err != nil {
		t.Fatal(err)
	}
	defer remove()
	address, err := s.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"missing-token", "foreign-origin", "foreign-host"} {
		request, _ := http.NewRequestWithContext(t.Context(), "GET", address+"/v1/debug/views", nil)
		request.Header.Set("Authorization", "Bearer "+s.config.Token)
		if kind == "missing-token" {
			request.Header.Del("Authorization")
		}
		if kind == "foreign-origin" {
			request.Header.Set("Origin", "https://untrusted.invalid")
		}
		if kind == "foreign-host" {
			request.Host = "untrusted.invalid"
		}
		response, err := http.DefaultClient.Do(request)
		if err != nil {
			t.Fatal(err)
		}
		response.Body.Close()
		if response.StatusCode != 401 && response.StatusCode != 403 {
			t.Fatal("access gate accepted", kind, response.StatusCode)
		}
	}
	status, raw := debugHTTP(t, s, address, "GET", "/v1/debug/state?view=window", "")
	if status != 200 {
		t.Fatal(status, string(raw))
	}
	var snapshot Snapshot
	if err := json.Unmarshal(raw, &snapshot); err != nil {
		t.Fatal(err)
	}
	action := Action{OperationID: "http-once", ViewID: "window", Revision: snapshot.Revision, Method: "increment", Args: json.RawMessage(`{}`)}
	body, _ := json.Marshal(action)
	for range 2 {
		status, raw = debugHTTP(t, s, address, "POST", "/v1/debug/actions", string(body))
		if status != 200 || !bytes.Contains(raw, []byte(`"status":"applied"`)) {
			t.Fatal(status, string(raw))
		}
	}
	if count.Load() != 1 {
		t.Fatal("HTTP retry replayed the action")
	}
	status, raw = debugHTTP(t, s, address, "POST", "/v1/debug/capture", `{"view_id":"window"}`)
	if status != 200 {
		t.Fatal(status, string(raw))
	}
	decoded, err := png.Decode(bytes.NewReader(raw))
	if err != nil || decoded.Bounds() != bitmap.Bounds() {
		t.Fatal("actual encoded owner capture was lost", err)
	}
	for _, body := range []string{`{"level":"debug","level":"error"}`, `{"level":"debug","unexpected":true}`} {
		status, _ := debugHTTP(t, s, address, "PUT", "/v1/debug/level", body)
		if status != 400 {
			t.Fatal("non-exact control JSON accepted", status)
		}
	}
}

func TestDefaultReadOnlyAndBoundedPrivateLogs(t *testing.T) {
	s := fixtureServer(t, false, false)
	s.logs.secrets = append(s.logs.secrets, "configured-fixture-value")
	logger := s.Logger()
	logger.Debug("filtered debug")
	logger.Info("configured-fixture-value", "api_key", "other-private-value")
	logger.Info(strings.Repeat("x", 8190) + "configured-fixture-value")
	page, err := s.logs.page(LogQuery{Limit: 8})
	if err != nil || len(page.Records) != 2 {
		t.Fatal(page, err)
	}
	raw, _ := json.Marshal(page)
	if bytes.Contains(raw, []byte("configured-fixture-value")) || bytes.Contains(raw, []byte("other-private-value")) {
		t.Fatal("configured or labelled credential retained")
	}
	if err := s.logs.setLevel("debug"); err != nil {
		t.Fatal(err)
	}
	logger.LogAttrs(t.Context(), slog.LevelDebug, "visible after level change", slog.Int("count", 1))
	for range 520 {
		logger.Info("bounded fixture")
	}
	page, err = s.logs.page(LogQuery{Limit: 8})
	if err != nil || len(s.logs.records) != 512 || !page.Dropped || !page.HasMore {
		t.Fatal("ring/cursor loss was not explicit", page, err)
	}
	address, err := s.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	status, _ := debugHTTP(t, s, address, "PUT", "/v1/debug/level", `{"level":"error"}`)
	if status != 403 {
		t.Fatal("read-only server accepted controls", status)
	}
	status, _ = debugHTTP(t, s, address, "POST", "/v1/debug/shutdown", `{}`)
	if status != 403 {
		t.Fatal("read-only server accepted shutdown", status)
	}
}
