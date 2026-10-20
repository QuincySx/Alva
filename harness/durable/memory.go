// INPUT: context, encoding/json, sync, agent/core and kernel/abi/wire.
// OUTPUT: MemoryBackend with atomic snapshots/delivery and exclusive process-lifetime claims.
// POS: [L5 optional harness] Volatile reference backend for custom hosts; never advertises restart durability.
package durable

import (
	"context"
	"encoding/json"
	"sync"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const Namespace = "alva.harness.durable"
const SnapshotType = "wait_state"

type MemoryBackend struct {
	mu       sync.Mutex
	log      *core.MemorySession
	snapshot Snapshot
	active   bool
}

func NewMemoryBackend(id string) *MemoryBackend {
	return &MemoryBackend{log: core.NewMemorySession(id), snapshot: Snapshot{State: State{Version: 1, Status: Idle}}}
}
func (b *MemoryBackend) Log() abi.ConversationLog { return b.log }
func (b *MemoryBackend) Claim(ctx context.Context) (func(), error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.active {
		return nil, core.ErrRunInProgress
	}
	b.active = true
	var once sync.Once
	return func() { once.Do(func() { b.mu.Lock(); b.active = false; b.mu.Unlock() }) }, nil
}
func (b *MemoryBackend) Load(ctx context.Context) (Snapshot, error) {
	if err := ctx.Err(); err != nil {
		return Snapshot{}, err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	return clone(b.snapshot), nil
}
func (b *MemoryBackend) Commit(ctx context.Context, revision uint64, state State, messages []wire.Message) (Snapshot, error) {
	if err := state.Validate(); err != nil {
		return Snapshot{}, err
	}
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.snapshot.Revision != revision {
		return Snapshot{}, ErrConflict
	}
	data, _ := json.Marshal(state)
	records := []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: Namespace, Type: SnapshotType, SchemaVersion: 1, Payload: data}}
	for _, message := range wire.CloneMessages(messages) {
		m := message
		records = append(records, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &m}))
	}
	committed, err := b.log.AppendRecords(ctx, records)
	if err != nil {
		return Snapshot{}, err
	}
	b.snapshot = Snapshot{Revision: committed[0].Sequence, State: clone(state)}
	return clone(b.snapshot), nil
}
