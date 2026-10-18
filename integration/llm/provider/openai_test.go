// INPUT: testing, httptest, context, kernel/abi, llm/wire.
// OUTPUT: Deterministic local transport, bounded parsing and cancellation regressions.
// POS: [integration / model transport] Provider contract tests without paid endpoints or ambient credentials.
package provider

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestExplicitRequestAndFragmentedToolCall(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" || r.URL.Path != "/chat/completions" || r.Header.Get("Authorization") != "Bearer fixture" {
			t.Error("unexpected explicit request")
		}
		var body map[string]json.RawMessage
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		if string(body["model"]) != "\"fixture-model\"" || string(body["stream"]) != "true" {
			t.Error("missing model/stream")
		}
		if !strings.Contains(string(body["messages"]), "raw bytes not delivered") || !strings.Contains(string(body["tools"]), "fixture_tool") {
			t.Error("wire conversion omitted references/tools")
		}
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"fixture_tool\",\"arguments\":\"{\\\"value\\\":\"}}]}}]}\n\n")
		fmt.Fprint(w, "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"42}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n")
		fmt.Fprint(w, "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5}}\n\ndata: [DONE]\n\n")
	}))
	defer server.Close()
	model, err := NewOpenAIChat(Config{Endpoint: server.URL + "/chat/completions", Model: "fixture-model", APIKey: "fixture"})
	if err != nil {
		t.Fatal(err)
	}
	var chunks []wire.StreamChunk
	request := abi.ModelRequest{Messages: []wire.Message{{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &wire.Reference{URI: "session://file", Bytes: 100, Processing: "not processed"}}}}}, Tools: []wire.ToolDefinition{{Name: "fixture_tool", Parameters: json.RawMessage(`{"type":"object"}`)}}}
	if err = model.Stream(context.Background(), request, func(chunk wire.StreamChunk) error { chunks = append(chunks, chunk); return nil }); err != nil {
		t.Fatal(err)
	}
	if len(chunks) != 4 || chunks[0].Text != "hello" || chunks[1].Kind != wire.ChunkUsage || chunks[2].ToolCall == nil || string(chunks[2].ToolCall.Arguments) != `{"value":42}` || chunks[3].Kind != wire.ChunkFinish {
		t.Fatalf("unexpected chunks: %#v", chunks)
	}
}

func TestMalformedTruncatedAndBoundedStreams(t *testing.T) {
	fixtures := map[string]string{
		"truncated":            "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
		"premature done":       "data: [DONE]\n\n",
		"empty tool terminal":  "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
		"stop with tool calls": "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"echo\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
		"duplicate finish":     "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
		"malformed":            "data: {nope}\n\n",
		"oversize":             "data: " + strings.Repeat("x", 300) + "\n\n",
		"incomplete tool":      "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call\",\"function\":{\"name\":\"echo\",\"arguments\":\"{\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n",
		"length terminal":      "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
	}
	for name, stream := range fixtures {
		t.Run(name, func(t *testing.T) {
			var finished bool
			err := parseChatFixture(context.Background(), strings.NewReader(stream), 256, func(chunk wire.StreamChunk) error {
				if chunk.Kind == wire.ChunkFinish {
					finished = true
				}
				return nil
			})
			if err == nil || finished {
				t.Fatalf("failed stream accepted: %v finished=%v", err, finished)
			}
		})
	}
}

func TestCallbackErrorPropagates(t *testing.T) {
	want := errors.New("subscriber failed")
	err := parseChatFixture(context.Background(), strings.NewReader("data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}]}\n\n"), 1024, func(wire.StreamChunk) error { return want })
	if !errors.Is(err, want) {
		t.Fatalf("callback error lost: %v", err)
	}
}

func TestHTTPFailureDoesNotExposeResponseOrCredentials(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusUnauthorized)
		fmt.Fprint(w, "private fixture body")
	}))
	defer server.Close()
	model, _ := NewOpenAIChat(Config{Endpoint: server.URL, Model: "fixture", APIKey: "secret-fixture"})
	err := model.Stream(context.Background(), abi.ModelRequest{}, func(wire.StreamChunk) error { return nil })
	if err == nil || strings.Contains(err.Error(), "private") || strings.Contains(err.Error(), "secret-fixture") {
		t.Fatalf("unsafe/absent error: %v", err)
	}
}

func TestRedirectDoesNotReachAnotherEndpoint(t *testing.T) {
	called := false
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { called = true }))
	defer target.Close()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusTemporaryRedirect)
	}))
	defer server.Close()
	model, _ := NewOpenAIChat(Config{Endpoint: server.URL, Model: "fixture", APIKey: "fixture"})
	if err := model.Stream(context.Background(), abi.ModelRequest{}, func(wire.StreamChunk) error { return nil }); err == nil || called {
		t.Fatalf("redirect followed: called=%v err=%v", called, err)
	}
}

func TestCancellationClosesLiveStream(t *testing.T) {
	started := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		w.WriteHeader(http.StatusOK)
		w.(http.Flusher).Flush()
		close(started)
		<-r.Context().Done()
	}))
	defer server.Close()
	model, _ := NewOpenAIChat(Config{Endpoint: server.URL, Model: "fixture"})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- model.Stream(ctx, abi.ModelRequest{}, func(wire.StreamChunk) error { return nil }) }()
	<-started
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("wrong cancellation: %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("request did not exit on cancellation")
	}
}

func TestConfigRequiresExplicitSafeRoute(t *testing.T) {
	for _, config := range []Config{{}, {Endpoint: "file:///tmp/model", Model: "m"}, {Endpoint: "https://user:pass@example.com/chat", Model: "m"}, {Endpoint: "https://example.com/chat?key=x", Model: "m"}, {Endpoint: "https://example.com/chat", Model: "m", APIKey: "x\r\ny"}} {
		if _, err := NewOpenAIChat(config); err == nil {
			t.Fatalf("invalid config accepted: %#v", config)
		}
	}
}
