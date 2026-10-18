// INPUT: Explicit stdin fixtures, process context and a private local HTTP upstream
// OUTPUT: No-discovery token/config validation and real headless startup/shutdown regression tests
// POS: [L6 application] Exercise Gateway executable ownership without starting user applications or paid model calls
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/system/loopback"
)

type unreadableInput struct{ t *testing.T }

func (r unreadableInput) Read([]byte) (int, error) {
	r.t.Error("token-only command read stdin")
	return 0, io.EOF
}

func TestTokenOnlyDoesNotReadConfigurationOrStartServer(t *testing.T) {
	var output, diagnostic bytes.Buffer
	if err := run(context.Background(), []string{"--new-token"}, unreadableInput{t}, &output, &diagnostic); err != nil {
		t.Fatal(err)
	}
	token := strings.TrimSpace(output.String())
	if _, err := loopback.New("127.0.0.1:1", token); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(output.String(), "http://") || diagnostic.Len() != 0 {
		t.Fatal("token command started or announced server")
	}
	if err := run(context.Background(), []string{"--new-token", "--secrets-stdin"}, unreadableInput{t}, io.Discard, io.Discard); err == nil {
		t.Fatal("token command accepted server options")
	}
}

func TestInvalidConfigurationDoesNotLeakSecretsOrOpenServer(t *testing.T) {
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		name string
		args []string
		body string
	}{
		{"missing-explicit-input", nil, `{}`},
		{"non-loopback", []string{"--secrets-stdin", "--listen", "0.0.0.0:0"}, `{}`},
		{"hostname-not-numeric", []string{"--secrets-stdin", "--listen", "localhost:0"}, `{}`},
		{"weak-token", []string{"--secrets-stdin"}, `{"token":"private-weak-token","routes":{}}`},
		{"unknown-field", []string{"--secrets-stdin"}, `{"token":"` + token + `","routes":{},"private-secret":1}`},
		{"duplicate-token", []string{"--secrets-stdin"}, `{"token":"` + token + `","token":"private-secret","routes":{}}`},
		{"wrong-case", []string{"--secrets-stdin"}, `{"Token":"` + token + `","routes":{}}`},
		{"null-routes", []string{"--secrets-stdin"}, `{"token":"` + token + `","routes":null}`},
		{"unknown-kind", []string{"--secrets-stdin"}, `{"token":"` + token + `","routes":{"alias":{"kind":"private-secret","endpoint":"https://upstream.invalid/api","model":"actual"}}}`},
		{"too-large", []string{"--secrets-stdin"}, strings.Repeat(" ", 64<<10) + "{}"},
	} {
		t.Run(test.name, func(t *testing.T) {
			var output, diagnostic bytes.Buffer
			err := run(context.Background(), test.args, strings.NewReader(test.body), &output, &diagnostic)
			if err == nil {
				t.Fatal("invalid configuration accepted")
			}
			if output.Len() != 0 || strings.Contains(err.Error(), "private-") || strings.Contains(diagnostic.String(), "private-") {
				t.Fatalf("configuration leaked or server announced: %s %s %s", err, &output, &diagnostic)
			}
		})
	}
}

type startupOutput struct{ lines chan string }

func (w startupOutput) Write(p []byte) (int, error) { w.lines <- string(p); return len(p), nil }

func TestActualProcessStartupListsExplicitModelsAndShutsDown(t *testing.T) {
	var upstreamCalls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		upstreamCalls.Add(1)
		t.Error("listing models must not call upstream")
	}))
	defer upstream.Close()
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	input, _ := json.Marshal(map[string]any{"token": token, "routes": map[string]any{"alias": map[string]any{"kind": "openai_chat", "endpoint": upstream.URL + "/api", "model": "actual-model", "api_key": "private-upstream-secret"}}})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started := make(chan string, 1)
	done := make(chan error, 1)
	var diagnostic bytes.Buffer
	go func() {
		done <- run(ctx, []string{"--secrets-stdin"}, bytes.NewReader(input), startupOutput{started}, &diagnostic)
	}()
	var announcement string
	select {
	case announcement = <-started:
	case err := <-done:
		t.Fatalf("startup failed: %v", err)
	case <-time.After(5 * time.Second):
		t.Fatal("startup did not settle")
	}
	if strings.Contains(announcement, token) || strings.Contains(announcement, "private-") {
		t.Fatal("startup leaked secrets")
	}
	fields := strings.Fields(announcement)
	if len(fields) < 3 || !strings.HasPrefix(fields[2], "http://127.0.0.1:") {
		t.Fatalf("unexpected startup: %s", announcement)
	}
	request, _ := http.NewRequest(http.MethodGet, fields[2]+"/v1/models", nil)
	request.Header.Set("Authorization", "Bearer "+token)
	client := &http.Client{Timeout: 3 * time.Second}
	response, err := client.Do(request)
	if err != nil {
		t.Fatal(err)
	}
	body, readErr := io.ReadAll(response.Body)
	response.Body.Close()
	if readErr != nil || response.StatusCode != 200 || !bytes.Contains(body, []byte(`"id":"alias"`)) || bytes.Contains(body, []byte("actual-model")) || upstreamCalls.Load() != 0 {
		t.Fatalf("bad explicit model listing: %s %v", body, readErr)
	}
	cancel()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("process failed to shut down")
	}
	if diagnostic.Len() != 0 {
		t.Fatalf("unexpected process diagnostic: %s", &diagnostic)
	}
}
