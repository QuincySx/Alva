// INPUT: Production CLI flags, actual SDK WASIp1 worker, private journal and a local HTTP model
// OUTPUT: Explicit QuickJS/fuel/grant wiring, reference-only retrieval and durable actual resource outcome regressions
// POS: [L6 application] No host key/path into scripts, automatic code execution, paid model or resumed replay
package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	sandboxabi "github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

func TestWASMCLIExplicitScriptFuelReferencesAndDurableOutcome(t *testing.T) {
	module, workspace := cliWorker(t), t.TempDir()
	dataDir := filepath.Join(t.TempDir(), "private")
	const key = "host-only-script-fixture-key"
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		data, _ := io.ReadAll(r.Body)
		var request struct {
			Messages []struct {
				Role    string          `json:"role"`
				Content json.RawMessage `json:"content"`
			} `json:"messages"`
		}
		if err := json.Unmarshal(data, &request); err != nil {
			t.Error("invalid model request", err)
		}
		lastTool := ""
		for _, message := range request.Messages {
			if message.Role == "tool" {
				if err := json.Unmarshal(message.Content, &lastTool); err != nil {
					t.Error("tool text missing", err)
				}
			}
		}
		if r.Header.Get("Authorization") != "Bearer "+key || bytes.Contains(data, []byte(key)) || bytes.Contains(data, []byte(workspace)) {
			t.Error("host-only identity crossed script/model boundary")
		}
		switch calls.Add(1) {
		case 1:
			modelCall(w, "run_script", map[string]any{"script": `writeFile('script.txt','actual script file'); print('captured CLI'); 'SCRIPT-RESULT-MARKER'`})
		case 2:
			if strings.Contains(lastTool, `"result":"SCRIPT-RESULT-MARKER"`) {
				t.Error("complete script result automatically expanded")
			}
			uri := regexp.MustCompile(`memory-ref://[0-9a-f]{32}/[0-9a-f]{64}`).FindString(string(data))
			if uri == "" {
				t.Error("script result reference missing")
			}
			modelCall(w, "context_read", map[string]any{"uri": uri, "offset": 0, "limit": 4096})
		case 3:
			var page struct {
				Text string `json:"text"`
			}
			var result sandboxabi.ScriptResult
			if json.Unmarshal([]byte(lastTool), &page) != nil || json.Unmarshal([]byte(page.Text), &result) != nil || result.Validate() != nil || result.Result != "SCRIPT-RESULT-MARKER" || len(result.Output) != 1 || result.Output[0] != "captured CLI" {
				t.Error("explicit script result retrieval missing", lastTool)
			}
			modelText(w, "isolated script inspected")
		default:
			t.Error("script/model replay")
			modelText(w, "unexpected")
		}
	}))
	defer server.Close()
	args := append(wasmCLIArgs(module, workspace), "--wasm-scripts", "--wasm-fuel", "1000000000", "--live", "--endpoint", server.URL, "--model", "fixture", "--api-key-stdin", "--permission-mode", "accept_edits", "--data-dir", dataDir, "--prompt", "execute explicit isolated script")
	var output, diagnostics bytes.Buffer
	if err := run(t.Context(), args, strings.NewReader(key+"\n"), &output, &diagnostics); err != nil {
		t.Fatal(err, diagnostics.String())
	}
	if calls.Load() != 3 || output.String() != "isolated script inspected\n" || !strings.Contains(diagnostics.String(), "QuickJS enabled=true") {
		t.Fatal("CLI script controls did not take effect", calls.Load(), output.String(), diagnostics.String())
	}
	if data, err := os.ReadFile(filepath.Join(workspace, "script.txt")); err != nil || string(data) != "actual script file" {
		t.Fatal("CLI script did not use actual grant", string(data), err)
	}
	repo, err := session.Open(context.Background(), dataDir)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	list, err := repo.List(t.Context(), cliOwner, "", 4)
	if err != nil || len(list.Sessions) != 1 {
		t.Fatal("missing durable CLI session", list, err)
	}
	native, err := repo.OpenSession(t.Context(), cliOwner, list.Sessions[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	records, err := native.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, r := range records {
		if r.Type != "job_outcome" {
			continue
		}
		var facts struct {
			ScriptCalls              int
			FuelLimit, FuelRemaining uint64
			FuelExhausted            bool
		}
		if err = json.Unmarshal(r.Payload, &facts); err != nil || facts.ScriptCalls != 1 || facts.FuelLimit != 1_000_000_000 || facts.FuelRemaining == 0 || facts.FuelRemaining >= facts.FuelLimit || facts.FuelExhausted {
			t.Fatal("resource outcome not actual", facts, err)
		}
		found = true
	}
	if !found {
		t.Fatal("durable resource outcome missing")
	}
}
