// INPUT: hubFixture real assembly with Codex (app-server), Claude (stream-json) and Pi (rpc) backed by this test binary speaking each approval transport
// OUTPUT: Scenario where every external engine asks for one tool call, the user's click reaches the engine once, and a late click fails
// POS: [scenario] External-engine approvals through the same product entry points as Alva's own; no user CLI, account or network.
package scenarios

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"hash/fnv"
	"os"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

// TestExternalApprovalFixtureProcess plays the bidirectional side of each
// vendor: it asks for one tool call, waits for the host's decision and then
// says which decision it saw.
func TestExternalApprovalFixtureProcess(t *testing.T) {
	vendor := os.Getenv("ALVA_SCENARIO_APPROVAL_FIXTURE")
	if vendor == "" {
		t.Skip("fixture CLI for TestEveryExternalEngineAsksAndHonoursTheDecision")
	}
	out := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 4096), 8<<20)
	said := func(allowed bool) string {
		if allowed {
			return "approval was allowed"
		}
		return "approval was denied"
	}
	cwd, _ := os.Getwd()
	// Native identities are unique per workspace: one Hub runs several sessions.
	sum := fnv.New32a()
	sum.Write([]byte(cwd))
	native := fmt.Sprintf("fixture-%s-%x", vendor, sum.Sum32())
	switch vendor {
	case "claude":
		in.Scan() // the user message
		out(map[string]any{"type": "system", "subtype": "init", "session_id": native})
		input := map[string]string{"command": "printf hi"}
		out(map[string]any{"type": "assistant", "message": map[string]any{"content": []any{map[string]any{"type": "tool_use", "id": "y1", "name": "Bash", "input": input}}}})
		out(map[string]any{"type": "control_request", "request_id": "r1", "request": map[string]any{"subtype": "can_use_tool", "tool_name": "Bash", "input": input, "tool_use_id": "y1"}})
		in.Scan()
		var reply struct {
			Response struct{ Response struct{ Behavior string } }
		}
		_ = json.Unmarshal(in.Bytes(), &reply)
		allowed := reply.Response.Response.Behavior == "allow"
		out(map[string]any{"type": "user", "message": map[string]any{"content": []any{map[string]any{"type": "tool_result", "tool_use_id": "y1", "content": "ok", "is_error": !allowed}}}})
		out(map[string]any{"type": "assistant", "message": map[string]any{"content": []any{map[string]string{"type": "text", "text": said(allowed)}}}})
		out(map[string]any{"type": "result", "subtype": "success", "session_id": native})
		in.Scan()
	case "codex":
		note := func(method string, params map[string]any) {
			params["threadId"] = native
			if method != "thread/started" {
				params["turnId"] = "turn1"
			}
			out(map[string]any{"method": method, "params": params})
		}
		for in.Scan() {
			var frame struct {
				ID     json.RawMessage
				Method string
			}
			_ = json.Unmarshal(in.Bytes(), &frame)
			reply := func(result any) { out(map[string]any{"id": frame.ID, "result": result}) }
			switch frame.Method {
			case "initialize":
				reply(map[string]string{"userAgent": "fixture"})
			case "thread/start":
				out(map[string]any{"method": "thread/started", "params": map[string]any{"thread": map[string]any{"id": native}}})
				reply(map[string]any{"thread": map[string]any{"id": native, "cwd": cwd}})
			case "turn/start":
				reply(map[string]any{"turn": map[string]string{"id": "turn1", "status": "inProgress"}})
				note("item/started", map[string]any{"item": map[string]any{"type": "commandExecution", "id": "cmd", "command": "printf hi", "cwd": cwd, "status": "inProgress"}})
				out(map[string]any{"id": 7, "method": "item/commandExecution/requestApproval", "params": map[string]any{"threadId": native, "turnId": "turn1", "itemId": "cmd", "command": "printf hi", "availableDecisions": []string{"accept", "decline"}}})
			case "":
				allowed := strings.Contains(in.Text(), `"accept"`)
				status := "declined"
				if allowed {
					status = "completed"
				}
				note("item/completed", map[string]any{"item": map[string]any{"type": "commandExecution", "id": "cmd", "command": "printf hi", "status": status, "exitCode": 0, "aggregatedOutput": "hi"}})
				note("item/started", map[string]any{"item": map[string]any{"type": "agentMessage", "id": "answer"}})
				note("item/completed", map[string]any{"item": map[string]any{"type": "agentMessage", "id": "answer", "text": said(allowed)}})
				note("turn/completed", map[string]any{"turn": map[string]string{"id": "turn1", "status": "completed"}})
			}
		}
	case "pi":
		arg := func(flag string) string {
			for i, value := range os.Args {
				if value == flag && i+1 < len(os.Args) {
					return os.Args[i+1]
				}
			}
			return ""
		}
		path := arg("--session")
		if info, err := os.Stat(path); err == nil && info.Size() == 0 {
			header, _ := json.Marshal(map[string]any{"type": "session", "version": 3, "id": native, "cwd": cwd})
			_ = os.WriteFile(path, append(header, '\n'), 0o600)
		}
		nonce := os.Getenv("ALVA_PI_BRIDGE_NONCE")
		bridge := func(extra map[string]any) string {
			extra["alvaBridge"], extra["nonce"] = 1, nonce
			data, _ := json.Marshal(extra)
			return string(data)
		}
		out(map[string]any{"type": "extension_ui_request", "id": "ready", "method": "notify", "message": bridge(map[string]any{"kind": "ready"})})
		for in.Scan() {
			var frame map[string]json.RawMessage
			_ = json.Unmarshal(in.Bytes(), &frame)
			var kind, id string
			_ = json.Unmarshal(frame["type"], &kind)
			_ = json.Unmarshal(frame["id"], &id)
			respond := func(data any) {
				out(map[string]any{"type": "response", "id": id, "command": kind, "success": true, "data": data})
			}
			switch kind {
			case "get_state":
				respond(map[string]any{"sessionId": native, "sessionFile": path, "isStreaming": false, "isCompacting": false})
			case "prompt":
				respond(map[string]string{"disposition": "started"})
				out(map[string]string{"type": "agent_start"})
				args := map[string]string{"path": cwd + "/x.txt", "content": "hi"}
				out(map[string]any{"type": "tool_execution_start", "toolCallId": "w1", "toolName": "write", "args": args})
				out(map[string]any{"type": "extension_ui_request", "id": "perm", "method": "confirm", "title": "Alva native tool approval", "message": bridge(map[string]any{"kind": "permission", "toolCallId": "w1", "toolName": "write", "input": args})})
			case "extension_ui_response":
				var allowed bool
				_ = json.Unmarshal(frame["confirmed"], &allowed)
				out(map[string]any{"type": "tool_execution_end", "toolCallId": "w1", "toolName": "write", "isError": !allowed, "result": map[string]any{}})
				out(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}})
				out(map[string]any{"type": "message_end", "message": map[string]any{"role": "assistant", "stopReason": "stop", "content": []any{map[string]string{"type": "text", "text": said(allowed)}}}})
				out(map[string]any{"type": "agent_end", "willRetry": false})
				out(map[string]any{"type": "agent_settled", "aborted": false})
			}
		}
	}
	os.Exit(0)
}

func approvalEngines(t *testing.T) []bootstrap.ExternalEngine {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	var out []bootstrap.ExternalEngine
	for vendor, transport := range map[string]string{"codex": "app-server", "claude": "stream-json", "pi": "rpc"} {
		out = append(out, bootstrap.ExternalEngine{Vendor: vendor, Command: binary, Transport: transport, PrefixArgs: []string{"-test.run=^TestExternalApprovalFixtureProcess$", "--"}, ApprovedHostAccess: true,
			Env: map[string]string{"ALVA_SCENARIO_APPROVAL_FIXTURE": vendor}})
	}
	return out
}

func TestEveryExternalEngineAsksAndHonoursTheDecision(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind)
	config := liveConfig(t, model)
	config.External = approvalEngines(t)
	h := openHub(t, config)
	ctx := context.Background()
	for _, vendor := range []string{"codex", "claude", "pi"} {
		for _, allow := range []bool{true, false} {
			name := vendor + "-deny"
			want := "approval was denied"
			if allow {
				name, want = vendor+"-allow", "approval was allowed"
			}
			t.Run(name, func(t *testing.T) {
				s := h.create(name, t.TempDir())
				if _, err := h.service().SwitchTarget(ctx, s.ID, engineTarget(t, h, vendor), "user picked "+vendor); err != nil {
					t.Fatal(err)
				}
				h.send(s.ID, "do the thing")
				item := h.pending(s.ID)
				if item.View.Kind != product.InteractionKindPermission {
					t.Fatalf("%s asked something other than a permission: %+v", vendor, item)
				}
				if err := h.service().RespondInteraction(ctx, s.ID, item, product.InteractionResponse{Allow: allow}); err != nil {
					t.Fatal(err)
				}
				h.expect(s.ID, engine.Succeeded)
				if r := lastReply(t, h.blocks(s.ID)); r.Text != want {
					t.Fatalf("%s: the engine saw a different decision than the click: %q, want %q", vendor, r.Text, want)
				}
				if err := h.service().RespondInteraction(ctx, s.ID, item, product.InteractionResponse{Allow: true}); err == nil {
					t.Fatalf("%s: a late click on a finished approval must fail", vendor)
				}
			})
		}
	}
}
