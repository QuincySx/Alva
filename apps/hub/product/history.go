// INPUT: platform/system/httpjson, Bound product/native identities, Hub Store and kernel Tool contract
// OUTPUT: HistoryTool with fixed-high-water metadata search and explicit UTF-8 byte-range reads
// POS: [L7 Hub product] Product context capability; neither grants foreign native histories nor injects complete transcripts
package product

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type HistoryTool struct {
	Store               hub.Store
	ProductID, NativeID string
}

func (HistoryTool) Name() string { return "hub_history" }
func (HistoryTool) Description() string {
	return "List/literal-search small references across this owned product session's engine branches, then explicitly read a selected product event byte range. Fixed high-water, bounded scan and truthful coverage; no Agent resume, raw native transcript, files or MCP worker starts."
}
func (HistoryTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"action":{"enum":["list","search","read"]},"query":{"type":"string","maxLength":256},"after":{"type":"integer","minimum":0},"high_water":{"type":"integer","minimum":0},"sequence":{"type":"integer","minimum":1},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":2048}},"required":["action"],"additionalProperties":false}`)
}
func (t *HistoryTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if t == nil || t.Store == nil || t.ProductID == "" || t.NativeID == "" || execution.SessionID != t.NativeID || call.Name != t.Name() || execution.ActualTool != t || call.ID == "" || execution.CallID != call.ID {
		return wire.ToolResult{}, errors.New("Hub history execution binding denied")
	}
	detail, err := t.Store.Detail(ctx, t.ProductID)
	if err != nil {
		return wire.ToolResult{}, err
	}
	owned := false
	for _, branch := range detail.Branches {
		owned = owned || branch.NativeSessionID == t.NativeID
	}
	if !owned {
		return wire.ToolResult{}, errors.New("native session is not a claimed branch of this product")
	}
	var args struct {
		Action    string `json:"action"`
		Query     string `json:"query"`
		After     uint64 `json:"after"`
		HighWater uint64 `json:"high_water"`
		Sequence  uint64 `json:"sequence"`
		Offset    int    `json:"offset"`
		Limit     int    `json:"limit"`
	}

	if len(call.Arguments) > 4096 {
		return wire.ToolResult{}, errors.New("history arguments exceed budget")
	}
	if err = httpjson.ValidateObject(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if err = json.Unmarshal(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	tail, err := t.Store.HistoryBefore(ctx, t.ProductID, 0, 1)
	if err != nil {
		return wire.ToolResult{}, err
	}
	var latest uint64
	if len(tail) > 0 {
		latest = tail[0].Sequence
	}
	if args.HighWater == 0 {
		args.HighWater = latest
	} else if args.HighWater > latest {
		return wire.ToolResult{}, errors.New("high_water cannot exceed the committed product snapshot")
	}
	var payload any
	if args.Action == "read" {
		if args.Sequence == 0 || args.Sequence > args.HighWater || args.Offset < 0 {
			return wire.ToolResult{}, errors.New("invalid history read range")
		}
		if args.Limit == 0 {
			args.Limit = 1024
		}
		if args.Limit < 1 || args.Limit > 2048 {
			return wire.ToolResult{}, errors.New("read limit must be 1..2048 bytes")
		}
		entries, err := t.Store.History(ctx, t.ProductID, args.Sequence-1, 1)
		if err != nil {
			return wire.ToolResult{}, err
		}
		if len(entries) != 1 || entries[0].Sequence != args.Sequence {
			return wire.ToolResult{}, errors.New("owned product event not found")
		}
		body, err := json.Marshal(entries[0])
		if err != nil {
			return wire.ToolResult{}, err
		}
		if args.Offset > len(body) || (args.Offset < len(body) && !utf8.RuneStart(body[args.Offset])) {
			return wire.ToolResult{}, errors.New("read offset must be a UTF-8 boundary")
		}
		end := args.Offset + args.Limit
		if end > len(body) {
			end = len(body)
		}
		for end > args.Offset && end < len(body) && !utf8.RuneStart(body[end]) {
			end--
		}
		payload = map[string]any{"sequence": args.Sequence, "text": string(body[args.Offset:end]), "next_offset": end, "total_bytes": len(body), "truncated": end < len(body), "high_water": args.HighWater}
	} else {
		if args.Action != "list" && args.Action != "search" {
			return wire.ToolResult{}, errors.New("unknown history action")
		}
		if len(args.Query) > 256 || (args.Action == "search" && args.Query == "") {
			return wire.ToolResult{}, errors.New("literal query must contain 1..256 bytes")
		}
		if args.Limit == 0 {
			args.Limit = 5
		}
		if args.Limit < 1 || args.Limit > 10 {
			return wire.ToolResult{}, errors.New("metadata limit must be 1..10")
		}
		entries, err := t.Store.History(ctx, t.ProductID, args.After, 16)
		if err != nil {
			return wire.ToolResult{}, err
		}
		items := []map[string]any{}
		through := args.After
		scanned := 0
		for _, entry := range entries {
			if entry.Sequence > args.HighWater {
				break
			}
			body, err := json.Marshal(entry)
			if err != nil {
				return wire.ToolResult{}, err
			}
			if scanned+len(body) > 128<<10 {
				break
			}
			scanned += len(body)
			through = entry.Sequence
			if args.Action == "search" && !strings.Contains(string(body), args.Query) {
				continue
			}
			excerpt := entry.Input
			if excerpt == "" {
				excerpt = entry.Event.Text
			}
			if len(excerpt) > 128 {
				excerpt = excerpt[:128]
				for !utf8.ValidString(excerpt) {
					excerpt = excerpt[:len(excerpt)-1]
				}
			}
			items = append(items, map[string]any{"sequence": entry.Sequence, "turn": entry.TurnID, "attempt": entry.AttemptID, "kind": entry.Event.Kind, "bytes": len(body), "excerpt": excerpt})
			if len(items) >= args.Limit {
				break
			}
		}
		payload = map[string]any{"items": items, "scanned_through": through, "high_water": args.HighWater, "scanned_bytes": scanned, "more": through < args.HighWater, "processing": "Metadata/excerpts only. Search covers this page's scanned range; explicitly continue after scanned_through. Read is a separate bounded operation."}
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(encoded) > 8192 {
		return wire.ToolResult{}, errors.New("history response exceeds small-output budget")
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(encoded)}}}, nil
}
func (*HistoryTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (*HistoryTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Read bounded product history for this claimed Hub session"}
}
func (*HistoryTool) IsReadOnly(wire.ToolCall) bool { return true }

type HistoryPlugin struct{ Tool *HistoryTool }

func (HistoryPlugin) Name() string { return "hub-history" }
func (p HistoryPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.Tool == nil {
		return errors.New("bound history tool required")
	}
	if err := r.Tool(p.Tool); err != nil {
		return err
	}
	return r.SystemPrompt("hub_history accesses only this claimed product session's committed events across retained engine branches. List/literal-search bounded metadata first, then explicitly read selected ranges. Search pagination is not a full history summary; original files/MCP/native records remain separately owned references.")
}
