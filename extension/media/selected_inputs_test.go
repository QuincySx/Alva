// INPUT: Actual native uploaded image, inputs-only Plugin and SDK request lifecycle
// OUTPUT: Default-tool exclusion, one-request image delivery and no run/history replay regressions
// POS: [L4 extension tests] Selected originals reuse existing derivatives, selection and receipt authority
package media

import (
	"context"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestInputsOnlySelectedImagesUseExistingOneRequestMiddleware(t *testing.T) {
	f := newFixture(t, true, true)
	config := f.plugin.config
	config.SelectedInputURIs = []string{f.source.Reference().URI}
	config.InputsOnly = true
	p, err := New(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close(context.Background())
	if p.Tool() != nil {
		t.Fatal("inputs-only plugin exposed media Tool")
	}
	round := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		round++
		want := 0
		if round == 1 {
			want = 1
		}
		if imageCount(request) != want {
			t.Fatal("selected image replayed or was omitted", round, imageCount(request))
		}
		for _, tool := range request.Tools {
			if tool.Name == "hub_media" {
				t.Fatal("inputs-only enabled media Tool")
			}
		}
		for _, message := range request.Messages {
			if message.Role == wire.RoleSystem && strings.Contains(message.Text(), "hub_media explicitly") {
				t.Fatal("inputs-only added media Tool prompt")
			}
		}
		return emitFinal(emit)
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.cwd).Plugin(p).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "explicit selected input")}, func(abi.Event) error { return nil }); err != nil {
			t.Fatal(err)
		}
	}
	messages, err := f.log.Messages(t.Context())
	if err != nil || imageCount(abi.ModelRequest{Messages: messages}) != 0 {
		t.Fatal("images entered saved conversation", err)
	}
	selections, err := f.repo.PageArtifacts(t.Context(), "owner", "actual", "media-selection", "", 0, 0, 4)
	if err != nil || len(selections.Entries) != 1 {
		t.Fatal("selection was replayed across runs", err)
	}
	if f.approvals != 0 {
		t.Fatal("explicit selected inputs enabled background Tool calls")
	}
	config.ImageCapable = false
	if _, err := New(t.Context(), config); err == nil {
		t.Fatal("unknown route admitted image inputs")
	}
}
