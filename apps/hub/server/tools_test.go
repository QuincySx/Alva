// INPUT: Local HTTP models, real optional plugins, authenticated routes and private session stores
// OUTPUT: Six real product tool loops, separate enablement/HITL and question ownership/late-answer regressions
// POS: [L7 Hub product] Settings are accepted only when their real execution and durable results work.
package server

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestHTTPOptionalToolsExecuteThroughActualProductRecipe(t *testing.T) {
	for _, tc := range []struct {
		kind, name, args string
		approval         bool
	}{
		{"planning", "task_create", `{"subject":"real workflow task"}`, true},
		{"utility", "config", `{"action":"set","key":"response.verbosity","value":"concise"}`, true},
		{"notebook", "notebook_edit", `{"notebook_path":"actual.ipynb","cell_id":"one","edit_mode":"replace","new_source":"actual edited source"}`, true},
		{"interaction", "ask_human", `{"question":"Actual clarification?","options":[{"label":"First"},{"label":"Second"}]}`, false},
		{"shell", "execute_shell", `{"command":"printf migrated-shell","timeout_ms":5000}`, true},
		{"web", "read_url", `{}`, true},
	} {
		t.Run(tc.kind, func(t *testing.T) {
			ctx := context.Background()
			var sourceCalls atomic.Int32
			source := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				sourceCalls.Add(1)
				fmt.Fprint(w, "actual referenced web evidence")
			}))
			defer source.Close()
			args := tc.args
			if tc.kind == "web" {
				encoded, _ := json.Marshal(map[string]string{"url": source.URL})
				args = string(encoded)
			}
			var modelCalls atomic.Int32
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				var request struct {
					Tools []struct {
						Function struct {
							Name string `json:"name"`
						} `json:"function"`
					} `json:"tools"`
				}
				if json.NewDecoder(r.Body).Decode(&request) != nil {
					http.Error(w, "bad fixture request", 400)
					return
				}
				advertised := false
				for _, tool := range request.Tools {
					advertised = advertised || tool.Function.Name == tc.name
				}
				if !advertised {
					t.Errorf("enabled actual %s tool not advertised", tc.name)
				}
				finish := "stop"
				delta := map[string]any{"content": "actual product fixture completed"}
				if modelCalls.Add(1) == 1 {
					finish = "tool_calls"
					delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "optional-call", "type": "function", "function": map[string]any{"name": tc.name, "arguments": args}}}}
				}
				data, _ := json.Marshal(map[string]any{"id": "optional-fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
			}))
			defer upstream.Close()
			f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
			s := create(t, f, "actual optional tool")
			prefix := "/v1/sessions/" + s.ID
			config := hub.ToolSettings{Shell: hub.ShellSettings{Executable: "/bin/sh"}, Web: hub.WebSettings{ReadURLs: true, AllowedPrivateOrigins: []string{source.URL}}}
			if err := f.app.Store.ReplaceTools(ctx, s.ID, config); err != nil {
				t.Fatal(err)
			}
			if tc.kind == "shell" || tc.kind == "web" {
				body(t, f.request(t, "PUT", prefix+"/settings/tools/"+tc.kind, []byte(`{"enabled":true}`), nil), 409)
			}
			body(t, f.request(t, "PUT", prefix+"/settings/tools/"+tc.kind, []byte(`{"enabled":true,"access_approved":true}`), nil), 200)
			if modelCalls.Load() != 0 || sourceCalls.Load() != 0 {
				t.Fatal("settings performed execution")
			}
			if tc.kind == "notebook" {
				if err := os.WriteFile(filepath.Join(s.Workspace, "actual.ipynb"), []byte(`{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"id":"one","cell_type":"code","metadata":{},"source":["old"],"outputs":[],"execution_count":null}]}`), 0600); err != nil {
					t.Fatal(err)
				}
			}
			body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"run actual selected optional tool"}`), nil), 202)
			if tc.approval {
				approval := httpApproval(t, f, s.ID)
				if approval.ToolName != tc.name {
					t.Fatal("wrong permission target", approval.ToolName)
				}
				if sourceCalls.Load() != 0 {
					t.Fatal("web started before call approval")
				}
				body(t, f.request(t, "PUT", prefix+"/settings/tools/"+tc.kind, []byte(`{"enabled":false}`), nil), 409)
				body(t, f.request(t, "POST", prefix+"/approvals/"+approval.ID+"/decision", []byte(`{"allow":true}`), nil), 200)
			} else {
				var pending []interaction.Question
				deadline := time.Now().Add(5 * time.Second)
				for time.Now().Before(deadline) {
					json.Unmarshal(body(t, f.request(t, "GET", prefix+"/questions", nil, nil), 200), &pending)
					if len(pending) > 0 {
						break
					}
					time.Sleep(time.Millisecond)
				}
				if len(pending) != 1 {
					t.Fatal("actual pending clarification absent")
				}
				other := create(t, f, "foreign question owner")
				path := "/questions/" + pending[0].ID + "/answer"
				body(t, f.request(t, "POST", "/v1/sessions/"+other.ID+path, []byte(`{"selected":[2]}`), nil), 409)
				body(t, f.request(t, "POST", prefix+path, []byte(`{"selected":[2]}`), nil), 200)
				body(t, f.request(t, "POST", prefix+path, []byte(`{"selected":[1]}`), nil), 409)
			}
			waitIdle(t, f, s.ID)
			detail, err := f.app.Store.Detail(ctx, s.ID)
			if err != nil || detail.Session.Status != engine.Succeeded {
				t.Fatal("real optional tool failed", detail, err)
			}
			if modelCalls.Load() != 2 {
				t.Fatal("real tool did not return to model", modelCalls.Load())
			}
			branch, _ := productBranch(detail)
			log, err := f.app.Repository.OpenSession(ctx, s.ID, branch.NativeSessionID)
			if err != nil {
				t.Fatal(err)
			}
			records, err := log.Records(ctx, 0)
			if err != nil {
				t.Fatal(err)
			}
			settled := false
			for _, record := range records {
				if record.ToolOutcome != nil && record.ToolOutcome.CallID == "optional-call" {
					settled = true
					if record.ToolOutcome.Status != abi.OutcomeSucceeded || record.ToolOutcome.Result.IsError {
						t.Fatal("optional tool returned a business error", record.ToolOutcome)
					}
				}
			}
			if !settled {
				t.Fatal("actual tool outcome absent")
			}
			if tc.kind == "notebook" {
				raw, _ := os.ReadFile(filepath.Join(s.Workspace, "actual.ipynb"))
				if !strings.Contains(string(raw), "actual edited source") {
					t.Fatal("notebook mutation absent")
				}
			}
			if tc.kind == "web" && sourceCalls.Load() != 1 {
				t.Fatal("expected one actual source request")
			}
		})
	}
}

func productBranch(detail hub.Detail) (hub.Branch, bool) {
	for _, b := range detail.Branches {
		if b.ID == detail.Session.ActiveBranchID {
			return b, true
		}
	}
	return hub.Branch{}, false
}
