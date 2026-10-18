// INPUT: context, errors, agent/context, agent/core, kernel/abi, llm/wire.
// OUTPUT: ContextRetrievalPlugin with reference-first large-text and unselected-media projection policy.
// POS: [L5 harness] Product policy registers SDK retrieval capabilities against an explicitly bound backend.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

const resultInlineLimit = 4 << 10
const referenceStoreLimit = 8 << 20

// Explicit context budgeting is a replaceable harness default. The pure SDK
// opts in through Middleware(BudgetAdmission), independently of soft preparation.
type contextBudgetPlugin struct{ system *abi.ContextSystem }

func (contextBudgetPlugin) Name() string { return "context-budget" }
func (p contextBudgetPlugin) Register(_ context.Context, r *core.Registrar) error {
	return r.Middleware(agentcontext.BudgetAdmission{System: p.system})
}

type ContextRetrievalPlugin struct {
	sessionID string
	backend   agentcontext.ReferenceBackend
}

func NewContextRetrievalPlugin(sessionID string, backend agentcontext.ReferenceBackend) *ContextRetrievalPlugin {
	return &ContextRetrievalPlugin{sessionID: sessionID, backend: backend}
}
func (*ContextRetrievalPlugin) Name() string { return "context-retrieval" }
func (p *ContextRetrievalPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.sessionID == "" || p.backend == nil {
		return errors.New("explicit actual-session reference backend required")
	}
	if err := r.Middleware(referenceProjection{sessionID: p.sessionID, backend: p.backend}); err != nil {
		return err
	}
	if err := r.Tool(agentcontext.NewReferenceReadTool(p.backend, p.sessionID)); err != nil {
		return err
	}
	return r.Tool(agentcontext.NewReferenceSearchTool(p.backend, p.sessionID))
}

type referenceProjection struct {
	sessionID string
	backend   agentcontext.ReferenceBackend
}

func (referenceProjection) Name() string { return "reference-first-context" }
func (p referenceProjection) BeforeModel(ctx context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	if runtime.State == nil || runtime.State.Session().ID() != p.sessionID {
		return errors.New("reference projection does not belong to this runtime session")
	}
	// A historical assistant call is eligible only when its following result is
	// present. Never rewrite an unresolved/current invocation's arguments.
	type position struct{ message, block int }
	pending := map[string][]position{}
	var executed []position
	for messageIndex, message := range request.Messages {
		for blockIndex, block := range message.Content {
			if message.Role == wire.RoleAssistant && block.ToolCall != nil {
				pending[block.ToolCall.ID] = append(pending[block.ToolCall.ID], position{messageIndex, blockIndex})
			}
			if message.Role == wire.RoleTool && block.ToolResult != nil {
				matches := pending[block.ToolResult.CallID]
				if len(matches) == 1 {
					executed = append(executed, matches[0])
				}
				delete(pending, block.ToolResult.CallID)
			}
		}
	}
	for _, location := range executed {
		call := request.Messages[location.message].Content[location.block].ToolCall
		if len(call.Arguments) <= resultInlineLimit {
			continue
		}
		if !json.Valid(call.Arguments) {
			return errors.New("invalid historical tool arguments cannot be projected")
		}
		reference, err := p.backend.StoreText(ctx, string(call.Arguments))
		if err != nil {
			return err
		}
		data, err := json.Marshal(struct {
			Reference wire.Reference `json:"alva_retained_arguments"`
			Notice    string         `json:"source_notice"`
		}{reference, "Already executed historical call; original arguments retained verbatim. This reference is not new executable arguments. Use context_read/context_search for relevant text only."})
		if err != nil {
			return err
		}
		call.Arguments = data
	}
	for index := range request.Messages {
		if err := ctx.Err(); err != nil {
			return err
		}
		message := &request.Messages[index]
		if message.Role != wire.RoleTool {
			continue
		}
		for blockIndex := range message.Content {
			block := &message.Content[blockIndex]
			if block.ToolResult == nil {
				continue
			}
			textBytes := 0
			for _, result := range block.ToolResult.Content {
				if result.Kind == wire.ContentText {
					textBytes += len(result.Text)
				}
			}
			for resultIndex := range block.ToolResult.Content {
				result := &block.ToolResult.Content[resultIndex]
				switch result.Kind {
				case wire.ContentImage, wire.ContentAudio, wire.ContentVideo:
					if err := p.projectMedia(ctx, result); err != nil {
						return err
					}
				case wire.ContentText:
					if textBytes <= resultInlineLimit || result.Text == "" {
						continue
					}
					if len(result.Text) > referenceStoreLimit {
						return errors.New("tool text exceeds 8 MiB reference projection budget; not delivered")
					}
					reference, err := p.backend.StoreText(ctx, result.Text)
					if err != nil {
						return err
					}
					*result = wire.ContentBlock{Kind: wire.ContentReference, Reference: &reference}
				}
			}
		}
	}
	return nil
}

func (p referenceProjection) projectMedia(ctx context.Context, block *wire.ContentBlock) error {
	if err := block.Validate(); err != nil {
		return err
	}
	// Check byte-bearing fields before base64/JSON allocation. Large files still
	// belong in native artifacts; a tool must return their references directly.
	bytes := 0
	account := func(payload wire.MediaPayload) {
		bytes += len(payload.Data) + len(payload.URL)
	}
	if block.Image != nil {
		account(block.Image.Payload)
	}
	if block.Audio != nil {
		account(block.Audio.Payload)
	}
	if block.Video != nil {
		if len(block.Video.Frames) > 128 {
			return errors.New("tool video exceeds 128 retained-frame projection budget; not delivered")
		}
		if block.Video.Payload != nil {
			account(*block.Video.Payload)
		}
		for _, frame := range block.Video.Frames {
			account(frame.Image.Payload)
		}
	}
	if bytes > referenceStoreLimit*3/4 {
		return errors.New("tool media exceeds bounded reference projection budget; return a native artifact reference instead")
	}
	data, err := json.Marshal(block)
	if err != nil {
		return err
	}
	if len(data) > referenceStoreLimit {
		return errors.New("tool media JSON exceeds 8 MiB reference projection budget; not delivered")
	}
	reference, err := p.backend.StoreText(ctx, string(data))
	if err != nil {
		return err
	}
	reference.Processing = "Complete tool " + string(block.Kind) + " DTO retained as JSON, including original bytes/URL and supplied provenance. Not decoded, compressed, fetched, selected or delivered as media. Use bounded text retrieval for metadata; explicit media selection is separate."
	*block = wire.ContentBlock{Kind: wire.ContentReference, Reference: &reference}
	return nil
}
