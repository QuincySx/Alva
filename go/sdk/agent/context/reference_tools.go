// INPUT: bytes, context, encoding/json, errors, io, kernel/abi, llm/wire
// OUTPUT: NewReferenceReadTool, NewReferenceSearchTool
// POS: [L3 capability library] Actual-session-bound retrieval tools with strict arguments and bounded encoded results.
package agentcontext

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"io"
)

type referenceTool struct {
	backend   ReferenceBackend
	sessionID string
	search    bool
}

func NewReferenceReadTool(backend ReferenceBackend, sessionID string) abi.Tool {
	return referenceTool{backend: backend, sessionID: sessionID}
}
func NewReferenceSearchTool(backend ReferenceBackend, sessionID string) abi.Tool {
	return referenceTool{backend: backend, sessionID: sessionID, search: true}
}
func (tool referenceTool) Name() string {
	if tool.search {
		return "context_search"
	}
	return "context_read"
}
func (tool referenceTool) Description() string {
	if tool.search {
		return "Literal search within one existing session text reference; bounded snippets, not a global filesystem/history search."
	}
	return "Read a bounded UTF-8 byte range from one existing session text reference; no file opening, network, media processing, or execution replay."
}
func (tool referenceTool) ParametersSchema() json.RawMessage {
	if tool.search {
		return json.RawMessage(`{"type":"object","properties":{"uri":{"type":"string"},"query":{"type":"string","maxLength":1024},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["uri","query"],"additionalProperties":false}`)
	}
	return json.RawMessage(`{"type":"object","properties":{"uri":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":4096}},"required":["uri"],"additionalProperties":false}`)
}
func (tool referenceTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if tool.sessionID == "" || execution.SessionID != tool.sessionID {
		return wire.ToolResult{}, errors.New("reference does not belong to this execution session")
	}
	if tool.backend == nil {
		return wire.ToolResult{}, errors.New("reference backend unavailable")
	}
	var payload []byte
	if tool.search {
		var args struct {
			URI   string `json:"uri"`
			Query string `json:"query"`
			Limit int    `json:"limit"`
		}
		if err := decodeArguments(call.Arguments, &args); err != nil {
			return wire.ToolResult{}, err
		}
		if args.Limit == 0 {
			args.Limit = 10
		}
		matches, err := (ReferenceReader{Backend: tool.backend}).Search(ctx, args.URI, args.Query, args.Limit)
		if err != nil {
			return wire.ToolResult{}, err
		}
		response := struct {
			URI       string      `json:"uri"`
			Matches   []TextMatch `json:"matches"`
			Truncated bool        `json:"truncated"`
		}{URI: args.URI, Matches: matches, Truncated: len(matches) == args.Limit}
		for {
			payload, err = json.Marshal(response)
			if err != nil {
				return wire.ToolResult{}, err
			}
			if len(payload) <= MaxTextPageBytes {
				break
			}
			if len(response.Matches) == 0 {
				return wire.ToolResult{}, errors.New("reference metadata exceeds output budget")
			}
			response.Matches = response.Matches[:len(response.Matches)-1]
			response.Truncated = true
		}
	} else {
		var args struct {
			URI    string `json:"uri"`
			Offset int64  `json:"offset"`
			Limit  int    `json:"limit"`
		}
		if err := decodeArguments(call.Arguments, &args); err != nil {
			return wire.ToolResult{}, err
		}
		if args.Limit == 0 {
			args.Limit = 2048
		}
		for {
			page, err := (ReferenceReader{Backend: tool.backend}).Read(ctx, args.URI, args.Offset, args.Limit)
			if err != nil {
				return wire.ToolResult{}, err
			}
			payload, err = json.Marshal(page)
			if err != nil {
				return wire.ToolResult{}, err
			}
			if len(payload) <= MaxTextPageBytes {
				break
			}
			if args.Limit <= 4 {
				return wire.ToolResult{}, errors.New("reference metadata exceeds output budget")
			}
			args.Limit /= 2
		}
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(payload)}}}, nil
}
func decodeArguments(raw json.RawMessage, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return errors.New("invalid reference tool arguments")
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return errors.New("trailing reference tool arguments")
	}
	return nil
}
func (referenceTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (referenceTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Bounded session reference text retrieval"}
}
func (referenceTool) IsReadOnly(wire.ToolCall) bool { return true }
