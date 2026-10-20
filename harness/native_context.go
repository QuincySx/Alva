// INPUT: context, errors, fmt, storage/session, kernel/abi and llm/wire.
// OUTPUT: Native context selection policy over bounded fixed-water conversation reads.
// POS: [L5 harness] Selects whole recent turns and explains omitted history without changing stored facts.
package harness

import (
	"context"
	"errors"
	"fmt"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const contextTargetMessages = 64
const contextMaxMessages = 512
const contextMaxBytes = 8 << 20

type nativeContextSource struct {
	repository *session.Repository
	owner, id  string
}

func (source *nativeContextSource) Load(ctx context.Context, log abi.ConversationReader) ([]wire.Message, error) {
	if log == nil || nilImplementation(log) || log.ID() != source.id {
		return nil, errors.New("native context source belongs to a different session")
	}
	tail, err := source.repository.BoundConversationTail(ctx, source.owner, source.id, contextMaxMessages)
	if err != nil {
		return nil, err
	}
	metadata, err := tail.Metadata()
	if err != nil {
		return nil, err
	}
	if len(metadata) == 0 {
		return nil, nil
	}
	last, remaining := -1, int64(contextMaxBytes)
	for i, record := range metadata {
		if i >= contextMaxMessages || record.RestoreBytes > remaining {
			break
		}
		remaining -= record.RestoreBytes
		if record.Role == wire.RoleUser {
			last = i
			if i+1 >= contextTargetMessages {
				break
			}
		}
		if i == len(metadata)-1 {
			last = i
		}
	}
	if last < 0 {
		return nil, errors.New("native context latest turn exceeds 512-message / 8 MiB working-view budget")
	}
	messages, err := tail.Read(ctx, last+1, contextMaxBytes)
	if err != nil {
		return nil, err
	}
	if last+1 < len(metadata) {
		boundary := metadata[last+1]
		text := fmt.Sprintf("Earlier conversation records through sequence %d are omitted from this bounded working view at high-water %d. This is not a summary. Original facts remain unchanged. Use native_history with high_water=%d and context_read/context_search for explicit bounded retrieval; this reference identifies only the last omitted record.", boundary.Record.Sequence, tail.HighWater(), boundary.Record.Sequence)
		notice := wire.Message{ID: fmt.Sprintf("context/native/%s/%d/%d", source.id, boundary.Record.Sequence, tail.HighWater()), Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}, {Kind: wire.ContentReference, Reference: &boundary.Reference}}}
		messages = append([]wire.Message{notice}, messages...)
	}
	return messages, nil
}

// This policy contributes only the narrow SDK working-view contract.
var _ abi.ContextSource = (*nativeContextSource)(nil)
