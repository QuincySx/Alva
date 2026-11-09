// INPUT: Actual product/native fixture, explicit Guard handler and claimed native invocation
// OUTPUT: Team Tool binding, small index and mutation HITL regressions
// POS: [L7 Hub product] Visible Worker creation is never inferred from native subagent permission or a Tool name
package product

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestTeamToolActualBindingAndFailClosedMutation(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	p, err := s.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(ctx, p.ID, "bind native fixture", nil); err != nil {
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
	workspace, err := workspacefs.OpenWorkspace(p.Workspace)
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

func TestTeamToolPagesManyDynamicRoutesWithStableAbsoluteIndices(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	var err error
	s.Connections, err = NewConnectionsService(ctx, s.Store.(hub.ConnectionsStore))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Connections.Close(ctx)
	update := hub.ConnectionUpdate{Name: "Explicit many-model fixture", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:12345/v1/chat/completions", Auth: "none", Enabled: true}
	for i := range 64 {
		update.Models = append(update.Models, hub.ConnectionModel{ID: fmt.Sprintf("accurate-model-%02d", i), Enabled: true})
	}
	saved, err := s.Connections.Save(ctx, update)
	if err != nil {
		t.Fatal(err)
	}
	// The five bounded Target identity fields can each expand sixfold in JSON.
	// Even that longest legal identity must fit a page and advance the cursor.
	longest := strings.Repeat("\x01", 256)
	longRoute := hub.Target{EngineID: longest, ModelID: longest, ProviderID: longest, AccountSource: longest, BillingRoute: longest, RequiredCapabilities: []string{strings.Repeat("capability-metadata", 1024)}}
	if err = longRoute.Validate(); err != nil {
		t.Fatal(err)
	}
	s.Targets = append(s.Targets, longRoute)
	expected := s.AvailableTargets()
	lead, err := s.Create(ctx, "Lead", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(ctx, lead.ID, "bind native fixture", nil); err != nil {
		t.Fatal(err)
	}
	<-requests
	waitIdle(t, s, lead.ID)
	detail, err := s.Store.Detail(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	initialWorker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "existing-roster", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	workspace, err := workspacefs.OpenWorkspace(lead.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	defer workspace.Close()
	approvals := 0
	var approvedEffect string
	guard, err := security.NewGuard(lead.Workspace, nil, workspace, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(_ context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
		approvals++
		approvedEffect = request.Effect.Description
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	tool := &TeamTool{Service: s, ProductID: lead.ID, NativeID: branch.NativeSessionID, Guard: guard}
	callSequence := 0
	invoke := func(arguments map[string]any) (wire.ToolResult, error) {
		callSequence++
		data, e := json.Marshal(arguments)
		if e != nil {
			t.Fatal(e)
		}
		call := wire.ToolCall{ID: fmt.Sprintf("route-call-%d", callSequence), Name: tool.Name(), Arguments: data}
		return tool.Execute(ctx, abi.ToolExecutionContext{SessionID: branch.NativeSessionID, RunID: "actual-route-pages", CallID: call.ID, ActualTool: tool}, call)
	}
	readResult := func(arguments map[string]any) string {
		result, e := invoke(arguments)
		if e != nil {
			t.Fatal(e)
		}
		if len(result.Content) != 1 || len(result.Content[0].Text) > 8<<10 {
			t.Fatal("Team result exceeded the unchanged 8 KiB budget")
		}
		return result.Content[0].Text
	}
	var roster struct {
		Workers    []hub.Worker `json:"workers"`
		RouteCount int          `json:"configured_route_count"`
	}
	if err = json.Unmarshal([]byte(readResult(map[string]any{"action": "list"})), &roster); err != nil || len(roster.Workers) != 1 || roster.Workers[0].ID != initialWorker.ID || roster.RouteCount != len(expected) {
		t.Fatal("many routes obscured the actual roster", err)
	}
	probe := wire.ToolCall{Arguments: []byte(`{"action":"routes"}`)}
	if !tool.IsReadOnly(probe) || tool.DescribeEffect(probe).Kind != abi.EffectRead || !strings.Contains(string(tool.ParametersSchema()), `"routes"`) {
		t.Fatal("routes schema/effect/read-only declarations disagree")
	}
	var all []teamRouteIndex
	for after := 0; ; {
		var page teamRoutePage
		if err = json.Unmarshal([]byte(readResult(map[string]any{"action": "routes", "after": after})), &page); err != nil {
			t.Fatal(err)
		}
		if page.Start != after || page.Total != len(expected) || page.Next != after+len(page.Routes) || page.Next <= after || len(page.Routes) > 4 {
			t.Fatal("route page did not make bounded contiguous progress", page.Start, page.Next)
		}
		for i, route := range page.Routes {
			absolute := after + i
			want := expected[absolute]
			if route.TargetIndex != absolute || route.EngineID != want.EngineID || route.ModelID != want.ModelID || route.ProviderID != want.ProviderID || route.AccountSource != want.AccountSource || route.BillingRoute != want.BillingRoute {
				t.Fatal("route identity or absolute index changed")
			}
		}
		all = append(all, page.Routes...)
		if !page.HasMore {
			break
		}
		after = page.Next
	}
	if len(all) != 66 || approvals != 0 {
		t.Fatal("did not enumerate every real configured route without mutation approval")
	}
	var exhausted teamRoutePage
	if err = json.Unmarshal([]byte(readResult(map[string]any{"action": "routes", "after": len(expected)})), &exhausted); err != nil || exhausted.HasMore || len(exhausted.Routes) != 0 || exhausted.Next != len(expected) {
		t.Fatal("exhausted route cursor was not terminal", err)
	}
	if _, err = invoke(map[string]any{"action": "routes", "after": len(expected) + 1}); err == nil {
		t.Fatal("out-of-bounds route cursor accepted")
	}
	bad := wire.ToolCall{ID: "duplicate-route-fields", Name: tool.Name(), Arguments: []byte(`{"action":"routes","after":0,"after":1}`)}
	if _, err = tool.Execute(ctx, abi.ToolExecutionContext{SessionID: branch.NativeSessionID, CallID: bad.ID, ActualTool: tool}, bad); err == nil {
		t.Fatal("routes bypassed strict duplicate-field validation")
	}
	selected := all[len(all)-1]
	var added hub.Worker
	if err = json.Unmarshal([]byte(readResult(map[string]any{"action": "add", "team_id": team.Team.ID, "name": "absolute-route-worker", "target_index": selected.TargetIndex})), &added); err != nil {
		t.Fatal(err)
	}
	workerDetail, err := s.Store.Detail(ctx, added.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	workerBranch, err := ActiveBranch(workerDetail)
	if err != nil || hub.RouteKey(workerBranch.Target) != hub.RouteKey(expected[selected.TargetIndex]) || approvals != 1 || !strings.Contains(approvedEffect, selected.ModelID) || !strings.Contains(approvedEffect, selected.AccountSource) {
		t.Fatal("Add used a page-relative route or described a different account", err)
	}
	edit := connectionEdit(saved)
	edit.Name = "Revision changed after route selection"
	if saved, err = s.Connections.Save(ctx, edit); err != nil {
		t.Fatal(err)
	}
	var stable teamRoutePage
	if err = json.Unmarshal([]byte(readResult(map[string]any{"action": "routes", "after": selected.TargetIndex})), &stable); err != nil || len(stable.Routes) != 1 || stable.Routes[0] != selected {
		t.Fatal("connection edit reassigned an already described route index", err)
	}
	// A rename keeps the route valid; disabling the connection makes it stale.
	disable := connectionEdit(saved)
	disable.Enabled = false
	if _, err = s.Connections.Save(ctx, disable); err != nil {
		t.Fatal(err)
	}
	if _, err = invoke(map[string]any{"action": "add", "team_id": team.Team.ID, "name": "stale-route-worker", "target_index": selected.TargetIndex}); err == nil {
		t.Fatal("stale configured route bypassed current-route admission")
	}
	actualTeam, err := s.Teams.Store.TeamForLead(ctx, lead.ID)
	if err != nil || len(actualTeam.Workers) != 2 || approvals != 2 {
		t.Fatal("stale route mutation created a Worker or skipped explicit approval", err)
	}
}
