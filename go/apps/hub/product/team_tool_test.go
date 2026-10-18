// INPUT: Actual product/native fixture, explicit Guard handler and claimed native invocation
// OUTPUT: Team Tool binding, small index and mutation HITL regressions
// POS: [L7 Hub product] Visible Worker creation is never inferred from native subagent permission or a Tool name
package product

import (
	"context"
	"database/sql"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestTeamToolActualBindingAndFailClosedMutation(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(ctx, p.ID, "bind native fixture"); err != nil {
		t.Fatal(err)
	}
	<-requests
	waitIdle(t, s, p.ID)
	detail, err := s.Store.Detail(ctx, p.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	workspace, err := builtin.OpenWorkspace(p.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	defer workspace.Close()
	allowed := false
	approvals := 0
	guard, err := security.NewGuard(p.Workspace, nil, workspace, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(_ context.Context, r security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals++
		if r.SessionID != branch.NativeSessionID || r.Effect.Kind != abi.EffectUnknown {
			t.Fatal("wrong actual mutation authority")
		}
		return security.ApprovalDecision{Allowed: allowed, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	tool := &TeamTool{Service: s, ProductID: p.ID, NativeID: branch.NativeSessionID, Guard: guard}
	execution := abi.ToolExecutionContext{SessionID: branch.NativeSessionID, RunID: "actual-fixture", CallID: "list", ActualTool: tool}
	call := wire.ToolCall{ID: "list", Name: tool.Name(), Arguments: []byte(`{"action":"list"}`)}
	for _, kind := range []string{"missing actual tool", "missing call ID", "duplicate action", "case alias"} {
		bad, input := execution, call
		switch kind {
		case "missing actual tool":
			bad.ActualTool = nil
		case "missing call ID":
			bad.CallID, input.ID = "", ""
		case "duplicate action":
			input.Arguments = []byte(`{"action":"list","action":"list"}`)
		case "case alias":
			input.Arguments = []byte(`{"Action":"list"}`)
		}
		if _, err := tool.Execute(ctx, bad, input); err == nil {
			t.Errorf("%s admitted", kind)
		}
	}
	result, err := tool.Execute(ctx, execution, call)
	if err != nil || len(result.Content[0].Text) > 8<<10 || approvals != 0 {
		t.Fatal("small read index failed", err)
	}
	execution.CallID = "create"
	call.ID = "create"
	call.Arguments = []byte(`{"action":"create"}`)
	if _, err = tool.Execute(ctx, execution, call); err == nil || approvals != 1 {
		t.Fatal("mutation bypassed explicit approval")
	}
	if _, err = s.Teams.Store.TeamForLead(ctx, p.ID); !errors.Is(err, sql.ErrNoRows) {
		t.Fatal("denied Tool created Team", err)
	}
	allowed = true
	execution.CallID = "create-approved"
	call.ID = execution.CallID
	result, err = tool.Execute(ctx, execution, call)
	if err != nil || !strings.Contains(result.Content[0].Text, "lead_session_id") {
		t.Fatal("approved mutation did not create actual Team", err)
	}
	execution.SessionID = "foreign-native"
	if _, err = tool.Execute(ctx, execution, call); err == nil {
		t.Fatal("foreign native invocation accepted")
	}
}
