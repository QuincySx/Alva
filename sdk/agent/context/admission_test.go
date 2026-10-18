// INPUT: context, encoding/json, strings, testing, kernel/abi, llm/wire
// OUTPUT: Final request budget policy and bound-session estimation regressions
// POS: [L3 context policy tests] Rejects late text/tool/media estimates without confusing mutable next-session state.
package agentcontext

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type boundBudgetHandle struct {
	*Store
	seen string
}

func (h *boundBudgetHandle) Budget(id string) abi.BudgetInfo {
	h.seen = id
	if id == "bound" {
		return abi.BudgetInfo{BudgetTokens: 100}
	}
	return abi.BudgetInfo{BudgetTokens: 100000}
}

func TestAdmissionCountsFinalToolsAndLateContent(t *testing.T) {
	policy := BudgetAdmission{System: &abi.ContextSystem{BudgetTokens: 100}}
	for name, q := range map[string]abi.ModelRequest{
		"text":  {Messages: []wire.Message{wire.TextMessage(wire.RoleUser, strings.Repeat("late", 256))}},
		"tools": {Tools: []wire.ToolDefinition{{Name: "large", Parameters: json.RawMessage(`{"description":"` + strings.Repeat("large", 256) + `"}`)}}},
		"media": {Messages: []wire.Message{{Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentImage, Image: &wire.Image{Payload: wire.MediaPayload{MediaType: "image/jpeg", Data: []byte(strings.Repeat("bytes", 256))}}}}}}},
	} {
		t.Run(name, func(t *testing.T) {
			if err := policy.AdmitModel(t.Context(), abi.RuntimeContext{}, q); err == nil {
				t.Fatal("final addition escaped configured estimate")
			}
		})
	}
	if err := policy.AdmitModel(t.Context(), abi.RuntimeContext{}, abi.ModelRequest{}); err != nil {
		t.Fatal(err)
	}
}

func TestAdmissionUsesInvocationSessionBudget(t *testing.T) {
	handle := &boundBudgetHandle{Store: NewStore(0, 0, nil, nil)}
	policy := BudgetAdmission{System: &abi.ContextSystem{Handle: handle}}
	q := abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleUser, strings.Repeat("x", 1000))}}
	if err := policy.AdmitModel(context.Background(), abi.RuntimeContext{InvocationSessionID: "bound"}, q); err == nil || handle.seen != "bound" {
		t.Fatal("bound budget ignored", err, handle.seen)
	}
}

func TestAdmissionNoFiniteBudgetMakesNoHardLimitClaim(t *testing.T) {
	policy := BudgetAdmission{System: &abi.ContextSystem{Handle: NewStore(0, 0, nil, nil)}}
	if err := policy.AdmitModel(t.Context(), abi.RuntimeContext{InvocationSessionID: "bound"}, abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleUser, strings.Repeat("x", 1000))}}); err != nil {
		t.Fatal(err)
	}
	var counter *missingCounter
	policy.System.Counter = counter
	if err := policy.AdmitModel(t.Context(), abi.RuntimeContext{InvocationSessionID: "bound"}, abi.ModelRequest{}); err == nil {
		t.Fatal("typed nil counter accepted")
	}
}

type missingCounter struct{}

func (*missingCounter) CountTokens(string) int { panic("typed nil called") }
func (*missingCounter) ContextWindow() int     { panic("typed nil called") }
