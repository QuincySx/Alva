// INPUT: context, agent/memory, agent/core, kernel/bus.
// OUTPUT: MemoryPlugin and SystemContextPlugin assembly wrappers.
// POS: [L5 harness] Default harness contributions consume separate SDK capability implementations.
package harness

import (
	"context"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/memory"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
)

type MemoryPlugin struct{ service *memory.Service }

func NewMemoryPlugin() *MemoryPlugin {
	return &MemoryPlugin{service: memory.NewService(memory.NewInMemoryBackend(), memory.NoopEmbeddingProvider{})}
}
func (*MemoryPlugin) Name() string { return "memory" }
func (p *MemoryPlugin) Register(_ context.Context, r *core.Registrar) error {
	return bus.Provide(r.BusWriter(), p.service)
}

type SystemContextPlugin struct{}

func (SystemContextPlugin) Name() string { return "system-context" }
func (SystemContextPlugin) Register(_ context.Context, r *core.Registrar) error {
	return r.SystemPrompt("Alva Go. Persistence, tools and media processing are explicit plugin contributions. Stored references are not raw-byte delivery; retrieve only when relevant. Never claim tools, transformations, approvals or persistence unavailable in this session's actual capability catalog.")
}
