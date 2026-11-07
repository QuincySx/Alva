// INPUT: Explicit installed Pi/node paths and a project-owned localhost HTTP response fixture
// OUTPUT: Actual Pi agent-loop pre-write permission and free-text question contract verification
// POS: [integration / native external-engine adapter] Opt-in local protocol acceptance uses private fixture config and never real account material
package process

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/bindings"
	owned "github.com/QuincySx/Alva/platform/system/process"
)

func TestPiNativeCLIApprovalAndQuestionWithLocalHTTPFixture(t *testing.T) {
	node, script := os.Getenv("ALVA_PI_TEST_NODE"), os.Getenv("ALVA_PI_TEST_SCRIPT")
	if node == "" || script == "" {
		t.Skip("explicit ALVA_PI_TEST_NODE and ALVA_PI_TEST_SCRIPT enable the installed-CLI local-only contract test")
	}
	if !filepath.IsAbs(node) || !filepath.IsAbs(script) {
		t.Fatal("explicit absolute installed runtime and script required")
	}
	for _, allow := range []bool{false, true} {
		t.Run(fmt.Sprint(allow), func(t *testing.T) {
			workspace, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			target := filepath.Join(workspace, "native-approved.txt")
			answer := "Native fixture complete"
			if allow {
				answer = piLongReply("actual native answer", 80<<10)
			}
			expectedAnswer := strings.ReplaceAll(answer, "actual native answer", "[REDACTED]")
			var mu sync.Mutex
			var requests int
			var problem string
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				mu.Lock()
				defer mu.Unlock()
				requests++
				body, err := io.ReadAll(io.LimitReader(r.Body, 1<<20))
				if err != nil {
					problem = err.Error()
				}
				var request struct {
					Model    string
					Messages []struct {
						Role    string
						Content json.RawMessage
					}
					Tools []struct{ Function struct{ Name string } }
				}
				if json.Unmarshal(body, &request) != nil || request.Model != "alva-native-fixture" {
					problem = "wrong native model/request"
				}
				tools := map[string]bool{}
				for _, tool := range request.Tools {
					tools[tool.Function.Name] = true
				}
				if !tools["write"] || !tools["ask_user"] {
					problem = "actual bridge tools were absent from native model request"
				}
				w.Header().Set("Content-Type", "text/event-stream")
				send := func(value any) { data, _ := json.Marshal(value); fmt.Fprintf(w, "data: %s\n\n", data) }
				delta := map[string]any{"role": "assistant"}
				finish := "stop"
				switch requests {
				case 1:
					delta["tool_calls"] = []any{map[string]any{"index": 0, "id": "native-write", "type": "function", "function": map[string]string{"name": "write", "arguments": mustJSON(map[string]string{"path": target, "content": "native gated edit"})}}}
					finish = "tool_calls"
				case 2:
					data, err := os.ReadFile(target)
					if allow && (err != nil || string(data) != "native gated edit") {
						problem = "approved native write was absent before the next model request"
					}
					if !allow && !os.IsNotExist(err) {
						problem = "denied native write executed"
					}
					delta["tool_calls"] = []any{map[string]any{"index": 0, "id": "native-question", "type": "function", "function": map[string]string{"name": "ask_user", "arguments": `{"question":"Native loop question?"}`}}}
					finish = "tool_calls"
				case 3:
					if !strings.Contains(string(body), "actual native answer") {
						problem = "native ask_user result did not reach the next model request"
					}
					delta["content"] = answer
				default:
					problem = "native loop made unexpected extra model requests"
					delta["content"] = "Unexpected fixture request"
				}
				send(map[string]any{"id": "fixture-completion", "object": "chat.completion.chunk", "created": 0, "model": "alva-native-fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": nil}}})
				send(map[string]any{"id": "fixture-completion", "object": "chat.completion.chunk", "created": 0, "model": "alva-native-fixture", "choices": []any{map[string]any{"index": 0, "delta": map[string]any{}, "finish_reason": finish}}, "usage": map[string]int{"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}})
				fmt.Fprint(w, "data: [DONE]\n\n")
			}))
			defer server.Close()
			profile := t.TempDir()
			models := map[string]any{"providers": map[string]any{"alva-local-fixture": map[string]any{"baseUrl": server.URL + "/v1", "api": "openai-completions", "apiKey": "local-fixture-not-an-account-key", "models": []any{map[string]any{"id": "alva-native-fixture", "name": "Local protocol fixture", "reasoning": false, "input": []string{"text"}, "contextWindow": 32768, "maxTokens": 1024, "cost": map[string]int{"input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0}}}}}}
			if err := os.WriteFile(filepath.Join(profile, "models.json"), []byte(mustJSON(models)), 0600); err != nil {
				t.Fatal(err)
			}
			directory := filepath.Join(t.TempDir(), "state")
			store, err := bindings.Open(t.Context(), directory)
			if err != nil {
				t.Fatal(err)
			}
			a, err := New(Config{Bindings: bindingPorts{store}, Vendor: "pi", Command: node, PrefixArgs: []string{script}, Transport: "rpc", StateDirectory: directory, CLIConfigDirectory: profile, ApprovedHostAccess: true, Timeout: 30 * time.Second, Env: map[string]string{"PATH": filepath.Dir(node) + string(os.PathListSeparator) + owned.SafePath}})
			if err != nil {
				store.Close()
				t.Fatal(err)
			}
			defer a.Close(context.Background())
			var terminal engine.Outcome
			var permissions, questions int
			var details []string
			var final strings.Builder
			err = a.Execute(t.Context(), engine.Request{OwnerID: "native-local-fixture", Workspace: workspace, Prompt: "Run the fixture tools", Provider: "alva-local-fixture", Model: "alva-native-fixture"}, func(event engine.Event) error {
				switch event.Kind {
				case engine.PermissionRequest:
					permissions++
					if _, err := os.Stat(target); !os.IsNotExist(err) {
						t.Fatal("actual native write ran before the permission callback", err)
					}
					return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: allow})
				case engine.UserInputRequested:
					questions++
					return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"answer": {"actual native answer"}}})
				case engine.Error:
					details = append(details, event.Text)
				case engine.Message:
					final.Reset()
					final.WriteString(event.Text)
				case engine.MessageDelta:
					final.WriteString(event.Text)
				case engine.Completed:
					terminal = event.Outcome
				}
				return nil
			})
			mu.Lock()
			count, issue := requests, problem
			mu.Unlock()
			if err != nil || terminal != engine.Succeeded || permissions != 1 || questions != 1 || count != 3 || issue != "" || final.String() != expectedAnswer {
				t.Fatal("actual Pi/native extension/local HTTP contract failed", terminal, permissions, questions, count, issue, details, err)
			}
		})
	}
}
