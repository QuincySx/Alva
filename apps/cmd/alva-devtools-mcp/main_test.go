// INPUT: Actual command helper, explicitly inherited token descriptor and native MCP client
// OUTPUT: Real stdio initialize/list/read, secret separation and open-token-pipe cancellation regressions
// POS: [L7 entrypoint] CLI tests use only temporary local credentials and never invoke user model accounts.
package main

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/protocol/mcp"
	"github.com/QuincySx/Alva/platform/storage/sqlite"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/QuincySx/Alva/platform/system/process"
)

func TestDevtoolsCommandHelper(t *testing.T) {
	mode := os.Getenv("ALVA_DEVTOOLS_HELPER")
	if mode == "" {
		return
	}
	ctx := context.Background()
	var cancel context.CancelFunc
	if mode == "cancel-secret" {
		ctx, cancel = context.WithTimeout(ctx, 50*time.Millisecond)
		defer cancel()
	}
	args := []string{"--hub", os.Getenv("ALVA_DEVTOOLS_ADDRESS"), "--token-fd", "3"}
	if mode == "ui" {
		args = []string{"--ui", os.Getenv("ALVA_DEVTOOLS_ADDRESS"), "--token-file", os.Args[len(os.Args)-1]}
	}
	err := run(ctx, args, os.Stdin, os.Stdout, os.Stderr)
	if mode == "cancel-secret" {
		if err == nil {
			os.Exit(4)
		}
		os.Exit(0)
	}
	if err != nil {
		os.Exit(3)
	}
	os.Exit(0)
}
func TestActualDevtoolsCommandUsesSeparateTokenDescriptor(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix fd3 ABI; portable private-file transport covered separately")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "GET" || r.URL.Path != "/v1/sessions" || r.Header.Get("Authorization") != "Bearer "+token {
			http.Error(w, "rejected", 403)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		io.WriteString(w, `{"sessions":[],"has_more":false}`)
	}))
	defer upstream.Close()
	path := filepath.Join(t.TempDir(), "explicit-token")
	if err = os.WriteFile(path, []byte(token), 0600); err != nil {
		t.Fatal(err)
	}
	secret, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer secret.Close()
	client, err := mcp.Open(ctx, mcp.Config{ID: "devtools", Process: process.Config{Command: os.Args[0], Args: []string{"-test.run=^TestDevtoolsCommandHelper$", "--"}, Cwd: t.TempDir(), Env: map[string]string{"ALVA_DEVTOOLS_HELPER": "serve", "ALVA_DEVTOOLS_ADDRESS": upstream.URL}, ReadFiles: []*os.File{secret}}}, mcp.ObserverFunc(func(_ context.Context, event mcp.IOEvent) (string, error) {
		if strings.Contains(string(event.Frame), token) {
			t.Error("secret leaked into protocol/stdout")
		}
		return "", nil
	}))
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())
	tools, err := client.ListTools(ctx)
	if err != nil || len(tools) != 1 || tools[0].Name != "hub_inspect" {
		t.Fatal("real command catalog invalid", err)
	}
	result, err := client.CallTool(ctx, "hub_inspect", json.RawMessage(`{"action":"sessions"}`))
	if err != nil || !strings.Contains(string(result), "has_more") {
		t.Fatal("actual command read failed", string(result), err)
	}
	if err = client.Close(ctx); err != nil || !client.CleanupComplete() {
		t.Fatal("actual command not reaped", err)
	}
}
func TestCommandTokenPipeReadIsCancellableWithoutProtocolOutput(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Unix fd3 ABI; inherited stdin cancellation covered by secretinput Windows tests")
	}
	read, write, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer read.Close()
	defer write.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestDevtoolsCommandHelper$", "--")
	cmd.Env = []string{"ALVA_DEVTOOLS_HELPER=cancel-secret", "ALVA_DEVTOOLS_ADDRESS=http://127.0.0.1:1", "PATH=" + process.SafePath}
	cmd.ExtraFiles = []*os.File{read}
	output, err := cmd.Output()
	if err != nil || len(output) != 0 {
		t.Fatal("secret cancellation did not join without stdout leakage", err, len(output))
	}
}

func TestActualUIDevtoolsCommandUsesExplicitPrivateFileWithoutTokenInProtocol(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 8*time.Second)
	defer cancel()
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "GET" || r.URL.Path != "/v1/debug/health" || r.Header.Get("Authorization") != "Bearer "+token {
			http.Error(w, "rejected", 403)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		io.WriteString(w, `{"status":"ready","controls":false,"execution":false}`)
	}))
	defer upstream.Close()
	directory := filepath.Join(t.TempDir(), "private")
	if _, err := sqlite.EnsurePrivateDir(directory); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "explicit-token")
	if err := os.WriteFile(path, []byte(token), 0600); err != nil {
		t.Fatal(err)
	}
	client, err := mcp.Open(ctx, mcp.Config{ID: "ui-devtools", Process: process.Config{Command: os.Args[0], Args: []string{"-test.run=^TestDevtoolsCommandHelper$", "--", path}, Cwd: directory, Env: map[string]string{"ALVA_DEVTOOLS_HELPER": "ui", "ALVA_DEVTOOLS_ADDRESS": upstream.URL}}}, mcp.ObserverFunc(func(_ context.Context, event mcp.IOEvent) (string, error) {
		if strings.Contains(string(event.Frame), token) {
			t.Error("file token leaked into protocol")
		}
		return "", nil
	}))
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())
	tools, err := client.ListTools(ctx)
	if err != nil || len(tools) != 2 {
		t.Fatal("portable UI catalog invalid", err)
	}
	result, err := client.CallTool(ctx, "ui_inspect", json.RawMessage(`{"action":"health"}`))
	if err != nil || !strings.Contains(string(result), "ready") {
		t.Fatal("actual portable UI read failed", err)
	}
	if err := client.Close(ctx); err != nil || !client.CleanupComplete() {
		t.Fatal("actual UI process not reaped", err)
	}
	data, err := os.ReadFile(path)
	if err != nil || string(data) != token {
		t.Fatal("explicit readonly input was changed", err)
	}
}
