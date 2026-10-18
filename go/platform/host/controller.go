// INPUT: context, reflect, sync, agent-independent harness.Runner, kernel/abi, llm/wire.
// OUTPUT: Controller, RunSnapshot, RecordedEvent and explicit run/subscription lifecycles.
// POS: [platform / native execution host] Native root-run owner whose observers cannot cancel execution by detaching.
package host

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"sync"

	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type RunStatus string

const (
	RunRunning   RunStatus = "running"
	RunCompleted RunStatus = "completed"
	RunCancelled RunStatus = "cancelled"
	RunFailed    RunStatus = "failed"
)

var (
	ErrRunNotFound      = errors.New("run not found or expired")
	ErrRunActive        = errors.New("a root run is already active")
	ErrHistoryExpired   = errors.New("requested replay history expired; refresh snapshot")
	ErrControllerClosed = errors.New("controller is closed")
)

const maxTextBytes = 1 << 20
const maxReplayBytes = 1 << 20
const maxEventBytes = 512 << 10
const maxRetainedRuns = 16
const maxSubscribers = 8
const subscriberBuffer = 8

type RunSnapshot struct {
	ID        string
	Status    RunStatus
	Text      string
	Error     string
	HighWater uint64
	OldestSeq uint64
}

// Status is nonempty only on the controller-owned terminal notification.
type RecordedEvent struct {
	Seq    uint64
	Event  abi.Event
	Status RunStatus
	Error  string
}
type runState struct {
	snapshot    RunSnapshot
	events      []RecordedEvent
	eventSizes  []int
	replayBytes int
	subscribers map[uint64]chan RecordedEvent
	cancel      context.CancelFunc
	done        chan struct{}
}

type Controller struct {
	mu             sync.Mutex
	runner         harness.Runner
	maxEvents      int
	nextRun        uint64
	nextSubscriber uint64
	runs           map[string]*runState
	order          []string
	active         string
	closed         bool
}

func NewController(runner harness.Runner, maxEvents int) *Controller {
	if maxEvents < 1 {
		maxEvents = 256
	}
	if maxEvents > 4096 {
		maxEvents = 4096
	}
	return &Controller{runner: runner, maxEvents: maxEvents, runs: make(map[string]*runState)}
}

func (c *Controller) StartPrompt(ctx context.Context, text string) (string, error) {
	if text == "" {
		return "", errors.New("prompt is empty")
	}
	return c.Start(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, text)})
}

// ctx controls admission only. Stop/Close own cancellation after admission.
// Input is the NEW batch, not a replay of the presentation transcript.
func (c *Controller) Start(ctx context.Context, input []wire.Message) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if len(input) == 0 {
		return "", errors.New("input batch is empty")
	}
	for _, message := range input {
		if err := message.Validate(); err != nil {
			return "", err
		}
	}
	input = wire.CloneMessages(input)
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return "", ErrControllerClosed
	}
	if c.active != "" {
		return "", ErrRunActive
	}
	if nilRunner(c.runner) {
		return "", errors.New("runner is nil")
	}
	c.nextRun++
	id := fmt.Sprintf("run-%d", c.nextRun)
	runCtx, cancel := context.WithCancel(context.Background())
	state := &runState{snapshot: RunSnapshot{ID: id, Status: RunRunning}, subscribers: make(map[uint64]chan RecordedEvent), cancel: cancel, done: make(chan struct{})}
	c.runs[id] = state
	c.order = append(c.order, id)
	c.active = id
	for len(c.order) > maxRetainedRuns {
		delete(c.runs, c.order[0])
		c.order = c.order[1:]
	}
	go c.execute(runCtx, id, state, input)
	return id, nil
}

func nilRunner(runner harness.Runner) bool {
	if runner == nil {
		return true
	}
	value := reflect.ValueOf(runner)
	switch value.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		return value.IsNil()
	}
	return false
}

func (c *Controller) execute(ctx context.Context, id string, state *runState, input []wire.Message) {
	var callbackErr error
	err := c.runner.Run(ctx, input, func(event abi.Event) error {
		c.mu.Lock()
		defer c.mu.Unlock()
		if callbackErr != nil {
			return callbackErr
		}
		if ctx.Err() != nil {
			return ctx.Err()
		}
		encoded, encodeErr := json.Marshal(event)
		if encodeErr != nil {
			callbackErr = errors.New("run event is not valid JSON")
			return callbackErr
		}
		if len(encoded) > maxEventBytes {
			callbackErr = errors.New("run event exceeds 512 KiB presentation limit")
			return callbackErr
		}
		if event.Kind == abi.EventTextDelta {
			if len(state.snapshot.Text)+len(event.Text) > maxTextBytes {
				callbackErr = errors.New("run presentation output exceeds 1 MiB limit")
				return callbackErr
			}
			state.snapshot.Text += event.Text
		}
		if event.Kind == abi.EventMessageCommitted && event.Message != nil && event.Message.Role == wire.RoleAssistant {
			text := event.Message.Text()
			for _, block := range event.Message.Content {
				if block.Kind == wire.ContentToolCall {
					text = ""
					break
				}
			}
			if len(text) > maxTextBytes {
				callbackErr = errors.New("committed run presentation output exceeds 1 MiB limit")
				return callbackErr
			}
			// A committed response is authoritative after AfterModel changes.
			// Tool-call prefaces and reasoning are not a final plain answer.
			state.snapshot.Text = text
		}
		c.recordLocked(state, RecordedEvent{Event: event}, len(encoded))
		return nil
	})
	c.mu.Lock()
	defer c.mu.Unlock()
	if callbackErr != nil && !errors.Is(err, callbackErr) {
		err = errors.Join(err, callbackErr)
	}
	status := RunCompleted
	if abi.OnlyCancellation(err) {
		status = RunCancelled
	} else if err != nil {
		status = RunFailed
	}
	state.snapshot.Status = status
	if err != nil && status != RunCancelled {
		state.snapshot.Error = err.Error()
		if len(state.snapshot.Error) > 4096 {
			state.snapshot.Error = "run failed; error exceeds presentation limit"
		}
	}
	c.recordLocked(state, RecordedEvent{Status: status, Error: state.snapshot.Error}, len(state.snapshot.Error)+128)
	for subscriberID, subscriber := range state.subscribers {
		close(subscriber)
		delete(state.subscribers, subscriberID)
	}
	state.cancel()
	close(state.done)
	if c.active == id {
		c.active = ""
	}
}

func (c *Controller) recordLocked(state *runState, event RecordedEvent, size int) {
	state.snapshot.HighWater++
	event.Seq = state.snapshot.HighWater
	event = cloneEvent(event)
	state.events = append(state.events, event)
	state.eventSizes = append(state.eventSizes, size)
	state.replayBytes += size
	for len(state.events) > c.maxEvents || state.replayBytes > maxReplayBytes {
		state.replayBytes -= state.eventSizes[0]
		state.events[0] = RecordedEvent{}
		state.events = state.events[1:]
		state.eventSizes = state.eventSizes[1:]
	}
	state.snapshot.OldestSeq = state.events[0].Seq
	for id, subscriber := range state.subscribers {
		select {
		case subscriber <- cloneEvent(event):
		default:
			close(subscriber)
			delete(state.subscribers, id)
		}
	}
}

func (c *Controller) Snapshot(id string) (RunSnapshot, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	state, ok := c.runs[id]
	if !ok {
		return RunSnapshot{}, ErrRunNotFound
	}
	return state.snapshot, nil
}

// Slow subscribers are closed, never allowed to block execution. A caller then
// reads Snapshot and explicitly resubscribes; this is bounded ephemeral replay.
func (c *Controller) Subscribe(id string, after uint64) (<-chan RecordedEvent, func(), error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	state, ok := c.runs[id]
	if !ok {
		return nil, nil, ErrRunNotFound
	}
	if after > state.snapshot.HighWater {
		return nil, nil, errors.New("replay cursor is ahead of run")
	}
	if state.snapshot.OldestSeq > 0 && after < state.snapshot.OldestSeq-1 {
		return nil, nil, ErrHistoryExpired
	}
	replayCount := 0
	for _, event := range state.events {
		if event.Seq > after {
			replayCount++
		}
	}
	if replayCount > subscriberBuffer {
		return nil, nil, ErrHistoryExpired
	}
	if state.snapshot.Status == RunRunning && len(state.subscribers) >= maxSubscribers {
		return nil, nil, errors.New("run observer limit reached")
	}
	channel := make(chan RecordedEvent, subscriberBuffer)
	for _, event := range state.events {
		if event.Seq > after {
			channel <- cloneEvent(event)
		}
	}
	if state.snapshot.Status != RunRunning {
		close(channel)
		return channel, func() {}, nil
	}
	c.nextSubscriber++
	subscriberID := c.nextSubscriber
	state.subscribers[subscriberID] = channel
	var once sync.Once
	unsubscribe := func() {
		once.Do(func() {
			c.mu.Lock()
			defer c.mu.Unlock()
			if subscriber, ok := state.subscribers[subscriberID]; ok {
				close(subscriber)
				delete(state.subscribers, subscriberID)
			}
		})
	}
	return channel, unsubscribe, nil
}

func (c *Controller) Stop(id string) error {
	c.mu.Lock()
	defer c.mu.Unlock()
	state, ok := c.runs[id]
	if !ok {
		return ErrRunNotFound
	}
	if state.snapshot.Status == RunRunning {
		state.cancel()
	}
	return nil
}

// Close cancels the root and waits for its owned execution to exit. If ctx expires,
// cancellation remains requested and later Close may wait again.
func (c *Controller) Close(ctx context.Context) error {
	c.mu.Lock()
	c.closed = true
	var done <-chan struct{}
	if state, ok := c.runs[c.active]; ok {
		state.cancel()
		done = state.done
	}
	c.mu.Unlock()
	if done == nil {
		return nil
	}
	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func cloneEvent(event RecordedEvent) RecordedEvent {
	if event.Event.Message != nil {
		message := wire.CloneMessages([]wire.Message{*event.Event.Message})[0]
		event.Event.Message = &message
	}
	if event.Event.ContentBlock != nil {
		block := wire.CloneContent([]wire.ContentBlock{*event.Event.ContentBlock})[0]
		event.Event.ContentBlock = &block
	}
	if event.Event.ToolCall != nil {
		call := *event.Event.ToolCall
		call.Arguments = append([]byte(nil), call.Arguments...)
		event.Event.ToolCall = &call
	}
	if event.Event.ToolResult != nil {
		result := *event.Event.ToolResult
		result.Content = wire.CloneContent(result.Content)
		event.Event.ToolResult = &result
	}
	if event.Event.Record != nil {
		record := *event.Event.Record
		event.Event.Record = &record
	}
	if event.Event.Usage != nil {
		usage := wire.CloneUsage(*event.Event.Usage)
		event.Event.Usage = &usage
	}
	return event
}
