// INPUT: context, errors, fmt, shared storage contracts, kernel/abi and llm/wire.
// OUTPUT: Native context selection policy over bounded fixed-water conversation reads, shared by the plain and compacting sources.
// POS: [L5 harness] Selects whole recent turns and explains omitted history without changing stored facts.
package harness

import (
	"context"
	"errors"
	"fmt"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const contextTargetMessages = 64
const contextMaxMessages = 512
const contextMaxBytes = 8 << 20

var errLatestTurnTooLarge = errors.New("native context latest turn exceeds 512-message / 8 MiB working-view budget")

type nativeContextSource struct {
	repository SessionStorage
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
	count := selectRecent(metadata, len(metadata), contextTargetMessages)
	if count == 0 {
		return nil, errLatestTurnTooLarge
	}
	messages, err := tail.Read(ctx, count, contextMaxBytes)
	if err != nil {
		return nil, err
	}
	if count < len(metadata) {
		messages = append([]wire.Message{omissionNotice(source.id, metadata[count], tail.HighWater())}, messages...)
	}
	return messages, nil
}

// selectRecent returns how many of the newest available metadata entries
// (newest first) form whole recent turns within the 512-message / 8 MiB
// working-view bounds, stopping at the first user turn start once target
// messages are reached. Reaching the oldest available entry also ends a valid
// range. Zero means even the latest turn does not fit.
func selectRecent(metadata []storage.ConversationMetadata, available, target int) int {
	last, remaining := -1, int64(contextMaxBytes)
	for i := 0; i < available && i < len(metadata); i++ {
		record := metadata[i]
		if i >= contextMaxMessages || record.RestoreBytes > remaining {
			break
		}
		remaining -= record.RestoreBytes
		if record.Role == wire.RoleUser {
			last = i
			if i+1 >= target {
				break
			}
		}
		if i == available-1 {
			last = i
		}
	}
	return last + 1
}

// omissionNotice marks history before the selected tail; boundary is the
// newest omitted conversation record.
func omissionNotice(sessionID string, boundary storage.ConversationMetadata, highWater uint64) wire.Message {
	text := fmt.Sprintf("Earlier conversation records through sequence %d are omitted from this bounded working view at high-water %d. This is not a summary. Original facts remain unchanged. Use native_history with high_water=%d and context_read/context_search for explicit bounded retrieval; this reference identifies only the last omitted record.", boundary.Record.Sequence, highWater, boundary.Record.Sequence)
	reference := boundary.Reference
	return wire.Message{ID: fmt.Sprintf("context/native/%s/%d/%d", sessionID, boundary.Record.Sequence, highWater), Role: wire.RoleUser, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}, {Kind: wire.ContentReference, Reference: &reference}}}
}

// This policy contributes only the narrow SDK working-view contract.
var _ abi.ContextSource = (*nativeContextSource)(nil)
