// INPUT: Real local HTTP model, native file tools, SDK Guard and shared policy persistence.
// OUTPUT: Actual Auto/Full file execution and Ask approval; scope and effect restrictions remain enforced.
// POS: [L7 native acceptance] Exercises execution behavior rather than merely projecting a menu setting.
package bootstrap

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestNativePermissionPoliciesEnforceRealWrites(t *testing.T) {
	for _, policy := range []hub.PermissionPolicy{hub.PermissionAsk, hub.PermissionAuto, hub.PermissionFull} {
		t.Run(string(policy), func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if calls.Add(1) == 1 {
					waitReply(w, "write_file", `{"path":"actual.txt","content":"explicit next turn"}`, "")
				} else {
					waitReply(w, "", "", "done")
				}
			}))
			defer server.Close()
			config := waitConfig(t, server.URL)
			a, err := Open(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			defer func() { closeApp(t, a) }()
			session, err := a.Service.Create(t.Context(), "policy", "")
			if err != nil {
				t.Fatal(err)
			}
			if err = a.Service.SetPermissionPolicy(t.Context(), session.ID, policy, policy == hub.PermissionFull); err != nil {
				t.Fatal(err)
			}
			closeApp(t, a)
			a, err = Open(t.Context(), config)
			if err != nil {
				t.Fatal(err)
			}
			if calls.Load() != 0 {
				t.Fatal("saving/reopening executed a model")
			}
			if _, err = a.Service.Submit(t.Context(), session.ID, "write now", nil); err != nil {
				t.Fatal(err)
			}
			if policy == hub.PermissionAsk {
				approval := nextApproval(t, a, session.ID)
				if _, err = os.Stat(filepath.Join(session.Workspace, "actual.txt")); !os.IsNotExist(err) {
					t.Fatal("Ask wrote before approval", err)
				}
				if err = a.Service.RespondApproval(t.Context(), session.ID, approval.ID, true); err != nil {
					t.Fatal(err)
				}
			}
			waitStatus(t, a, session.ID, engine.Succeeded)
			data, err := os.ReadFile(filepath.Join(session.Workspace, "actual.txt"))
			if err != nil || string(data) != "explicit next turn" {
				t.Fatal("actual write missing", err)
			}
		})
	}
}

func TestNativeAutoDoesNotApproveCommandsMCPOrUnknownEffects(t *testing.T) {
	var asks int
	deny := security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		asks++
		return security.ApprovalDecision{Allowed: false, Scope: security.Once}, nil
	})
	manager := nativePermissionManager(hub.Settings{PermissionPolicy: hub.PermissionAuto}, deny)
	for _, tc := range []struct {
		name string
		kind abi.EffectKind
	}{
		{"shell_exec", abi.EffectExecute}, {"mcp_write", abi.EffectWrite}, {"hub_team", abi.EffectWrite}, {"write_file", abi.EffectUnknown}, {"web_fetch", abi.EffectNetwork},
	} {
		request := security.ApprovalRequest{SessionID: "native", ToolName: tc.name, Effect: abi.Effect{Kind: tc.kind, Resources: []abi.ResourceClaim{{Resource: "/workspace/file", Access: abi.AccessWrite}}}}
		if err := manager.Authorize(t.Context(), request); !errors.Is(err, security.ErrPermissionDenied) {
			t.Fatal("Auto bypassed explicit approval", tc.name, err)
		}
	}
	if asks != 5 {
		t.Fatal("non-file effects did not reach the real approval owner", asks)
	}
	workspace, err := workspacefs.OpenWorkspace(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer workspace.Close()
	full := nativePermissionManager(hub.Settings{PermissionPolicy: hub.PermissionFull}, deny)
	guard, err := security.NewGuard(workspace.Directory(), nil, workspace, full)
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{filepath.Join(t.TempDir(), "outside"), filepath.Join(workspace.Directory(), ".env")} {
		call := wire.ToolCall{ID: "call", Name: "write_file", Arguments: []byte(`{}`)}
		err = guard.Authorize(t.Context(), abi.ToolExecutionContext{SessionID: "native", CallID: "call"}, call, abi.Effect{Kind: abi.EffectWrite, Resources: []abi.ResourceClaim{{Resource: path, Access: abi.AccessWrite}}})
		if !errors.Is(err, security.ErrPermissionDenied) {
			t.Fatal("Full widened Guard scope", path, err)
		}
	}
}
