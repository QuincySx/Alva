// INPUT: context, errors, reflect, time, agent/context, kernel/abi, llm/wire
// OUTPUT: RetryConfig, ContextTransform, TransformPipeline, CompactionConfig, GraphRun
// POS: [L3 capability library] Explicit opt-in graph orchestration helpers without automatic retry or a second AgentLoop.
package graph

import (
	"context"
	"encoding/json"
	"errors"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"reflect"
	"time"
)

type RetryConfig struct {
	MaxRetries             int
	InitialDelay, MaxDelay time.Duration
	Retryable              func(error) bool
}

func (config RetryConfig) Delay(attempt int) time.Duration {
	delay := config.InitialDelay
	if delay < 0 {
		delay = 0
	}
	max := config.MaxDelay
	if max <= 0 {
		max = 30 * time.Second
	}
	for i := 0; i < attempt; i++ {
		if delay >= max || delay > max/2 {
			return max
		}
		delay *= 2
	}
	if delay > max {
		return max
	}
	return delay
}
func (config RetryConfig) Wait(ctx context.Context, attempt int) error {
	timer := time.NewTimer(config.Delay(attempt))
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

type ContextTransform interface {
	Transform(context.Context, []wire.Message) ([]wire.Message, error)
}
type TransformFunc func(context.Context, []wire.Message) ([]wire.Message, error)

func (fn TransformFunc) Transform(ctx context.Context, messages []wire.Message) ([]wire.Message, error) {
	if fn == nil {
		return nil, errors.New("graph transform function is nil")
	}
	return fn(ctx, messages)
}

type TransformPipeline struct{ Transforms []ContextTransform }

func (pipeline TransformPipeline) Apply(ctx context.Context, messages []wire.Message) ([]wire.Message, error) {
	working := wire.CloneMessages(messages)
	for _, transform := range pipeline.Transforms {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if transform == nil {
			return nil, errors.New("graph transform is nil")
		}
		value := reflect.ValueOf(transform)
		switch value.Kind() {
		case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
			if value.IsNil() {
				return nil, errors.New("graph transform is typed nil")
			}
		}
		next, err := transform.Transform(ctx, wire.CloneMessages(working))
		err = errors.Join(err, ctx.Err())
		if err != nil {
			return nil, err
		}
		working = wire.CloneMessages(next)
	}
	return working, nil
}

type CompactionConfig struct {
	MaxTokens, KeepRecent int
	Counter               abi.TokenCounter
}

func (config CompactionConfig) ShouldCompact(messages []wire.Message) bool {
	tokens := 0
	for _, message := range messages {
		data, _ := json.Marshal(message.Content)
		if config.Counter != nil {
			tokens += config.Counter.CountTokens(string(data)) + 4
		} else {
			tokens += (len([]rune(string(data)))+3)/4 + 4
		}
	}
	return config.MaxTokens > 0 && tokens > config.MaxTokens
}
func (config CompactionConfig) Compact(messages []wire.Message) []wire.Message {
	if !config.ShouldCompact(messages) {
		return wire.CloneMessages(messages)
	}
	return agentcontext.SafeWindow(messages, config.KeepRecent)
}

type GraphRun[S any] struct {
	Graph  *CompiledGraph[S]
	Config InvokeConfig[S]
}

func (run GraphRun[S]) Invoke(ctx context.Context, state S) (S, error) {
	if run.Graph == nil {
		return state, errors.New("compiled graph required")
	}
	return run.Graph.InvokeWithConfig(ctx, state, run.Config)
}
