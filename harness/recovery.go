// INPUT: context, errors, time, kernel/abi and llm/wire.
// OUTPUT: RecoveryEvidence middleware committing typed post-operation failure evidence.
// POS: [L5 harness] Keeps recovery facts queryable independently of joined human-readable errors.
package harness

import (
	"context"
	"errors"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// RecoveryEvidence consumes an error's optional RecoveryRecord contribution.
// Unlike telemetry, a failed recovery-fact commit remains a fatal SessionError.
func RecoveryEvidence() abi.Middleware {
	return core.WrapTool("recovery-evidence", func(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
		result, err := next(ctx, e, call)
		var provider interface{ RecoveryRecord() abi.SessionRecord }
		if !errors.As(err, &provider) {
			return result, err
		}
		if e.Session == nil {
			return result, errors.Join(err, errors.New("recovery evidence requires actual execution log"))
		}
		record := abi.CloneRecords([]abi.SessionRecord{provider.RecoveryRecord()})[0]
		cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer cancel()
		_, commitErr := e.Session.AppendRecords(cleanup, []abi.SessionRecord{record})
		if commitErr != nil {
			commitErr = &abi.SessionError{Operation: "commit recovery evidence", SessionID: e.SessionID, Err: commitErr}
		}
		return result, errors.Join(err, commitErr)
	})
}
