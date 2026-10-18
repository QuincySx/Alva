// INPUT: llm/wire
// OUTPUT: CloneEvent
// POS: [L1 kernel contracts] Deep ownership copy for queued execution events and detached subscribers.
package abi

import (
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func CloneEvent(event Event) Event {
	copy := event
	if event.Message != nil {
		value := wire.CloneMessages([]wire.Message{*event.Message})[0]
		copy.Message = &value
	}
	if event.ToolCall != nil {
		call := wire.CloneToolCall(*event.ToolCall)
		copy.ToolCall = &call
	}
	if event.ContentBlock != nil {
		block := wire.CloneContent([]wire.ContentBlock{*event.ContentBlock})[0]
		copy.ContentBlock = &block
	}
	if event.ToolResult != nil {
		result := *event.ToolResult
		result.Content = wire.CloneContent(result.Content)
		copy.ToolResult = &result
	}
	if event.Record != nil {
		reference := *event.Record
		copy.Record = &reference
	}
	if event.Usage != nil {
		usage := wire.CloneUsage(*event.Usage)
		copy.Usage = &usage
	}
	return copy
}
