// INPUT: Approval requests from the root native session and a child session with the same call ID
// OUTPUT: Regression that a child's call never borrows the root attempt's tool display
// POS: [L7 Hub product test] Settled approval lines stay attributed to the session that asked
package product

import (
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/security"
)

func TestSettledCallIDIgnoresChildSessionCalls(t *testing.T) {
	root := security.ApprovalRequest{SessionID: "root", CallID: "c1"}
	child := security.ApprovalRequest{SessionID: "child", CallID: "c1"}
	if settledCallID(root, "root") != "c1" || settledCallID(child, "root") != "" {
		t.Fatal("a child session's call ID must not select the root's tool display")
	}
}
