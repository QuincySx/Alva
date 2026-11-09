// INPUT: The approval binding JSON before and after Effect.Billable existed
// OUTPUT: Regression that a non-billable Effect serializes exactly as the old shape
// POS: [harness/durable test] Saved approvals bind the serialized Effect; a new field must not orphan them
package durable

import (
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestApprovalBindingOfNonBillableEffectMatchesTheOldShape(t *testing.T) {
	type oldEffect struct {
		Kind        abi.EffectKind
		Description string
		Resources   []abi.ResourceClaim
	}
	type oldBinding struct {
		SessionID, ToolName, ArgumentsSHA256, InputSummary string
		Effect                                             oldEffect
	}
	claims := []abi.ResourceClaim{{Resource: "/w/a", Access: abi.AccessWrite}}
	oldJSON, _ := json.Marshal(oldBinding{"s", "t", "h", "i", oldEffect{abi.EffectWrite, "d", claims}})
	newJSON, _ := json.Marshal(approvalBinding{"s", "t", "h", "i", abi.Effect{Kind: abi.EffectWrite, Description: "d", Resources: claims}})
	if string(oldJSON) != string(newJSON) {
		t.Fatalf("fingerprint input changed:\nold %s\nnew %s", oldJSON, newJSON)
	}
	billable, _ := json.Marshal(approvalBinding{"s", "t", "h", "i", abi.Effect{Kind: abi.EffectWrite, Billable: true}})
	if string(billable) == string(newJSON) {
		t.Fatal("a billable effect must bind differently")
	}
}
