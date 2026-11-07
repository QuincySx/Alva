// INPUT: Explicit recording requests, cancellation and an owned native recognition driver.
// OUTPUT: Bounded local dictation snapshots and independently joined Stop/Cancel lifecycle.
// POS: [platform / OS primitive] Microphone text only; no model, network fallback, storage or task execution.
package speech

import (
	"context"
	"errors"
	"strings"
	"sync"
	"time"
	"unicode/utf8"
)

var ErrUnsupported = errors.New("系统本地语音识别在此平台不可用")
var ErrPermissionDenied = errors.New("请在系统隐私设置中允许 Alva 使用麦克风和语音识别")
var ErrLocalUnavailable = errors.New("当前语言的系统本地语音模型不可用；未切换到云端识别")

type State string

const (
	Requesting   State = "requesting"
	Recording    State = "recording"
	Transcribing State = "transcribing"
	Completed    State = "completed"
	Cancelled    State = "cancelled"
	Failed       State = "failed"
)

type Options struct {
	Locale          string
	MaximumDuration time.Duration
}

type Snapshot struct {
	State     State
	Text      string
	Problem   string
	StartedAt time.Time
}

// Stop ends microphone capture and requests a final local transcription.
// Cancel discards the result. Done closes only after the native driver returns.
type Recorder interface {
	Snapshot() Snapshot
	Stop()
	Cancel()
	Done() <-chan struct{}
	Close(context.Context) error
}

type capture struct {
	mu       sync.Mutex
	value    Snapshot
	stop     chan struct{}
	done     chan struct{}
	stopOnce sync.Once
	cancel   context.CancelFunc
}

func Start(ctx context.Context, options Options) (Recorder, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if options.Locale == "" {
		options.Locale = "zh-CN"
	}
	if len(options.Locale) > 64 || !utf8.ValidString(options.Locale) || strings.ContainsAny(options.Locale, "\x00\r\n") {
		return nil, errors.New("invalid dictation locale")
	}
	if options.MaximumDuration == 0 {
		options.MaximumDuration = time.Minute
	}
	if options.MaximumDuration < time.Second || options.MaximumDuration > time.Minute {
		return nil, errors.New("dictation duration must be between one and sixty seconds")
	}
	if !Supported() {
		return nil, ErrUnsupported
	}
	return newCapture(ctx, options, runNative), nil
}

type driver func(context.Context, Options, <-chan struct{}, func(State, string) error) error

func newCapture(parent context.Context, options Options, run driver) *capture {
	ctx, cancel := context.WithCancel(parent)
	c := &capture{value: Snapshot{State: Requesting}, stop: make(chan struct{}), done: make(chan struct{}), cancel: cancel}
	go func() {
		defer close(c.done)
		defer cancel()
		err := run(ctx, options, c.stop, func(state State, text string) error {
			if len(text) > 32<<10 || !utf8.ValidString(text) {
				return errors.New("语音文字超过输入上限，请分段录入")
			}
			c.mu.Lock()
			defer c.mu.Unlock()
			if ctx.Err() != nil {
				return ctx.Err()
			}
			c.value.State, c.value.Text = state, text
			if state == Recording && c.value.StartedAt.IsZero() {
				c.value.StartedAt = time.Now()
			}
			return nil
		})
		c.mu.Lock()
		defer c.mu.Unlock()
		switch {
		case ctx.Err() != nil:
			c.value.State, c.value.Text, c.value.Problem = Cancelled, "", ""
		case err != nil:
			c.value.State, c.value.Problem = Failed, err.Error()
		case strings.TrimSpace(c.value.Text) == "":
			c.value.State, c.value.Problem = Failed, "未识别到语音，请重试。"
		default:
			c.value.State = Completed
		}
	}()
	return c
}

func (c *capture) Snapshot() Snapshot    { c.mu.Lock(); defer c.mu.Unlock(); return c.value }
func (c *capture) Stop()                 { c.stopOnce.Do(func() { close(c.stop) }) }
func (c *capture) Cancel()               { c.cancel() }
func (c *capture) Done() <-chan struct{} { return c.done }
func (c *capture) Close(ctx context.Context) error {
	c.Cancel()
	select {
	case <-c.done:
		return nil
	default:
	}
	select {
	case <-c.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
