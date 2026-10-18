// INPUT: context, errors, fmt, time, kernel/abi, llm/wire
// OUTPUT: ToolTimeoutMiddleware, DanglingToolCallMiddleware, ErrToolTimeout
// POS: [L2 kernel runtime] Generic loop hygiene without product authorization, persistence or automatic side-effect replay.
package core

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"time"
)

var ErrToolTimeout = errors.New("tool exceeded timeout; waited for actual exit; side effects may have occurred")

type ToolTimeoutMiddleware struct{ Timeout time.Duration }

func (ToolTimeoutMiddleware) Name() string { return "tool-timeout" }
func (middleware ToolTimeoutMiddleware) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	if execution.Scheduling.ManagesOwnTimeout {
		return next(ctx, execution, call)
	}
	if middleware.Timeout <= 0 {
		return wire.ToolResult{}, errors.New("positive tool timeout required")
	}
	deadline, cancel := context.WithTimeout(ctx, middleware.Timeout)
	defer cancel()
	result, err := next(deadline, execution, call)
	if errors.Is(deadline.Err(), context.DeadlineExceeded) {
		return result, errors.Join(ErrToolTimeout, deadline.Err(), err)
	}
	return result, err
}

type DanglingToolCallMiddleware struct{}

func (DanglingToolCallMiddleware) Name() string { return "dangling-tool-call" }
func (DanglingToolCallMiddleware) BeforeModel(_ context.Context, _ abi.RuntimeContext, request *abi.ModelRequest) error {
	results := make(map[string]bool)
	for _, message := range request.Messages {
		for _, block := range message.Content {
			if block.ToolResult != nil {
				results[block.ToolResult.CallID] = true
			}
		}
	}
	var working []wire.Message
	for _, message := range request.Messages {
		working = append(working, message)
		for _, block := range message.Content {
			if block.ToolCall != nil && !results[block.ToolCall.ID] {
				result := wire.ToolResult{CallID: block.ToolCall.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: fmt.Sprintf("Prior tool call %s has no recorded result. It was not replayed; outcome is unknown.", block.ToolCall.Name)}}}
				working = append(working, wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}})
				results[block.ToolCall.ID] = true
			}
		}
	}
	request.Messages = working
	return nil
}
