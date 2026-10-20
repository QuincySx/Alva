// INPUT: encoding/json, unicode/utf8 and kernel/abi.
// OUTPUT: AnswerReceivedError, typed AnswerRecovery and versioned answered-but-unsaved evidence.
// POS: [L4 extension] Existing error settlement retains question correlation without repeating human interaction.
package interaction

import (
	"encoding/json"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"unicode/utf8"
)

// AnswerReceivedError preserves a valid answer's identity when output or
// retention fails. It never implies that evidence was successfully stored.
type AnswerReceivedError struct {
	QuestionID string
	Err        error
}

type AnswerRecovery struct {
	Answered   bool   `json:"answered"`
	Receipt    string `json:"receipt"`
	QuestionID string `json:"question_id"`
	Recovery   string `json:"recovery"`
}

// Recovery is structured recovery evidence; Error text is not a JSON protocol.
func (e *AnswerReceivedError) Recovery() AnswerRecovery {
	id := e.QuestionID
	if len(id) > 512 || !utf8.ValidString(id) {
		id = ""
	}
	return AnswerRecovery{true, "failed", id, "Do not automatically ask again. Inspect the host handler and any question outcome/evidence for this question_id; storage or presentation failed after a valid answer."}
}
func (e *AnswerReceivedError) Error() string {
	raw, _ := json.Marshal(e.Recovery())
	return string(raw)
}
func (e *AnswerReceivedError) RecoveryRecord() abi.SessionRecord {
	data, _ := json.Marshal(e.Recovery())
	return abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.tool.recovery", Type: "answered_receipt_failed", SchemaVersion: 1, Payload: data}
}

func (e *AnswerReceivedError) Unwrap() error { return e.Err }
