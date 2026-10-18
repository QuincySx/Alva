// INPUT: Isolated stdin secrets, local listener and empty private Go product directories
// OUTPUT: Token-only/no-start validation and real server shutdown ownership regressions
// POS: [L7 Hub product] Entry acceptance without user credentials, external engines, model requests or browser UI
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/platform/system/loopback"
)

func TestNewTokenHasNoApplicationSideEffects(t *testing.T) {
	var output, diagnostic bytes.Buffer
	if err := run(context.Background(), []string{"--new-token"}, strings.NewReader(""), &output, &diagnostic); err != nil {
		t.Fatal(err)
	}
	token := strings.TrimSpace(output.String())
	decoded, err := base64.RawURLEncoding.DecodeString(token)
	if err != nil || len(decoded) != 32 || diagnostic.Len() != 0 {
		t.Fatal("invalid random token output")
	}
	if err = run(context.Background(), []string{"--new-token", "--demo"}, strings.NewReader(""), io.Discard, io.Discard); err == nil {
		t.Fatal("mixed token/server options")
	}
}

func TestBadInputDoesNotCreateStores(t *testing.T) {
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		args   []string
		secret string
	}{
		{nil, `{}`},
		{[]string{"--secrets-stdin"}, `{"token":"weak"}`},
		{[]string{"--secrets-stdin", "--listen", "0.0.0.0:0"}, `{"token":"` + token + `"}`},
		{[]string{"--secrets-stdin", "--listen", "localhost:0"}, `{"token":"` + token + `"}`},
		{[]string{"--secrets-stdin"}, `{"token":"` + token + `","unexpected":true}`},
		{[]string{"--secrets-stdin"}, `{"token":"` + token + `","api_key":"DO-NOT-ECHO"}`},
		{[]string{"--secrets-stdin"}, `{"token":"` + token + `","token":"` + token + `"}`},
		{[]string{"--secrets-stdin"}, `{"Token":"` + token + `"}`},
		{[]string{"--secrets-stdin"}, `{"token":"` + token + `","api_key":null}`},
		{[]string{"--secrets-stdin", "--image-capable"}, `{"token":"` + token + `"}`},
	} {
		dir := filepath.Join(t.TempDir(), "not-created")
		args := append([]string{"--data-dir", dir}, test.args...)
		var output, diagnostic bytes.Buffer
		err = run(context.Background(), args, strings.NewReader(test.secret), &output, &diagnostic)
		if err == nil {
			t.Fatal("invalid input accepted")
		}
		if strings.Contains(err.Error(), token) || strings.Contains(err.Error(), "DO-NOT-ECHO") || strings.Contains(output.String(), token) || strings.Contains(diagnostic.String(), token) {
			t.Fatal("secrets leaked")
		}
		if _, statErr := os.Stat(dir); !os.IsNotExist(statErr) {
			t.Fatal("bad input created application storage", statErr)
		}
	}
}

type startupWriter struct {
	mu    sync.Mutex
	data  bytes.Buffer
	ready chan string
}

func (w *startupWriter) Write(p []byte) (int, error) {
	w.mu.Lock()
	defer w.mu.Unlock()
	n, err := w.data.Write(p)
	select {
	case w.ready <- string(p):
	default:
	}
	return n, err
}
func (w *startupWriter) String() string { w.mu.Lock(); defer w.mu.Unlock(); return w.data.String() }

func TestRealServerStopsAndReleasesApplicationLocks(t *testing.T) {
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	secrets, _ := json.Marshal(map[string]string{"token": token})
	dir := filepath.Join(t.TempDir(), "product")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	output := &startupWriter{ready: make(chan string, 1)}
	done := make(chan error, 1)
	go func() {
		done <- run(ctx, []string{"--data-dir", dir, "--secrets-stdin"}, bytes.NewReader(secrets), output, io.Discard)
	}()
	var line string
	select {
	case line = <-output.ready:
	case err = <-done:
		t.Fatalf("early exit: %v", err)
	case <-time.After(5 * time.Second):
		t.Fatal("server did not become ready")
	}
	fields := strings.Fields(line)
	if len(fields) < 4 || fields[0] != "ALVA" || fields[1] != "Hub" {
		t.Fatal("invalid startup report")
	}
	url := fields[3]
	request, err := http.NewRequest(http.MethodGet, url+"/v1/health", nil)
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Authorization", "Bearer "+token)
	client := &http.Client{Timeout: time.Second}
	response, err := client.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	_, _ = io.Copy(io.Discard, response.Body)
	response.Body.Close()
	if response.StatusCode != 200 {
		t.Fatal("server not authenticated/ready")
	}
	cancel()
	select {
	case err = <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("server failed owned shutdown")
	}
	if strings.Contains(output.String(), token) {
		t.Fatal("startup printed token")
	}
	a, err := bootstrap.Open(context.Background(), bootstrap.Config{DataDirectory: dir, Demo: true})
	if err != nil {
		t.Fatal("locks not released", err)
	}
	closeCtx, closeCancel := context.WithTimeout(context.Background(), time.Second)
	defer closeCancel()
	if err = a.Close(closeCtx); err != nil {
		t.Fatal(err)
	}
}
