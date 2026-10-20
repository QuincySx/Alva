// INPUT: Actual planning writer/timeout, typed recovery records, memory SDK and kernel/abi/wire.
// OUTPUT: Recovery facts survive joined timeout text without repeating a committed mutation.
// POS: [L5 harness testing] Validates the persistent recovery channel independently from human-readable error formatting.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/planning"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestRecoveryRecordSurvivesJoinedTimeout(t *testing.T) {
	writes := 0
	tool, err := planning.NewPlanTool(func(ctx context.Context, _ string) (*wire.Reference, error) { writes++; <-ctx.Done(); return nil, nil })
	if err != nil {
		t.Fatal(err)
	}
	log := core.NewMemorySession("recovery-timeout")
	model := childModelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return childToolReply(emit, "saved", "todo_write", `{"content":"committed plan"}`)
	})
	a, err := core.NewBuilder(model, log).Tool(tool).Middleware(RecoveryEvidence(), -9000).Middleware(kernel.ToolTimeoutMiddleware{Timeout: 10 * time.Millisecond}).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	err = a.Run(t.Context(), nil, func(abi.Event) error { return nil })
	var committed *planning.CommittedError
	if !errors.As(err, &committed) || !errors.Is(err, kernel.ErrToolTimeout) || writes != 1 {
		t.Fatal("commit lost or repeated", err, writes)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, record := range records {
		if record.Namespace != "alva.tool.recovery" {
			continue
		}
		found = true
		var receipt planning.CommittedReceipt
		if record.SchemaVersion != 1 || json.Unmarshal(record.Payload, &receipt) != nil || receipt.State != "committed" || receipt.CallID != "saved" {
			t.Fatal("unstructured recovery", record)
		}
	}
	if !found {
		t.Fatal("recovery facts only existed in Error text")
	}
}
