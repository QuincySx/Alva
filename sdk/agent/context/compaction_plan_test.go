// INPUT: context, encoding/json, errors, strings, testing, agent/abi, kernel/abi, llm/wire
// OUTPUT: Compaction planning, rendering and summary collection regressions
// POS: [L3 context capability testing] Cuts never split tool protocol groups, keep the newest turn, pin a long turn's input and bound every summary input.
package agentcontext

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func planUser(text string) wire.Message { return wire.TextMessage(wire.RoleUser, text) }
func planReply(text string) wire.Message {
	return wire.TextMessage(wire.RoleAssistant, text)
}
func planCall(id string) wire.Message {
	call := wire.ToolCall{ID: id, Name: "probe", Arguments: json.RawMessage(`{}`)}
	return wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &call}}}
}
func planResult(id, text string) wire.Message {
	result := wire.ToolResult{CallID: id, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}
	return wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
}

func TestPlanCompactionKeepsWholeRecentTurns(t *testing.T) {
	big := strings.Repeat("x", 400)
	view := []wire.Message{planUser("one " + big), planReply(big), planUser("two " + big), planCall("a"), planResult("a", big), planReply(big), planUser("three " + big), planReply("short")}
	plan, ok := PlanCompaction(view, 250, agentabi.SimpleTokenEstimator{})
	if !ok || plan.Cut != 6 || plan.Pinned != -1 {
		t.Fatal("did not keep the newest whole turn that fits", plan, ok)
	}
	plan, ok = PlanCompaction(view, 1_000_000, agentabi.SimpleTokenEstimator{})
	if !ok || plan.Cut != 2 {
		t.Fatal("a generous budget must still summarize the earliest turn only", plan, ok)
	}
	if _, ok := PlanCompaction(view[:2], 10, agentabi.SimpleTokenEstimator{}); ok {
		t.Fatal("a single turn without tool groups has nothing safe to summarize")
	}
}

func TestPlanCompactionCutsInsideALongTurnOnlyAtResolvedGroups(t *testing.T) {
	big := strings.Repeat("y", 800)
	view := []wire.Message{planUser("task"), planCall("a"), planResult("a", big), planCall("b"), planResult("b", big), planCall("c"), planResult("c", big), planReply("done")}
	plan, ok := PlanCompaction(view, 250, agentabi.SimpleTokenEstimator{})
	if !ok || plan.Pinned != 0 || view[plan.Cut].Role != wire.RoleAssistant {
		t.Fatal("long turn not pinned or cut off a group boundary", plan, ok)
	}
	pending := map[string]bool{}
	for _, message := range view[:plan.Cut] {
		for _, block := range message.Content {
			if block.ToolCall != nil {
				pending[block.ToolCall.ID] = true
			}
			if block.ToolResult != nil {
				delete(pending, block.ToolResult.CallID)
			}
		}
	}
	if len(pending) != 0 {
		t.Fatal("cut separated a tool call from its result")
	}
	if plan.Cut >= len(view) {
		t.Fatal("newest message summarized")
	}
}

func TestRenderForSummaryBoundsBlocksAndNeverOpensMedia(t *testing.T) {
	reference := wire.Reference{URI: "alva-artifact://secret"}
	message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{
		{Kind: wire.ContentText, Text: strings.Repeat("长", 2000)},
		{Kind: wire.ContentReference, Reference: &reference},
	}}
	text := RenderForSummary(message, 100)
	if len(text) > 300 || !strings.Contains(text, "bytes omitted") || strings.Contains(text, "secret") || !strings.Contains(text, "[reference omitted]") {
		t.Fatal("rendering not bounded or exposed a reference", text)
	}
	result := RenderForSummary(planResult("a", strings.Repeat("r", 5000)), 64)
	if !strings.HasPrefix(result, "[tool result]") || len(result) > 200 {
		t.Fatal("tool result not bounded", len(result))
	}
}

type summaryModel func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn summaryModel) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

func TestSummaryRequestAndCollection(t *testing.T) {
	request := SummaryRequest("old summary", "[#3 user]\nhello", 512, wire.ModelConfig{ExtraBody: json.RawMessage(`{"route":"x"}`), AllowedTools: []string{"probe"}})
	if !request.Config.DisableTools || len(request.Config.AllowedTools) != 0 || len(request.Tools) != 0 || request.Config.MaxTokens == nil || *request.Config.MaxTokens != 512 || string(request.Config.ExtraBody) != `{"route":"x"}` {
		t.Fatal("summary request may use tools or lost the route transport settings", request.Config)
	}
	if !strings.Contains(request.Messages[1].Text(), "Previous summary:\nold summary") {
		t.Fatal("previous summary not merged into the request")
	}
	answer := func(chunks ...wire.StreamChunk) summaryModel {
		return func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			for _, chunk := range chunks {
				if err := emit(chunk); err != nil {
					return err
				}
			}
			return nil
		}
	}
	usage := &wire.Usage{InputTokens: 9, OutputTokens: 3}
	text, reported, err := CollectSummary(t.Context(), answer(wire.StreamChunk{Kind: wire.ChunkText, Text: "  " + strings.Repeat("s", 40) + "  "}, wire.StreamChunk{Kind: wire.ChunkUsage, Usage: usage}, wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"}), request, 16)
	if err != nil || text != strings.Repeat("s", 16) || reported == nil || reported.InputTokens != 9 {
		t.Fatal("summary not bounded or usage lost", text, reported, err)
	}
	if _, _, err := CollectSummary(t.Context(), answer(wire.StreamChunk{Kind: wire.ChunkText, Text: "partial"}), request, 64); err == nil {
		t.Fatal("unfinished summary accepted")
	}
	if _, _, err := CollectSummary(t.Context(), answer(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"}), request, 64); err == nil {
		t.Fatal("empty summary accepted")
	}
	call := wire.ToolCall{ID: "x", Name: "probe", Arguments: json.RawMessage(`{}`)}
	if _, _, err := CollectSummary(t.Context(), answer(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}, wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "tool_calls"}), request, 64); err == nil {
		t.Fatal("tool-calling summary accepted")
	}
	failure := errors.New("provider down")
	if _, _, err := CollectSummary(t.Context(), summaryModel(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return failure }), request, 64); !errors.Is(err, failure) {
		t.Fatal("provider failure hidden", err)
	}
}
