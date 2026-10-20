// INPUT: encoding/json, errors, unicode/utf8 and kernel/abi.
// OUTPUT: CommittedError, typed CommittedReceipt and versioned recovery facts for a known completed mutation.
// POS: [L4 extension] Recovery information survives the existing kernel error path without replay or kernel changes.
package planning

import (
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"unicode/utf8"
)

// CommittedError reports a known committed business operation, not an atomic
// failure or permission to retry. Err remains available to the host via Unwrap.
// A backend may return this error after committing even if its own flush fails.
type CommittedError struct {
	Operation string
	CallID    string
	TaskID    string
	Err       error
}

// CommittedReceipt is the versioned recovery data. Error() remains ordinary
// text and must not be parsed after errors.Join or middleware wrapping.
type CommittedReceipt struct {
	State     string `json:"business_state"`
	Receipt   string `json:"receipt"`
	Operation string `json:"operation"`
	CallID    string `json:"call_id"`
	TaskID    string `json:"task_id,omitempty"`
	Recovery  string `json:"recovery"`
}

func (e *CommittedError) Recovery() CommittedReceipt {
	// Do not include business content or backend errors in the model receipt.
	identity := func(s string) string {
		if len(s) > 512 || !utf8.ValidString(s) {
			return ""
		}
		return s
	}
	return CommittedReceipt{"committed", "failed", identity(e.Operation), identity(e.CallID), identity(e.TaskID),
		"Do not repeat the mutation. Query task_get using task_id, or task_list if absent. For todo_write, inspect the host plan store. Inspect the host error for the receipt failure."}
}
func (e *CommittedError) Error() string {
	data, _ := json.Marshal(e.Recovery())
	return string(data)
}
func (e *CommittedError) RecoveryRecord() abi.SessionRecord {
	data, _ := json.Marshal(e.Recovery())
	return abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.tool.recovery", Type: "committed_receipt_failed", SchemaVersion: 1, Payload: data}
}

func (e *CommittedError) Unwrap() error { return e.Err }

func correlateCommit(err error, operation, callID string) error {
	var committed *CommittedError
	if errors.As(err, &committed) {
		return &CommittedError{Operation: operation, CallID: callID, TaskID: committed.TaskID, Err: err}
	}
	return err
}
