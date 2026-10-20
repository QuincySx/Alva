// INPUT: errors, strings, kernel/abi and llm/wire.
// OUTPUT: Child response selection from committed facts with explicitly partial failure drafts.
// POS: [L4 extension] Selects effective final text/refusal without leaking reasoning or tool-call prefaces.
package subagent

import (
	"errors"
	"strings"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type childOutput struct {
	sessionID          string
	text               strings.Builder
	committed, refused bool
	nonText            int
}

func (o *childOutput) reset() {
	o.text.Reset()
	o.committed, o.refused, o.nonText = false, false, 0
}
func (o *childOutput) append(text string) error {
	if o.text.Len()+len(text) > 1<<20 {
		return errors.New("selected child output exceeds 1 MiB")
	}
	o.text.WriteString(text)
	return nil
}
func (o *childOutput) block(block wire.ContentBlock) error {
	switch block.Kind {
	case wire.ContentText:
		return o.append(block.Text)
	case wire.ContentRefusal:
		if block.Refusal != nil {
			o.refused = true
			return o.append("[Refusal] " + block.Refusal.Text)
		}
	case wire.ContentReasoning, wire.ContentProviderState:
	default:
		o.nonText++
	}
	return nil
}
func (o *childOutput) observe(event abi.Event) error {
	switch event.Kind {
	case abi.EventToolStarted:
		o.reset()
	case abi.EventMessageCommitted:
		message, record := event.Message, event.Record
		if message == nil || message.Role != wire.RoleAssistant || record == nil || record.SessionID != o.sessionID || record.RecordID == "" || record.Sequence == 0 || message.ID != record.RecordID {
			return errors.New("child final event lacks actual committed assistant identity")
		}
		o.reset()
		for _, block := range message.Content {
			if block.Kind == wire.ContentToolCall {
				return nil
			}
		}
		for _, block := range message.Content {
			if err := o.block(block); err != nil {
				return err
			}
		}
		o.committed = true
	case abi.EventTextDelta:
		if o.committed {
			o.reset()
		}
		return o.append(event.Text)
	case abi.EventContentBlock:
		if o.committed {
			o.reset()
		}
		if event.ContentBlock != nil {
			return o.block(*event.ContentBlock)
		}
	}
	return nil
}
