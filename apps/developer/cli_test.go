// INPUT: Shipped developer CLI flags, explicit route/secret fixtures and all four real provider codecs over local HTTP.
// OUTPUT: Cold plans, real bounded trial matrices, credential privacy and transport/behavior failure separation.
// POS: [application developer testing] Never discovers accounts or invokes a paid model or advertised tool.
package developer

import (
	"bytes"
	"context"
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
	"time"

	"github.com/QuincySx/Alva/integration/llm/provider"
)

func configFile(t *testing.T, config configuration) string {
	t.Helper()
	name := filepath.Join(t.TempDir(), "routes.json")
	data, err := json.Marshal(config)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(name, data, 0600); err != nil {
		t.Fatal(err)
	}
	return name
}
func fixtureConfig(kind provider.Kind, endpoint string) configuration {
	return configuration{Version: 1, Routes: []route{{ID: "fixture", Protocol: kind, Endpoint: endpoint, Model: "fixture"}}, ToolNames: []string{"ask_human", "AskUserQuestion", "request_user_input"}, Repeats: 2, MaxOutputTokens: 1024}
}
func evaluate(t *testing.T, args []string, input string) (Report, string, error) {
	t.Helper()
	var out, diagnostic bytes.Buffer
	err := RunCLI(t.Context(), append([]string{"test"}, args...), strings.NewReader(input), &out, &diagnostic)
	var report Report
	if out.Len() > 0 && json.Unmarshal(out.Bytes(), &report) != nil {
		t.Fatal("invalid Report JSON", out.String())
	}
	return report, out.String() + diagnostic.String(), err
}
func event(w http.ResponseWriter, value any) {
	data, _ := json.Marshal(value)
	fmt.Fprintf(w, "data: %s\n\n", data)
}

func fixtureResponse(w http.ResponseWriter, kind provider.Kind, name string, args any, text string) {
	w.Header().Set("Content-Type", "text/event-stream")
	raw, _ := json.Marshal(args)
	switch kind {
	case provider.OpenAIChatKind:
		delta := map[string]any{"content": text}
		finish := "stop"
		if name != "" {
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "call", "type": "function", "function": map[string]any{"name": name, "arguments": string(raw)}}}}
			finish = "tool_calls"
		}
		event(w, map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}, "usage": map[string]int{"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}})
		fmt.Fprint(w, "data: [DONE]\n\n")
	case provider.OpenAIResponsesKind:
		if name != "" {
			event(w, map[string]any{"type": "response.output_item.added", "output_index": 0, "item": map[string]any{"type": "function_call", "id": "item", "call_id": "call", "name": name, "arguments": ""}})
			event(w, map[string]any{"type": "response.function_call_arguments.done", "output_index": 0, "item_id": "item", "arguments": string(raw)})
			event(w, map[string]any{"type": "response.output_item.done", "output_index": 0, "item": map[string]any{"type": "function_call", "id": "item", "call_id": "call", "name": name, "arguments": string(raw), "status": "completed"}})
		} else {
			event(w, map[string]any{"type": "response.output_item.done", "output_index": 0, "item": map[string]any{"type": "message", "id": "item", "role": "assistant", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": text}}}})
		}
		event(w, map[string]any{"type": "response.completed", "response": map[string]any{"id": "response", "status": "completed", "usage": map[string]int{"input_tokens": 10, "output_tokens": 5, "total_tokens": 15}}})
	case provider.AnthropicKind:
		event(w, map[string]any{"type": "message_start", "message": map[string]any{"id": "response", "role": "assistant", "usage": map[string]int{"input_tokens": 10, "output_tokens": 0}}})
		stop := "end_turn"
		if name != "" {
			event(w, map[string]any{"type": "content_block_start", "index": 0, "content_block": map[string]any{"type": "tool_use", "id": "call", "name": name, "input": map[string]any{}}})
			event(w, map[string]any{"type": "content_block_delta", "index": 0, "delta": map[string]any{"type": "input_json_delta", "partial_json": string(raw)}})
			stop = "tool_use"
		} else {
			event(w, map[string]any{"type": "content_block_start", "index": 0, "content_block": map[string]any{"type": "text", "text": ""}})
			event(w, map[string]any{"type": "content_block_delta", "index": 0, "delta": map[string]any{"type": "text_delta", "text": text}})
		}
		event(w, map[string]any{"type": "content_block_stop", "index": 0})
		event(w, map[string]any{"type": "message_delta", "delta": map[string]any{"stop_reason": stop}, "usage": map[string]int{"output_tokens": 5}})
		event(w, map[string]any{"type": "message_stop"})
	case provider.GeminiKind:
		part := map[string]any{"text": text}
		if name != "" {
			part = map[string]any{"functionCall": map[string]any{"id": "call", "name": name, "args": args}}
		}
		event(w, map[string]any{"responseId": "response", "candidates": []any{map[string]any{"index": 0, "content": map[string]any{"parts": []any{part}}, "finishReason": "STOP"}}, "usageMetadata": map[string]int{"promptTokenCount": 10, "candidatesTokenCount": 5, "totalTokenCount": 15}})
	}
}

func validReply(w http.ResponseWriter, kind provider.Kind, data []byte) {
	name := "ask_human"
	for _, variant := range []string{"AskUserQuestion", "request_user_input"} {
		if bytes.Contains(data, []byte(`"name":"`+variant+`"`)) {
			name = variant
		}
	}
	args := map[string]any{"question": "Please clarify"}
	switch {
	case bytes.Contains(data, []byte("What is 2+2?")):
		fixtureResponse(w, kind, "", nil, "4")
		return
	case bytes.Contains(data, []byte("current weather for the city Paris")):
		fixtureResponse(w, kind, "get_weather", map[string]string{"city": "Paris"}, "")
		return
	case bytes.Contains(data, []byte("which database")):
		args["options"] = []any{map[string]string{"label": "SQLite"}, map[string]string{"label": "PostgreSQL"}, map[string]string{"label": "MySQL"}}
	case bytes.Contains(data, []byte("which features")):
		args["options"] = []any{map[string]string{"label": "Search"}, map[string]string{"label": "Export"}, map[string]string{"label": "Alerts"}}
		args["multi_select"] = true
	}
	fixtureResponse(w, kind, name, args, "")
}

func TestDeveloperCLIAllProtocolsAndNameVariantsUseActualProviderPath(t *testing.T) {
	for _, kind := range []provider.Kind{provider.OpenAIChatKind, provider.OpenAIResponsesKind, provider.AnthropicKind, provider.GeminiKind} {
		t.Run(string(kind), func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				header, want := "Authorization", "Bearer fixture-key"
				if kind == provider.AnthropicKind {
					header, want = "X-Api-Key", "fixture-key"
				}
				if kind == provider.GeminiKind {
					header, want = "X-Goog-Api-Key", "fixture-key"
				}
				if r.Header.Get(header) != want {
					t.Error("explicit credential not bound to route")
				}
				data, _ := io.ReadAll(r.Body)
				validReply(w, kind, data)
			}))
			defer server.Close()
			file := configFile(t, fixtureConfig(kind, server.URL))
			plan, text, err := evaluate(t, []string{"--config", file}, "")
			if err != nil || calls.Load() != 0 || plan.Mode != "plan" || plan.TotalPlannedCalls != 30 || plan.Reports[0].AttemptedCalls != 0 || plan.Reports[0].Passed != 0 {
				t.Fatal(plan, text, err, calls.Load())
			}
			report, text, err := evaluate(t, []string{"--config", file, "--run", "--secrets-stdin"}, `{"fixture":"fixture-key"}`)
			if err != nil || calls.Load() != 30 || report.Mode != "explicit-run" || len(report.Reports) != 1 || report.Reports[0].Passed != 30 || report.Reports[0].Failed != 0 || report.Reports[0].Errors != 0 {
				t.Fatal(report, text, err, calls.Load())
			}
			if strings.Contains(text, "fixture-key") || strings.Contains(text, server.URL) || strings.Contains(text, "Please clarify") {
				t.Fatal("raw data leaked", text)
			}
			for _, result := range report.Reports[0].Results {
				if result.Usage == nil || result.Usage.InputTokens != 10 {
					t.Fatal("usage lost", result)
				}
			}
		})
	}
}

func TestDeveloperCLIPreflightAndCredentialsNeverCauseImplicitCalls(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); http.Error(w, "must not execute", 500) }))
	defer server.Close()
	config := fixtureConfig(provider.OpenAIChatKind, server.URL)
	file := configFile(t, config)
	for _, args := range [][]string{{"--config", file, "--max-calls", "29", "--run"}, {"--config", file, "--secrets-stdin"}, {"--config", file, "--per-call-timeout", "0s", "--run"}} {
		if _, _, err := evaluate(t, args, ""); err == nil {
			t.Fatal("invalid plan accepted", args)
		}
	}
	for _, secret := range []string{`{"unknown":"private"}`, `{"fixture":null}`, `{"fixture":"one","fixture":"two"}`} {
		if _, text, err := evaluate(t, []string{"--config", file, "--run", "--secrets-stdin"}, secret); err == nil || strings.Contains(text, "private") {
			t.Fatal(text, err)
		}
	}
	invalidNames := config
	invalidNames.ToolNames = []string{"9question"}
	if _, _, err := evaluate(t, []string{"--config", configFile(t, invalidNames), "--run"}, ""); err == nil {
		t.Fatal("non-portable question name admitted")
	}
	config.Routes = append(config.Routes, route{ID: "second", Protocol: "unknown", Endpoint: server.URL, Model: "fixture"})
	config.Repeats = 1
	config.ToolNames = []string{"ask_human"}
	file = configFile(t, config)
	if _, _, err := evaluate(t, []string{"--config", file, "--run"}, ""); err == nil {
		t.Fatal("invalid later route admitted")
	}
	if calls.Load() != 0 {
		t.Fatal("preflight performed inference", calls.Load())
	}
}

func TestDeveloperCLIErrorReportsStayPrivateAndDoNotRetryTrials(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		http.Error(w, "SECRET-RESPONSE-KEY", http.StatusUnauthorized)
	}))
	defer server.Close()
	config := fixtureConfig(provider.OpenAIChatKind, server.URL)
	config.ToolNames = []string{"ask_human"}
	config.Repeats = 1
	file := configFile(t, config)
	report, text, err := evaluate(t, []string{"--config", file, "--run"}, "")
	if err == nil || calls.Load() != 5 || report.Reports[0].Errors != 5 || report.Reports[0].Failed != 0 || strings.Contains(text, "SECRET-RESPONSE") || strings.Contains(text, server.URL) {
		t.Fatal(report, text, err, calls.Load())
	}
}

func TestDeveloperCLIGradesValidStreamsWithWrongBehaviorAsFailures(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		data, _ := io.ReadAll(r.Body)
		args := map[string]any{"question": "Clarify"}
		switch {
		case bytes.Contains(data, []byte("which database")):
			args["options"] = []any{map[string]string{"label": "SQLite"}, map[string]string{"label": "PostgreSQL"}, map[string]string{"label": "Oracle"}}
		case bytes.Contains(data, []byte("which features")):
			args["options"] = []any{map[string]string{"label": "Search"}, map[string]string{"label": "Export"}, map[string]string{"label": "Alerts"}}
		case bytes.Contains(data, []byte("project name")):
			args["session_id"] = "model-cannot-set-host-identity"
		}
		fixtureResponse(w, provider.OpenAIChatKind, "ask_human", args, "")
	}))
	defer server.Close()
	config := fixtureConfig(provider.OpenAIChatKind, server.URL)
	config.ToolNames = []string{"ask_human"}
	config.Repeats = 1
	report, _, err := evaluate(t, []string{"--config", configFile(t, config), "--run"}, "")
	if err == nil || len(report.Reports) != 1 || calls.Load() != 5 {
		t.Fatal(report, err, calls.Load())
	}
	r := report.Reports[0]
	if r.Passed != 0 || r.Failed != 5 || r.Errors != 0 {
		t.Fatal("incorrect behavior was not separated from transport errors", r)
	}
	for i, finding := range []string{"unexpected_choice_labels", "wrong_choice_count_or_mode", "invalid_question_arguments", "unnecessary_tool_call", "wrong_tool_selection"} {
		if len(r.Results[i].Findings) != 1 || r.Results[i].Findings[0] != finding {
			t.Fatal(r.Results[i])
		}
	}
}

func TestDeveloperCLIParentCancellationReturnsPartialReportAndClosesHTTP(t *testing.T) {
	entered := make(chan struct{}, 1)
	closed := make(chan struct{}, 1)
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		entered <- struct{}{}
		select {
		case <-r.Context().Done():
			closed <- struct{}{}
		case <-release:
		}
	}))
	defer func() {
		close(release)
		server.Close()
	}()
	config := fixtureConfig(provider.OpenAIChatKind, server.URL)
	config.ToolNames = []string{"ask_human"}
	config.Repeats = 1
	file := configFile(t, config)
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	var out bytes.Buffer
	done := make(chan error, 1)
	go func() {
		done <- RunCLI(ctx, []string{"test", "--config", file, "--run"}, strings.NewReader(""), &out, io.Discard)
	}()
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("no actual request")
	}
	cancel()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("cancel reported success")
		}
	case <-time.After(3 * time.Second):
		t.Fatal("evaluation did not join")
	}
	select {
	case <-closed:
	case <-time.After(3 * time.Second):
		t.Fatal("HTTP request retained")
	}
	var report Report
	if json.Unmarshal(out.Bytes(), &report) != nil || report.Reports[0].AttemptedCalls != 1 || report.Reports[0].PlannedCalls != 5 {
		t.Fatal(out.String())
	}
}
