// INPUT: context, errors, fmt, sort, sync
// OUTPUT: InvokeConfig, GraphEvent, CompiledGraph.Invoke, InvokeWithConfig, Resume
// POS: [L3 capability library] Bounded Pregel BSP execution with dynamic Sends, deterministic merge and real checkpoint frontiers.
package graph

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"sync"
)

type GraphEvent struct {
	Kind                     string
	Node, From, CheckpointID string
	Step, NodesExecuted      int
}
type InvokeConfig[S any] struct {
	Checkpoint                                    CheckpointSaver[S]
	CheckpointID                                  string
	MaxSteps, MaxParallel, MaxSendDepth, MaxTasks int
	Emit                                          func(GraphEvent)
	Retry                                         *RetryConfig
}

func (config InvokeConfig[S]) emit(event GraphEvent) {
	if config.Emit != nil {
		config.Emit(event)
	}
}
func (graph *CompiledGraph[S]) Invoke(ctx context.Context, input S) (S, error) {
	return graph.InvokeWithConfig(ctx, input, InvokeConfig[S]{})
}
func (graph *CompiledGraph[S]) InvokeWithConfig(ctx context.Context, input S, config InvokeConfig[S]) (S, error) {
	owned, err := graph.cloneState(ctx, input)
	if err != nil {
		return input, err
	}
	return graph.drive(ctx, owned, []string{graph.entry}, 0, config)
}
func (graph *CompiledGraph[S]) Resume(ctx context.Context, checkpointID string, config InvokeConfig[S]) (S, error) {
	var zero S
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if config.Checkpoint == nil {
		return zero, errors.New("checkpoint saver required for resume")
	}
	checkpoint, found, err := config.Checkpoint.Load(ctx, checkpointID)
	if err = joinCancellation(ctx, err); err != nil {
		return zero, err
	}
	if !found {
		return zero, errors.New("graph checkpoint not found")
	}
	if checkpoint.Step < 0 {
		return zero, errors.New("invalid graph checkpoint step")
	}
	owned, err := graph.cloneState(ctx, checkpoint.State)
	if err != nil {
		return zero, err
	}
	return graph.drive(ctx, owned, append([]string(nil), checkpoint.Pending...), checkpoint.Step, config)
}

// A clone is supplied by the caller, just like a node or router. Recheck
// cancellation before admitting the next callback after it returns.
func (graph *CompiledGraph[S]) cloneState(ctx context.Context, state S) (S, error) {
	if err := ctx.Err(); err != nil {
		return state, err
	}
	owned := graph.clone(state)
	return owned, ctx.Err()
}

func joinCancellation(ctx context.Context, err error) error {
	canceled := ctx.Err()
	if canceled == nil {
		return err
	}
	if err == nil {
		return canceled
	}
	return errors.Join(err, canceled)
}

func (graph *CompiledGraph[S]) drive(ctx context.Context, state S, current []string, step int, config InvokeConfig[S]) (S, error) {
	if config.MaxSteps <= 0 {
		config.MaxSteps = 100
	}
	if config.MaxParallel <= 0 {
		config.MaxParallel = 8
	}
	if config.MaxParallel > 256 {
		return state, errors.New("graph concurrency exceeds 256")
	}
	if config.MaxSendDepth <= 0 {
		config.MaxSendDepth = 64
	}
	if config.MaxTasks <= 0 {
		config.MaxTasks = 10000
	}
	if config.CheckpointID == "" {
		config.CheckpointID = "graph"
	}
	budget := config.MaxTasks
	for {
		if err := ctx.Err(); err != nil {
			return state, err
		}
		current = uniqueNodes(current)
		if len(current) == 0 {
			config.emit(GraphEvent{Kind: "completed", Step: step})
			return state, ctx.Err()
		}
		if step >= config.MaxSteps {
			return state, fmt.Errorf("graph exceeded max_steps (%d)", config.MaxSteps)
		}
		step++
		config.emit(GraphEvent{Kind: "superstep_start", Step: step})
		tasks := make([]SendTo[S], len(current))
		for i, name := range current {
			owned, err := graph.cloneState(ctx, state)
			if err != nil {
				return state, err
			}
			tasks[i] = SendTo[S]{Node: name, State: owned}
		}
		nextState, next, err := graph.executeTasks(ctx, state, tasks, config, step, 0, false, &budget)
		if err = joinCancellation(ctx, err); err != nil {
			return state, err
		}
		state, current = nextState, next
		config.emit(GraphEvent{Kind: "superstep_end", Step: step, NodesExecuted: len(tasks)})
		if err := ctx.Err(); err != nil {
			return state, err
		}
		if config.Checkpoint != nil {
			id := fmt.Sprintf("%s-step-%d", config.CheckpointID, step)
			owned, err := graph.cloneState(ctx, state)
			if err != nil {
				return state, err
			}
			err = config.Checkpoint.Save(ctx, id, Checkpoint[S]{State: owned, Step: step, Pending: append([]string(nil), current...)})
			if err = joinCancellation(ctx, err); err != nil {
				return state, fmt.Errorf("graph checkpoint failed: %w", err)
			}
			config.emit(GraphEvent{Kind: "checkpoint", Step: step, CheckpointID: id})
		}
	}
}

type execution[S any] struct {
	result NodeResult[S]
	err    error
}

func (graph *CompiledGraph[S]) executeTasks(ctx context.Context, base S, tasks []SendTo[S], config InvokeConfig[S], step, depth int, dynamic bool, budget *int) (S, []string, error) {
	if err := ctx.Err(); err != nil {
		return base, nil, err
	}
	if depth > config.MaxSendDepth || len(tasks) > *budget {
		return base, nil, errors.New("graph dynamic Send budget exceeded")
	}
	*budget -= len(tasks)
	for _, task := range tasks {
		if graph.nodes[task.Node] == nil {
			return base, nil, fmt.Errorf("graph node %q not registered", task.Node)
		}
	}
	results := make([]execution[S], len(tasks))
	semaphore := make(chan struct{}, config.MaxParallel)
	var wait sync.WaitGroup
	for i, task := range tasks {
		config.emit(GraphEvent{Kind: "node_start", Node: task.Node, Step: step})
		wait.Add(1)
		go func(index int, task SendTo[S]) {
			defer wait.Done()
			select {
			case semaphore <- struct{}{}:
			case <-ctx.Done():
				results[index].err = ctx.Err()
				return
			}
			defer func() { <-semaphore }()
			owned, err := graph.cloneState(ctx, task.State)
			if err != nil {
				results[index].err = err
				return
			}
			results[index].result, results[index].err = graph.invokeNode(ctx, task.Node, owned, config.Retry)
		}(i, task)
	}
	wait.Wait()
	var failures []error
	for i, result := range results {
		config.emit(GraphEvent{Kind: "node_end", Node: tasks[i].Node, Step: step})
		if result.err != nil {
			failures = append(failures, result.err)
		}
	}
	if err := joinCancellation(ctx, errors.Join(failures...)); err != nil {
		return base, nil, err
	}
	var updates []S
	var sends []SendTo[S]
	for i, result := range results {
		switch result.result.Kind {
		case ResultUpdate:
			owned, err := graph.cloneState(ctx, result.result.State)
			if err != nil {
				return base, nil, err
			}
			updates = append(updates, owned)
		case ResultSends:
			for _, send := range result.result.Sends {
				config.emit(GraphEvent{Kind: "send_emitted", From: tasks[i].Node, Node: send.Node, Step: step})
				owned, err := graph.cloneState(ctx, send.State)
				if err != nil {
					return base, nil, err
				}
				sends = append(sends, SendTo[S]{Node: send.Node, State: owned})
			}
		default:
			return base, nil, errors.New("invalid graph node result kind")
		}
	}
	state, err := graph.cloneState(ctx, base)
	if err != nil {
		return base, nil, err
	}
	if len(updates) > 0 {
		if len(tasks) == 1 {
			state = updates[0]
		} else if graph.merge != nil {
			owned, err := graph.cloneState(ctx, base)
			if err != nil {
				return base, nil, err
			}
			next, err := graph.merge(owned, updates)
			if err = joinCancellation(ctx, err); err != nil {
				return base, nil, err
			}
			state = next
		} else {
			state = updates[len(updates)-1]
		}
	}
	var next []string
	for i, result := range results {
		if result.result.Kind == ResultUpdate {
			routingState := state
			if dynamic && len(tasks) > 1 {
				routingState = result.result.State
			}
			targets, err := graph.resolve(ctx, tasks[i].Node, routingState)
			if err != nil {
				return base, nil, err
			}
			next = append(next, targets...)
		}
	}
	if len(sends) > 0 {
		nestedState, nestedNext, err := graph.executeTasks(ctx, state, sends, config, step, depth+1, true, budget)
		if err != nil {
			return base, nil, err
		}
		state = nestedState
		next = append(next, nestedNext...)
	}
	owned, err := graph.cloneState(ctx, state)
	return owned, uniqueNodes(next), err
}
func (graph *CompiledGraph[S]) invokeNode(ctx context.Context, name string, state S, retry *RetryConfig) (NodeResult[S], error) {
	for attempt := 0; ; attempt++ {
		if err := ctx.Err(); err != nil {
			return NodeResult[S]{}, err
		}
		owned, err := graph.cloneState(ctx, state)
		if err != nil {
			return NodeResult[S]{}, err
		}
		result, err := graph.nodes[name](ctx, owned)
		err = joinCancellation(ctx, err)
		if err == nil || errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) || retry == nil || attempt >= retry.MaxRetries || retry.Retryable == nil || !retry.Retryable(err) {
			return result, err
		}
		if waitErr := retry.Wait(ctx, attempt); waitErr != nil {
			return NodeResult[S]{}, errors.Join(err, waitErr)
		}
	}
}
func (graph *CompiledGraph[S]) resolve(ctx context.Context, from string, state S) ([]string, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var targets []string
	for _, edge := range graph.edges {
		if edge.from != from {
			continue
		}
		target := edge.to
		if edge.router != nil {
			owned, err := graph.cloneState(ctx, state)
			if err != nil {
				return nil, err
			}
			next, err := edge.router(ctx, owned)
			if err = joinCancellation(ctx, err); err != nil {
				return nil, err
			}
			target = next
		}
		if target != End && graph.nodes[target] == nil {
			return nil, fmt.Errorf("graph route target %q not registered", target)
		}
		targets = append(targets, target)
	}
	if len(targets) == 0 {
		return []string{End}, nil
	}
	return targets, nil
}
func uniqueNodes(nodes []string) []string {
	set := make(map[string]bool)
	var result []string
	for _, node := range nodes {
		if node != End && !set[node] {
			set[node] = true
			result = append(result, node)
		}
	}
	sort.Strings(result)
	return result
}
