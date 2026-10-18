// INPUT: context, errors, reflect, sync, sync/atomic, testing, time, agent/core, agent/graph, kernel/abi, llm/wire
// OUTPUT: Independent-agent BSP scheduling, session isolation and cancellation join probes
// POS: [architecture / executable probes] Composes the existing graph with real standard agent loops outside the kernel.
package extensibility_test

import (
	"context"
	"errors"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/graph"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestOutsideSchedulerComposesIndependentAgents(t *testing.T) {
	for _, cancelWorkers := range []bool{false, true} {
		t.Run(map[bool]string{false: "parallel-merge", true: "cancel-join"}[cancelWorkers], func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			defer cancel()
			entered := make(chan string, 2)
			release := make(chan struct{})
			var once sync.Once
			unblock := func() { once.Do(func() { close(release) }) }
			defer unblock()
			cancelObserved := make(chan string, 2)
			var exited atomic.Int32
			sessions := map[string]*core.MemorySession{}
			builder := graph.NewStateGraph(func(state []string) []string { return append([]string(nil), state...) })
			builder.SetEntryPoint("dispatch")
			if err := builder.AddNode("dispatch", func(context.Context, []string) (graph.NodeResult[[]string], error) {
				return graph.Sends(graph.SendTo[[]string]{Node: "alpha"}, graph.SendTo[[]string]{Node: "beta"}), nil
			}); err != nil {
				t.Fatal(err)
			}
			for _, name := range []string{"alpha", "beta"} {
				session := core.NewMemorySession(name)
				sessions[name] = session
				model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
					if request.Messages[len(request.Messages)-1].Role != wire.RoleTool {
						return requestTool(emit, "work", name+"-call")
					}
					return finishText(emit, name+"-answer")
				})
				agent := build(t, core.NewBuilder(model, session).Tool(tool{name: "work", readOnly: true, execute: func(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
					if execution.SessionID != name {
						return wire.ToolResult{}, errors.New("foreign session bound")
					}
					entered <- name
					defer exited.Add(1)
					select {
					case <-ctx.Done():
						cancelObserved <- name
						<-release // Cancellation observed; actual work has not exited.
						return wire.ToolResult{}, ctx.Err()
					case <-release:
						return toolResult(call, name+"-work"), nil
					}
				}}))
				if err := builder.AddNode(name, func(ctx context.Context, _ []string) (graph.NodeResult[[]string], error) {
					var answer string
					err := agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, name+"-private-input")}, func(event abi.Event) error {
						if event.Kind == abi.EventTextDelta {
							answer += event.Text
						}
						return nil
					})
					return graph.Update([]string{answer}), err
				}); err != nil {
					t.Fatal(err)
				}
			}
			builder.SetMerge(func(_ []string, updates [][]string) ([]string, error) {
				var merged []string
				for _, update := range updates {
					merged = append(merged, update...)
				}
				return merged, nil
			})
			scheduler, err := builder.Compile()
			if err != nil {
				t.Fatal(err)
			}
			type outcome struct {
				answers []string
				err     error
			}
			done := make(chan outcome, 1)
			go func() { answers, err := scheduler.Invoke(ctx, nil); done <- outcome{answers, err} }()
			for i := 0; i < 2; i++ {
				select {
				case <-entered:
				case <-ctx.Done():
					t.Fatal("workers did not overlap", ctx.Err())
				}
			}
			if cancelWorkers {
				cancel()
				for i := 0; i < 2; i++ {
					select {
					case <-cancelObserved:
					case <-time.After(5 * time.Second):
						t.Fatal("tool did not observe cancellation")
					}
				}
				select {
				case <-done:
					t.Fatal("scheduler returned before cancelled tools exited")
				case <-time.After(25 * time.Millisecond):
				}
				unblock()
			} else {
				unblock()
			}
			var result outcome
			select {
			case result = <-done:
			case <-time.After(5 * time.Second):
				t.Fatal("scheduler failed to join")
			}
			if exited.Load() != 2 {
				t.Fatal("scheduler returned with active tools", exited.Load())
			}
			if cancelWorkers {
				if !errors.Is(result.err, context.Canceled) {
					t.Fatal(result.err)
				}
			} else if result.err != nil || !reflect.DeepEqual(result.answers, []string{"alpha-answer", "beta-answer"}) {
				t.Fatal(result.answers, result.err)
			}
			for name, session := range sessions {
				for _, record := range recordList(t, session) {
					if record.Message != nil && record.Message.Role == wire.RoleUser && record.Message.Text() != name+"-private-input" {
						t.Fatal("peer history leaked", name)
					}
				}
			}
		})
	}
}
