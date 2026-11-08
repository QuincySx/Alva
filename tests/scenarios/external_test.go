// INPUT: hubFixture real assembly with three explicitly approved external engines backed by this test binary as a JSONL fixture CLI
// OUTPUT: Scenario where one conversation moves Alva -> Codex -> Claude -> Pi -> Alva with handoffs, typed tool blocks and restart
// POS: [scenario] External engines through the same product entry points; no user CLI, account or network.
package scenarios

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

// TestExternalEngineFixtureProcess plays a vendor CLI's print/exec JSONL
// protocol: it records its stdin and answers with one command and one reply.
func TestExternalEngineFixtureProcess(t *testing.T) {
	vendor := os.Getenv("ALVA_SCENARIO_ENGINE_FIXTURE")
	if vendor == "" {
		t.Skip("fixture CLI for TestConversationMovesBetweenAlvaAndEachExternalEngine")
	}
	input, _ := io.ReadAll(os.Stdin)
	if file, err := os.OpenFile(os.Getenv("ALVA_SCENARIO_ENGINE_LOG"), os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600); err == nil {
		fmt.Fprintf(file, "=== %s\n%s\n", vendor, input)
		file.Close()
	}
	native := "fixture-" + vendor
	switch vendor {
	case "codex":
		fmt.Println(`{"type":"thread.started","thread_id":"` + native + `"}`)
		fmt.Println(`{"type":"item.started","item":{"id":"x1","type":"command_execution","command":"ls -la"}}`)
		fmt.Println(`{"type":"item.completed","item":{"id":"x1","type":"command_execution","command":"ls -la","exit_code":0,"status":"completed","aggregated_output":"a.txt"}}`)
		fmt.Println(`{"type":"item.completed","item":{"type":"agent_message","id":"answer","text":"hello from codex"}}`)
		fmt.Println(`{"type":"turn.completed","usage":{"input_tokens":7,"output_tokens":3}}`)
	case "claude":
		fmt.Println(`{"type":"system","subtype":"init","session_id":"` + native + `"}`)
		fmt.Println(`{"type":"assistant","message":{"content":[{"type":"tool_use","id":"y1","name":"Bash","input":{"command":"ls -la"}}]}}`)
		fmt.Println(`{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"y1","content":"a.txt"}]}}`)
		fmt.Println(`{"type":"assistant","message":{"content":[{"type":"text","text":"hello from claude"}]}}`)
		fmt.Println(`{"type":"result","subtype":"success","session_id":"` + native + `"}`)
	case "pi":
		for i, arg := range os.Args {
			if arg == "--session" && i+1 < len(os.Args) {
				if info, err := os.Stat(os.Args[i+1]); err == nil && info.Size() > 0 {
					break
				}
				cwd, _ := os.Getwd()
				header, _ := json.Marshal(map[string]string{"type": "session", "id": native, "cwd": cwd})
				if err := os.WriteFile(os.Args[i+1], append(header, '\n'), 0o600); err != nil {
					os.Exit(23)
				}
			}
		}
		fmt.Println(`{"type":"session","id":"` + native + `"}`)
		fmt.Println(`{"type":"tool_execution_start","toolCallId":"z1","toolName":"ls","args":{"path":"."}}`)
		fmt.Println(`{"type":"tool_execution_end","toolCallId":"z1","toolName":"ls","isError":false,"result":{}}`)
		fmt.Println(`{"type":"message_start","message":{"role":"assistant"}}`)
		fmt.Println(`{"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"text","text":"hello from pi"}]}}`)
		fmt.Println(`{"type":"agent_end","willRetry":false}`)
		fmt.Println(`{"type":"agent_settled","aborted":false}`)
	}
	os.Exit(0)
}

func externalEngines(t *testing.T, log string) []bootstrap.ExternalEngine {
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	var out []bootstrap.ExternalEngine
	for _, vendor := range []string{"codex", "claude", "pi"} {
		out = append(out, bootstrap.ExternalEngine{Vendor: vendor, Command: binary, PrefixArgs: []string{"-test.run=^TestExternalEngineFixtureProcess$", "--"}, ApprovedHostAccess: true,
			Env: map[string]string{"ALVA_SCENARIO_ENGINE_FIXTURE": vendor, "ALVA_SCENARIO_ENGINE_LOG": log}})
	}
	return out
}

func engineTarget(t *testing.T, h *hubFixture, id string) hub.Target {
	t.Helper()
	for _, target := range h.service().AvailableTargets() {
		if target.EngineID == id {
			return target
		}
	}
	t.Fatalf("no %s route", id)
	return hub.Target{}
}

func TestConversationMovesBetweenAlvaAndEachExternalEngine(t *testing.T) {
	log := filepath.Join(t.TempDir(), "engine-stdin.log")
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(r modelRequest) reply { return text("alva answer ", fmt.Sprint(r.N)) }
	config := liveConfig(t, model)
	config.External = externalEngines(t, log)
	h := openHub(t, config)
	s := h.create("multi engine", "")
	ctx := context.Background()
	h.send(s.ID, "alva first turn")
	h.expect(s.ID, engine.Succeeded)
	alvaBranch := h.branch(s.ID)
	for _, vendor := range []string{"codex", "claude", "pi"} {
		if _, err := h.service().SwitchTarget(ctx, s.ID, engineTarget(t, h, vendor), "user picked "+vendor); err != nil {
			t.Fatal(err)
		}
		h.send(s.ID, vendor+" turn please")
		h.expect(s.ID, engine.Succeeded)
		blocks := h.blocks(s.ID)
		if r := lastReply(t, blocks); r.Text != "hello from "+vendor {
			t.Fatalf("%s reply %+v", vendor, r)
		}
		var tool *product.Block
		if tools := blocksOf(blocks, product.BlockKindTool); len(tools) > 0 {
			tool = &tools[len(tools)-1]
		}
		if tool == nil || tool.Tool == nil || tool.Status != product.BlockStatusCompleted {
			t.Fatalf("%s tool block %+v", vendor, tool)
		}
		if vendor != "pi" && (tool.Tool.Kind != engine.ToolCommand || tool.Tool.Command != "ls -la") {
			t.Fatalf("%s command block %+v", vendor, tool.Tool)
		}
		if b := h.branch(s.ID); b.Target.EngineID != vendor || b.NativeSessionID != "fixture-"+vendor {
			t.Fatalf("%s branch %+v", vendor, b)
		}
	}
	data, err := os.ReadFile(log)
	if err != nil {
		t.Fatal(err)
	}
	stdin := string(data)
	for _, vendor := range []string{"codex", "claude", "pi"} {
		section := stdin[strings.Index(stdin, "=== "+vendor):]
		if next := strings.Index(section[4:], "=== "); next >= 0 {
			section = section[:next+4]
		}
		if !strings.Contains(section, "[Hub handoff") || !strings.Contains(section, "alva first turn") || !strings.Contains(section, vendor+" turn please") {
			t.Fatalf("%s did not receive the handoff and its prompt:\n%s", vendor, section)
		}
	}
	// Back to Alva: the original native conversation continues with a handoff
	// of what happened on the other engines.
	if _, err := h.service().SwitchTarget(ctx, s.ID, alvaBranch.Target, "back to alva"); err != nil {
		t.Fatal(err)
	}
	h.send(s.ID, "back on alva")
	h.expect(s.ID, engine.Succeeded)
	if b := h.branch(s.ID); b.ID != alvaBranch.ID || b.NativeSessionID != alvaBranch.NativeSessionID {
		t.Fatalf("returning to Alva did not reuse its branch: %+v vs %+v", b, alvaBranch)
	}
	back := model.request(model.calls())
	if !back.has("alva first turn") || !back.has("[Hub handoff") || !back.has("pi turn please") {
		t.Fatalf("Alva did not receive its history plus the handoff: %s", back.Body)
	}
	// Every engine's turns survive a restart in the one Message Stream.
	h.reopen()
	var replies []string
	for _, b := range blocksOf(h.blocks(s.ID), product.BlockKindReply) {
		replies = append(replies, b.Text)
	}
	joined := strings.Join(replies, "|")
	for _, want := range []string{"hello from codex", "hello from claude", "hello from pi", "alva answer"} {
		if !strings.Contains(joined, want) {
			t.Fatalf("after restart the Message Stream lacks %q: %s", want, joined)
		}
	}
}
