// INPUT: encoding/json, errors, fmt, unicode/utf8, llm/wire
// OUTPUT: SessionRecord.Validate, CloneRecords
// POS: [L1 kernel contracts] Durable union validation and deep ownership copies shared by native and memory stores.
package abi

import (
	"encoding/json"
	"errors"
	"fmt"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func (r SessionRecord) Validate() error {
	if r.Namespace == "" || r.Type == "" || r.SchemaVersion == 0 {
		return errors.New("record envelope metadata missing")
	}
	if !utf8.ValidString(r.Namespace) || !utf8.ValidString(r.Type) {
		return errors.New("record envelope metadata must be valid UTF-8")
	}
	if r.Kind != RecordExtension && (r.Namespace != "alva.kernel" || r.Type != string(r.Kind) || r.SchemaVersion != 1) {
		return errors.New("unsupported built-in record envelope")
	}
	n := 0
	if r.Message != nil {
		n++
	}
	if r.ModelIntent != nil {
		n++
	}
	if r.ModelOutcome != nil {
		n++
	}
	if r.ToolIntent != nil {
		n++
	}
	if r.ToolOutcome != nil {
		n++
	}
	if len(r.Payload) != 0 {
		n++
	}
	if n != 1 {
		return errors.New("session record must contain exactly one payload")
	}
	switch r.Kind {
	case RecordConversation:
		if r.Message == nil {
			return errors.New("conversation payload missing")
		}
		return r.Message.Validate()
	case RecordModelIntent:
		if r.ModelIntent == nil || r.ModelIntent.RequestID == "" {
			return errors.New("model intent identity missing")
		}
	case RecordModelOutcome:
		if r.ModelOutcome == nil || r.ModelOutcome.RequestID == "" || !validStatus(r.ModelOutcome.Status) {
			return errors.New("model outcome identity/status missing")
		}
	case RecordToolIntent:
		if r.ToolIntent == nil || r.ToolIntent.RunID == "" || r.ToolIntent.Call.ID == "" || r.ToolIntent.Call.Name == "" || !json.Valid(r.ToolIntent.Call.Arguments) {
			return errors.New("tool intent call invalid")
		}
		if err := (wire.ContentBlock{Kind: wire.ContentToolCall, ToolCall: &r.ToolIntent.Call}).Validate(); err != nil {
			return err
		}
		if declared := r.ToolIntent.DeclaredCall; declared != nil {
			if declared.ID != r.ToolIntent.Call.ID || declared.Name != r.ToolIntent.Call.Name {
				return errors.New("prepared tool identity differs from model declaration")
			}
			if err := (wire.ContentBlock{Kind: wire.ContentToolCall, ToolCall: declared}).Validate(); err != nil {
				return err
			}
		}
		if err := r.ToolIntent.Scheduling.Validate(); err != nil {
			return err
		}
	case RecordToolOutcome:
		if r.ToolOutcome == nil || r.ToolOutcome.RunID == "" || r.ToolOutcome.CallID == "" || r.ToolOutcome.Result.CallID != r.ToolOutcome.CallID || !validStatus(r.ToolOutcome.Status) {
			return errors.New("tool outcome identity/status invalid")
		}
		if err := (wire.ContentBlock{Kind: wire.ContentToolResult, ToolResult: &r.ToolOutcome.Result}).Validate(); err != nil {
			return err
		}
	case RecordExtension:
		if r.Namespace == "alva.kernel" || !wire.ValidJSON(r.Payload) {
			return errors.New("extension requires its own namespace and valid JSON")
		}
	default:
		return fmt.Errorf("unknown session record kind %q", r.Kind)
	}
	return nil
}

// BuiltinRecord constructs the current built-in envelope; this does not decode
// or translate historical record formats. Extension envelopes are explicit.
func BuiltinRecord(record SessionRecord) SessionRecord {
	if record.Kind != RecordExtension {
		record.Namespace = "alva.kernel"
		record.Type = string(record.Kind)
		record.SchemaVersion = 1
	}
	return record
}

func validStatus(status OutcomeStatus) bool {
	return status == OutcomeSucceeded || status == OutcomeFailed || status == OutcomeCancelled
}

func CloneRecords(records []SessionRecord) []SessionRecord {
	if records == nil {
		return nil
	}
	out := make([]SessionRecord, len(records))
	for i, record := range records {
		out[i] = record
		out[i].Payload = append(json.RawMessage(nil), record.Payload...)
		if record.Parent != nil {
			parent := *record.Parent
			out[i].Parent = &parent
		}
		if record.Message != nil {
			message := wire.CloneMessages([]wire.Message{*record.Message})[0]
			out[i].Message = &message
		}
		if record.ModelIntent != nil {
			intent := *record.ModelIntent
			out[i].ModelIntent = &intent
		}
		if record.ModelOutcome != nil {
			outcome := *record.ModelOutcome
			if outcome.Usage != nil {
				usage := wire.CloneUsage(*outcome.Usage)
				outcome.Usage = &usage
			}
			out[i].ModelOutcome = &outcome
		}
		if record.ToolIntent != nil {
			intent := *record.ToolIntent
			intent.Call = wire.CloneToolCall(intent.Call)
			if intent.DeclaredCall != nil {
				declared := wire.CloneToolCall(*intent.DeclaredCall)
				intent.DeclaredCall = &declared
			}
			intent.Effect.Resources = append([]ResourceClaim(nil), intent.Effect.Resources...)
			intent.Scheduling.Resources = append([]ResourceClaim(nil), intent.Scheduling.Resources...)
			out[i].ToolIntent = &intent
		}
		if record.ToolOutcome != nil {
			outcome := *record.ToolOutcome
			outcome.Result.Content = wire.CloneContent(outcome.Result.Content)
			out[i].ToolOutcome = &outcome
		}
	}
	return out
}
