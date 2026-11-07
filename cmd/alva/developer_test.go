// INPUT: Actual CLI developer dispatch, explicit temporary configuration and local HTTP model replies.
// OUTPUT: Discoverable separate test/eval entry points, cold default and task-score output independent of chat sessions.
// POS: [L6 CLI testing] No Hub, credentials, native session or Agent execution is required.
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/developer"
)

func TestCLIDeveloperModesAreSeparateAndColdByDefault(t *testing.T) {
	var out, diagnostic bytes.Buffer
	if err := run(t.Context(), []string{"--help"}, strings.NewReader(""), &out, &diagnostic); err != nil || !strings.Contains(diagnostic.String(), "developer test|eval") {
		t.Fatal(err, diagnostic.String())
	}
	if err := run(t.Context(), []string{"developer", "--help"}, strings.NewReader(""), &out, io.Discard); err != nil || !strings.Contains(out.String(), "versioned task quality") {
		t.Fatal(err, out.String())
	}
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"expected\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	expected := "expected"
	config := map[string]any{"version": 1, "routes": []any{map[string]string{"id": "fixture", "protocol": "openai_chat", "endpoint": server.URL, "model": "fixture"}}, "repeats": 2, "dataset": developer.Dataset{ID: "cli", Version: "v1", Tasks: []developer.Task{{ID: "task", Prompt: "Reply expected", ExpectedText: &expected, Weight: 1}}}}
	data, _ := json.Marshal(config)
	file := filepath.Join(t.TempDir(), "developer.json")
	if err := os.WriteFile(file, data, 0600); err != nil {
		t.Fatal(err)
	}
	out.Reset()
	if err := run(t.Context(), []string{"developer", "eval", "--config", file}, strings.NewReader(""), &out, io.Discard); err != nil || calls.Load() != 0 {
		t.Fatal(err, calls.Load())
	}
	var plan developer.Report
	if json.Unmarshal(out.Bytes(), &plan) != nil || plan.Kind != developer.Evaluate || plan.Mode != "plan" || plan.Scores[0].WeightedScore != nil {
		t.Fatal(plan, out.String())
	}
	out.Reset()
	if err := run(t.Context(), []string{"developer", "eval", "--config", file, "--run"}, strings.NewReader(""), &out, io.Discard); err != nil || calls.Load() != 2 {
		t.Fatal(err, calls.Load())
	}
	var result developer.Report
	if json.Unmarshal(out.Bytes(), &result) != nil || result.Scores[0].WeightedScore == nil || *result.Scores[0].WeightedScore != 100 || result.Scores[0].ConsistentTaskPasses != 1 {
		t.Fatal(result, out.String())
	}
	if err := run(t.Context(), []string{"developer", "test", "--config", file, "--run"}, strings.NewReader(""), io.Discard, io.Discard); err == nil || calls.Load() != 2 {
		t.Fatal("eval dataset was reused as integration tests")
	}
}
