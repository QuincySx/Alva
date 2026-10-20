// INPUT: Actual pure SDK model wrappers, malformed prepared values and frozen model requests.
// OUTPUT: Invalid wire input cannot reach a model even without optional telemetry.
// POS: [L2.5 SDK testing] General request integrity remains an execution invariant after extracting diagnostic encoders.
package core_test

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestInvalidActualRequestFailsWithoutTelemetry(t *testing.T) {
	for _, location := range []string{"before", "wrapper"} {
		t.Run(location, func(t *testing.T) {
			called := 0
			m := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { called++; return nil })
			bad := func(r *abi.ModelRequest) {
				r.Tools = []wire.ToolDefinition{{Name: "broken", Parameters: json.RawMessage(`{"type":`)}}
			}
			b := core.NewBuilder(m, core.NewMemorySession("request-integrity"))
			if location == "before" {
				b.Middleware(core.BeforeModel("bad", func(_ context.Context, _ abi.RuntimeContext, r *abi.ModelRequest) error { bad(r); return nil }))
			} else {
				b.Middleware(core.WrapModel("bad", func(ctx context.Context, runtime abi.RuntimeContext, r abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
					bad(&r)
					return next(ctx, runtime, r, emit)
				}))
			}
			if err := build(t, b).Run(t.Context(), nil, discard); err == nil || called != 0 {
				t.Fatal("invalid transport request entered model", err, called)
			}
		})
	}
}
