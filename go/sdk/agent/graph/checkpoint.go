// INPUT: context, errors, sort, sync
// OUTPUT: Checkpoint, CheckpointSaver, MemoryCheckpointSaver, NewMemoryCheckpointSaver
// POS: [L3 capability library] Typed detached graph checkpoints carrying state and exact next frontier without implicit execution replay.
package graph

import (
	"context"
	"errors"
	"sort"
	"sync"
)

type Checkpoint[S any] struct {
	State   S
	Step    int
	Pending []string
}
type CheckpointSaver[S any] interface {
	Save(context.Context, string, Checkpoint[S]) error
	Load(context.Context, string) (Checkpoint[S], bool, error)
	List(context.Context) ([]string, error)
	Delete(context.Context, string) error
}
type MemoryCheckpointSaver[S any] struct {
	mu       sync.RWMutex
	values   map[string]Checkpoint[S]
	clone    func(S) S
	capacity int
}

func NewMemoryCheckpointSaver[S any](clone func(S) S, capacity int) (*MemoryCheckpointSaver[S], error) {
	if clone == nil || capacity < 1 {
		return nil, errors.New("checkpoint clone and positive capacity required")
	}
	return &MemoryCheckpointSaver[S]{values: make(map[string]Checkpoint[S]), clone: clone, capacity: capacity}, nil
}
func (saver *MemoryCheckpointSaver[S]) owned(checkpoint Checkpoint[S]) Checkpoint[S] {
	checkpoint.State = saver.clone(checkpoint.State)
	checkpoint.Pending = append([]string(nil), checkpoint.Pending...)
	return checkpoint
}
func (saver *MemoryCheckpointSaver[S]) Save(ctx context.Context, id string, checkpoint Checkpoint[S]) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if id == "" || checkpoint.Step < 0 {
		return errors.New("invalid graph checkpoint")
	}
	owned := saver.owned(checkpoint)
	if err := ctx.Err(); err != nil {
		return err
	}
	saver.mu.Lock()
	defer saver.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if _, exists := saver.values[id]; !exists && len(saver.values) >= saver.capacity {
		return errors.New("graph checkpoint capacity exceeded")
	}
	saver.values[id] = owned
	return nil
}
func (saver *MemoryCheckpointSaver[S]) Load(ctx context.Context, id string) (Checkpoint[S], bool, error) {
	if err := ctx.Err(); err != nil {
		return Checkpoint[S]{}, false, err
	}
	saver.mu.RLock()
	checkpoint, found := saver.values[id]
	saver.mu.RUnlock()
	if !found {
		return Checkpoint[S]{}, false, nil
	}
	owned := saver.owned(checkpoint)
	if err := ctx.Err(); err != nil {
		return Checkpoint[S]{}, false, err
	}
	return owned, true, nil
}
func (saver *MemoryCheckpointSaver[S]) List(ctx context.Context) ([]string, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	saver.mu.RLock()
	defer saver.mu.RUnlock()
	result := make([]string, 0, len(saver.values))
	for key := range saver.values {
		result = append(result, key)
	}
	sort.Strings(result)
	return result, nil
}
func (saver *MemoryCheckpointSaver[S]) Delete(ctx context.Context, id string) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	saver.mu.Lock()
	delete(saver.values, id)
	saver.mu.Unlock()
	return nil
}
