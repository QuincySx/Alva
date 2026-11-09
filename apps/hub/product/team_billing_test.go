// INPUT: hub_team DescribeEffect for read and mutating actions
// OUTPUT: Regression that every non-read Team action declares Billable so no permission tier auto-approves it
// POS: [L7 Hub product test] Worker creation can incur metered cost; the tool declares it instead of the host matching its name
package product

import (
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestTeamToolDeclaresMutationsBillable(t *testing.T) {
	tool := &TeamTool{}
	for _, action := range []string{"add", "assign", "stop", "remove"} {
		effect := tool.DescribeEffect(wire.ToolCall{ID: "c", Name: "hub_team", Arguments: []byte(`{"action":"` + action + `"}`)})
		if !effect.Billable || effect.Kind == abi.EffectRead {
			t.Fatalf("%s must be a billable non-read effect: %+v", action, effect)
		}
	}
	if effect := tool.DescribeEffect(wire.ToolCall{ID: "c", Name: "hub_team", Arguments: []byte(`{"action":"list"}`)}); effect.Billable || effect.Kind != abi.EffectRead {
		t.Fatalf("a read must not be billable: %+v", effect)
	}
}
