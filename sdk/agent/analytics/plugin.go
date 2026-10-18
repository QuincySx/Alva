// INPUT: context, errors, strings, sync, time, unicode/utf8, agent/abi, agent/core, kernel/abi, kernel/bus, llm/wire
// OUTPUT: Plugin, Middleware
// POS: [L3 capability library] Opt-in lifecycle observations above generic kernel, preserving real results and unknown model metadata.
package analytics

import (
	"context"
	"errors"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Plugin struct{ Sink agentabi.AnalyticsSink }

func (Plugin) Name() string { return "analytics" }
func (plugin Plugin) Register(_ context.Context, registrar *core.Registrar) error {
	if plugin.Sink == nil {
		return errors.New("analytics sink required")
	}
	if err := bus.Provide[agentabi.AnalyticsSink](registrar.BusWriter(), plugin.Sink); err != nil {
		return err
	}
	// Observe inside ordinary policy wrappers, preserving inclusive timing.
	return registrar.Middleware(Middleware{}, 5000)
}

type Middleware struct{}

func (Middleware) Name() string { return "analytics" }

type runStart struct {
	RunID, SessionID string
	Started          time.Time
}

func record(handle bus.Handle, event agentabi.AnalyticsEvent) {
	if sink, ok := bus.Resolve[agentabi.AnalyticsSink](handle); ok {
		sink.Record(event)
	}
}
func (Middleware) OnRunStart(_ context.Context, runtime abi.RuntimeContext) error {
	start := runStart{runtime.RunID, runtime.State.Session().ID(), time.Now()}
	abi.PutExtension(runtime.State.Extensions(), start)
	record(runtime.Capabilities, agentabi.AnalyticsEvent{Type: agentabi.AnalyticsSessionStart, SessionID: start.SessionID, RunID: runtime.RunID, Time: start.Started.UTC(), SessionStart: &agentabi.AnalyticsSessionStarted{Workspace: runtime.Workspace}})
	return nil
}
func (Middleware) OnRunEnd(_ context.Context, runtime abi.RuntimeContext, _ error) error {
	start, ok := abi.GetExtension[runStart](runtime.State.Extensions())
	if !ok || start.RunID != runtime.RunID {
		return nil
	}
	abi.DeleteExtension[runStart](runtime.State.Extensions())
	record(runtime.Capabilities, agentabi.AnalyticsEvent{Type: agentabi.AnalyticsSessionEnd, SessionID: start.SessionID, RunID: runtime.RunID, Time: time.Now().UTC(), SessionEnd: &agentabi.AnalyticsSessionEnded{DurationMS: uint64(time.Since(start.Started).Milliseconds())}})
	return nil
}
func (Middleware) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	started := time.Now()
	sessionID := runtime.InvocationSessionID
	if sessionID == "" {
		sessionID = runtime.State.Session().ID()
	}
	providerID, modelID := "", ""
	actualModel := runtime.InvocationModel
	if actualModel == nil {
		actualModel = runtime.State.Model()
	}
	if identity, ok := actualModel.(abi.ModelIdentity); ok {
		providerID = identity.ProviderID()
		modelID = identity.ModelID()
	}
	var mu sync.Mutex
	var usage *wire.Usage
	var callbackErr error
	finished := false
	err := next(ctx, runtime, request, func(chunk wire.StreamChunk) error {
		mu.Lock()
		if chunk.Usage != nil {
			value := wire.CloneUsage(*chunk.Usage)
			usage = &value
		}
		if chunk.Kind == wire.ChunkFinish {
			finished = true
		}
		mu.Unlock()
		err := emit(chunk)
		if err != nil {
			mu.Lock()
			callbackErr = errors.Join(callbackErr, err)
			mu.Unlock()
		}
		return err
	})
	if err != nil {
		return err
	}
	mu.Lock()
	captured := usage
	completed := finished && callbackErr == nil
	mu.Unlock()
	if !completed || ctx.Err() != nil {
		return nil
	}
	record(runtime.Capabilities, agentabi.AnalyticsEvent{Type: agentabi.AnalyticsLLMCall, SessionID: sessionID, RunID: runtime.RunID, Time: time.Now().UTC(), LLMCall: &agentabi.AnalyticsModelCall{Provider: providerID, Model: modelID, Usage: captured, LatencyMS: uint64(time.Since(started).Milliseconds())}})
	return nil
}
func (Middleware) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	started := time.Now()
	record(execution.Capabilities, agentabi.AnalyticsEvent{Type: agentabi.AnalyticsToolStart, SessionID: execution.SessionID, RunID: execution.RunID, Time: started.UTC(), ToolStart: &agentabi.AnalyticsToolStarted{Tool: call.Name, CallID: call.ID}})
	result, err := next(ctx, execution, call)
	summary := ""
	if err != nil {
		summary = strings.ToValidUTF8(err.Error(), "�")
	} else if result.IsError {
		summary = "tool reported IsError"
	}
	if len(summary) > 1024 {
		summary = summary[:1024]
		for !utf8.ValidString(summary) {
			summary = summary[:len(summary)-1]
		}
	}
	record(execution.Capabilities, agentabi.AnalyticsEvent{Type: agentabi.AnalyticsToolEnd, SessionID: execution.SessionID, RunID: execution.RunID, Time: time.Now().UTC(), ToolEnd: &agentabi.AnalyticsToolEnded{Tool: call.Name, CallID: call.ID, LatencyMS: uint64(time.Since(started).Milliseconds()), OK: err == nil && !result.IsError, Error: summary}})
	return result, err
}
