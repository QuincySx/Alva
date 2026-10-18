// INPUT: testing, explicit temporary Skill repository and in-memory reference backend
// OUTPUT: Progressive prompt, reference and fixed-session capability regressions
// POS: [L4 extension] Skill Plugin acceptance without script execution or external services
package skillextension

import (
	"context"
	"encoding/json"
	"github.com/QuincySx/Alva/go/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
	"testing"
)

type noModel struct{}

func (noModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}
func TestSkillPluginIsLazyReferenceFirstAndSessionBound(t *testing.T) {
	ctx := context.Background()
	repo, err := skill.Open([]skill.Root{{Path: t.TempDir(), Writable: true}})
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	body := strings.Repeat("hidden long instructions\n", 400)
	if err = repo.Install(ctx, "example", map[string][]byte{"SKILL.md": []byte("---\nname: example\ndescription: compact trigger\n---\n" + body)}); err != nil {
		t.Fatal(err)
	}
	backend, err := agentcontext.NewMemoryReferences("native", 8<<20)
	if err != nil {
		t.Fatal(err)
	}
	plugin := New(repo, "native", backend)
	agent, err := core.NewBuilder(noModel{}, core.NewMemorySession("native")).Plugin(plugin).Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if agent == nil {
		t.Fatal("assembly failed")
	}
	call := wire.ToolCall{ID: "c", Name: "skill", Arguments: json.RawMessage(`{"name":"example"}`)}
	result, err := plugin.tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "native", ActualTool: plugin.tool, CallID: call.ID}, call)
	if err != nil || len(result.Content) != 1 || result.Content[0].Reference == nil {
		t.Fatalf("large load must be reference: %+v %v", result, err)
	}
	page, err := backend.ReadText(ctx, result.Content[0].Reference.URI, 0, 4096)
	if err != nil || !strings.Contains(page.Text, "hidden long instructions") {
		t.Fatal("reference text missing")
	}
	if _, err = plugin.tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "child"}, call); err == nil {
		t.Fatal("child borrowed parent skill capability")
	}
}
