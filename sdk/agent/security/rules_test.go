// INPUT: context, errors, strings, testing, kernel/abi
// OUTPUT: Actual permission rule precedence, immutable snapshot and approval binding regressions
// POS: [L3 capability library] Explicit caller policy does not become automatic dynamic-tool read authority or bypass Plan.
package security

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"strings"
	"testing"
)

func TestPermissionRulePrecedenceAndAnchoredInputPatterns(t *testing.T) {
	rules := PermissionRules{Allow: []string{"shell(git *)"}, Ask: []string{"shell(git push*)"}, Deny: []string{"shell(git push --force*)"}, ReadOnly: []string{"mcp:docs:*"}}
	for _, test := range []struct {
		input string
		want  RuleDecision
	}{{"git status", RulesAllow}, {"git push", RulesAsk}, {"git push --force origin main", RulesDeny}, {"echo git status", RulesAsk}} {
		if got := rules.Check("shell", test.input); got != test.want {
			t.Fatal(test, got)
		}
	}
	if rules.Check("other", "git status") != RulesAsk || !rules.IsReadOnly("mcp:docs:lookup") || rules.IsReadOnly("mcp:other:write") {
		t.Fatal("pattern scope")
	}
}
func TestRulesApplyBeforeCacheAndReadShortcutButNeverOverridePlan(t *testing.T) {
	calls := 0
	manager := NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		calls++
		return ApprovalDecision{Allowed: true, Scope: Session}, nil
	}), PermissionRules{Ask: []string{"shell(sensitive)"}, Deny: []string{"read(secret)"}})
	request := ApprovalRequest{SessionID: "session", ToolName: "shell", Effect: abi.Effect{Kind: abi.EffectExecute}, InputSummary: "ordinary"}
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	request.InputSummary = "sensitive"
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	if calls != 2 {
		t.Fatal("cached Session grant bypassed explicit Ask", calls)
	}
	request.ToolName = "read"
	request.InputSummary = "secret"
	request.Effect.Kind = abi.EffectRead
	if err := manager.Authorize(context.Background(), request); !errors.Is(err, ErrPermissionDenied) {
		t.Fatal("read shortcut bypassed explicit deny", err)
	}
	plan := NewPermissionManager(Plan, nil, PermissionRules{Allow: []string{"*"}})
	request.Effect.Kind = abi.EffectWrite
	if err := plan.Authorize(context.Background(), request); !errors.Is(err, ErrPermissionDenied) {
		t.Fatal("allow rule bypassed Plan", err)
	}
	unknown := NewPermissionManager(Ask, nil, PermissionRules{ReadOnly: []string{"mcp:*"}})
	request.ToolName = "mcp:arbitrary"
	request.Effect.Kind = abi.EffectUnknown
	if err := unknown.Authorize(context.Background(), request); !errors.Is(err, ErrApprovalRequired) {
		t.Fatal("read-only metadata granted unknown tool execution", err)
	}
}
func TestRulesSnapshotInputBoundsAndOnceBindsActualInput(t *testing.T) {
	rules := PermissionRules{Allow: []string{"shell(safe)"}}
	manager := NewPermissionManager(Ask, nil, rules)
	rules.Allow[0] = "*"
	request := ApprovalRequest{SessionID: "s", ToolName: "shell", Effect: abi.Effect{Kind: abi.EffectExecute}, InputSummary: "unsafe"}
	if err := manager.Authorize(context.Background(), request); !errors.Is(err, ErrApprovalRequired) {
		t.Fatal("caller mutated rule snapshot", err)
	}
	for _, input := range []string{string([]byte{0xff}), strings.Repeat("x", 256<<10+1)} {
		request.InputSummary = input
		if err := manager.Authorize(context.Background(), request); err == nil {
			t.Fatal("invalid summary accepted")
		}
	}
	calls := 0
	manager = NewPermissionManager(Ask, ApprovalFunc(func(context.Context, ApprovalRequest) (ApprovalDecision, error) {
		calls++
		return ApprovalDecision{Allowed: true, Scope: Once}, nil
	}))
	request = approvalFixture()
	request.InputSummary = "first"
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	request.InputSummary = "second"
	if err := manager.Authorize(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	if calls != 2 {
		t.Fatal("changed actual input reused Once grant", calls)
	}
}
