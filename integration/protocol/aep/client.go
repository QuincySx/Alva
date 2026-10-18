// INPUT: bounded bidirectional AEP JSON-RPC stdio, explicit host callbacks and owned process exit observation
// OUTPUT: Open, Client.Call, Notify, Handshake, Shutdown, Close and CleanupComplete
// POS: [integration / external protocol] Owned Plugin I/O with bounded callback workers, fail-closed recording and no replay
package aep

import (
	"bufio"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type reply struct {
	data json.RawMessage
	err  error
}
type callback struct {
	id, method string
	params     json.RawMessage
}
type Client struct {
	config            Config
	p                 *process.Process
	handler           Handler
	observer          Observer
	ctx               context.Context
	cancel            context.CancelFunc
	mu                sync.Mutex
	once              sync.Once
	done, workersDone chan struct{}
	ioDone            chan struct{}
	streams           sync.WaitGroup
	shutdownGate      chan struct{}
	shutting          bool
	shutdownID        string
	shutdownACK       bool
	failure           error
	observerFailure   error
	write, gate       chan struct{}
	queue             chan callback
	wg                sync.WaitGroup
	next              uint64
	pending           string
	response          chan reply
	seen              map[string]bool
	callbacks         int
	callbacksTotal    int
	callbackBytes     int
	grants            map[string]bool
	handshake         Handshake
}

func Open(ctx context.Context, config Config, handler Handler, observer Observer) (*Client, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if config.Name == "" || len(config.Name) > 64 || config.PluginVersion == "" || len(config.PluginVersion) > 64 || len(config.RequestedCapabilities) > 32 || len(config.HostCapabilities.HostAPI) > 32 || len(config.HostCapabilities.Events) > 32 || len(config.HostCapabilities.StateAccess) > 32 {
		return nil, errors.New("invalid bounded AEP identity/capabilities")
	}
	config.RequestedCapabilities = append([]string(nil), config.RequestedCapabilities...)
	config.HostCapabilities.StateAccess = append([]string(nil), config.HostCapabilities.StateAccess...)
	config.HostCapabilities.Events = append([]string(nil), config.HostCapabilities.Events...)
	config.HostCapabilities.HostAPI = append([]string(nil), config.HostCapabilities.HostAPI...)
	for _, list := range [][]string{config.HostCapabilities.Events, config.HostCapabilities.StateAccess} {
		seen := map[string]bool{}
		for _, value := range list {
			if value == "" || len(value) > 256 || !utf8.ValidString(value) || strings.ContainsAny(value, "\x00\r\n") || seen[value] {
				return nil, errors.New("invalid duplicate/bounded AEP host capability")
			}
			seen[value] = true
		}
	}
	if config.ConnectTimeout == 0 {
		config.ConnectTimeout = 10 * time.Second
	}
	if config.RequestTimeout == 0 {
		config.RequestTimeout = time.Minute
	}
	if config.ConnectTimeout < time.Millisecond || config.ConnectTimeout > time.Minute || config.RequestTimeout < time.Millisecond || config.RequestTimeout > 10*time.Minute {
		return nil, errors.New("AEP timeouts outside bounds")
	}
	grants := map[string]bool{}
	offered := map[string]bool{}
	for _, name := range config.HostCapabilities.HostAPI {
		if name == "" || len(name) > 128 || strings.ContainsAny(name, "/\x00\r\n") || offered[name] {
			return nil, errors.New("invalid offered AEP host API")
		}
		offered[name] = true
	}
	for _, capability := range config.RequestedCapabilities {
		name, ok := strings.CutPrefix(capability, "host:")
		if !ok || !offered[name] || grants["host/"+name] || handler == nil {
			return nil, errors.New("AEP capability is undeclared, duplicated or unsupported")
		}
		grants["host/"+name] = true
	}
	p, err := process.Start(config.Process)
	if err != nil {
		return nil, err
	}
	worker, cancel := context.WithCancel(context.Background())
	c := &Client{config: config, p: p, handler: handler, observer: observer, ctx: worker, cancel: cancel, done: make(chan struct{}), workersDone: make(chan struct{}), write: make(chan struct{}, 1), gate: make(chan struct{}, 1), queue: make(chan callback, 128), grants: grants, seen: map[string]bool{}}
	c.ioDone, c.shutdownGate = make(chan struct{}), make(chan struct{}, 1)
	c.streams.Add(2)
	go func() { c.streams.Wait(); close(c.ioDone) }()
	c.wg.Add(10)
	go c.stdout()
	go c.stderr()
	for range 8 {
		go c.callbackWorker()
	}
	go func() { c.wg.Wait(); close(c.workersDone) }()
	connect, stop := context.WithTimeout(ctx, config.ConnectTimeout)
	defer stop()
	data, err := c.Call(connect, "initialize", map[string]any{"protocolVersion": Version, "hostInfo": map[string]string{"name": "alva", "version": "go-v1"}, "hostCapabilities": config.HostCapabilities})
	if err == nil {
		if json.Unmarshal(data, &c.handshake) != nil {
			err = errors.New("invalid AEP handshake")
		} else {
			err = c.validateHandshake()
		}
	}
	if err == nil {
		err = c.Notify(connect, "initialized", nil)
	}
	if err != nil {
		cleanup, end := context.WithTimeout(context.Background(), 5*time.Second)
		defer end()
		return nil, errors.Join(err, c.Close(cleanup))
	}
	return c, nil
}
func (c *Client) validateHandshake() error {
	h := c.handshake
	if h.ProtocolVersion != Version || h.Plugin.Name != c.config.Name || h.Plugin.Version != c.config.PluginVersion || len(h.Tools) > 1024 || len(h.Subscriptions) > 32 || len(h.Capabilities) > 32 {
		return errors.New("AEP handshake identity/version/budget mismatch")
	}
	seen := map[string]bool{}
	total := 0
	for _, tool := range h.Tools {
		var schema map[string]any
		if tool.Name == "" || len(tool.Name) > 256 || seen[tool.Name] || len(tool.Description) > 16<<10 || len(tool.InputSchema) > 64<<10 || json.Unmarshal(tool.InputSchema, &schema) != nil || schema == nil {
			return errors.New("invalid AEP tool declaration")
		}
		seen[tool.Name] = true
		total += len(tool.Description) + len(tool.InputSchema)
		if total > 8<<20 {
			return errors.New("AEP tool catalog exceeds 8 MiB")
		}
	}
	seen = map[string]bool{}
	for _, capability := range h.Capabilities {
		name, ok := strings.CutPrefix(capability, "host:")
		if !ok || !c.grants["host/"+name] || seen[capability] {
			return errors.New("AEP handshake expanded granted capabilities")
		}
		seen[capability] = true
	}
	if len(seen) != len(c.grants) {
		return errors.New("AEP handshake capability set differs from approved manifest")
	}
	seen = map[string]bool{}
	for _, event := range h.Subscriptions {
		allowed := false
		for _, offered := range c.config.HostCapabilities.Events {
			allowed = allowed || offered == event
		}
		if !allowed || seen[event] {
			return errors.New("AEP handshake expanded event subscriptions")
		}
		seen[event] = true
	}
	return nil
}
func (c *Client) Handshake() Handshake {
	data, _ := json.Marshal(c.handshake)
	var value Handshake
	_ = json.Unmarshal(data, &value)
	return value
}
func (c *Client) fail(err error) {
	c.once.Do(func() {
		c.mu.Lock()
		c.failure = err
		c.mu.Unlock()
		close(c.done)
		c.cancel()
		c.p.BeginClose()
	})
}
func (c *Client) record(direction string, data []byte) error {
	// Already observed bytes outlive stream cancellation. A bounded independent
	// journal context avoids manufacturing a storage failure by cancelling it
	// ourselves while Close is waiting for these exact workers.
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(c.ctx), 5*time.Second)
	defer cancel()
	return c.recordContext(cleanup, direction, data)
}
func (c *Client) recordContext(ctx context.Context, direction string, data []byte) error {
	if c.observer == nil {
		return nil
	}
	if err := c.observer.Record(ctx, IOEvent{Direction: direction, Frame: append(json.RawMessage(nil), data...), At: time.Now().UTC()}); err != nil {
		failure := &observerError{cause: err}
		c.mu.Lock()
		c.observerFailure = errors.Join(c.observerFailure, failure)
		c.mu.Unlock()
		return failure
	}
	return nil
}
func (c *Client) send(ctx context.Context, value any) error {
	data, err := json.Marshal(value)
	if err != nil || len(data) > FrameLimit || !wire.ValidJSON(data) {
		return errors.New("invalid/oversize AEP outgoing frame")
	}
	select {
	case c.write <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	case <-c.done:
		return errors.New("AEP closed")
	}
	defer func() { <-c.write }()
	if err := ctx.Err(); err != nil {
		return err
	}
	select {
	case <-c.done:
		return errors.New("AEP closed")
	default:
	}
	finished := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			select {
			case <-finished:
				return
			default:
				c.fail(ctx.Err())
			}
		case <-finished:
		}
	}()
	defer close(finished)
	if err = c.record("outgoing_attempt", data); err != nil {
		c.fail(err)
		return err
	}
	n, err := c.p.Stdin.Write(append(data, '\n'))
	if err != nil || n != len(data)+1 {
		c.fail(errors.New("AEP stdin write failed"))
		return errors.New("AEP stdin write failed")
	}
	return nil
}
func (c *Client) Call(ctx context.Context, method string, params any) (json.RawMessage, error) {
	return c.call(ctx, method, params, false)
}
func (c *Client) call(ctx context.Context, method string, params any, shutdown bool) (json.RawMessage, error) {
	if method == "" || len(method) > 256 {
		return nil, errors.New("invalid AEP request method")
	}
	select {
	case c.gate <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-c.done:
		return nil, errors.New("AEP closed")
	}
	defer func() { <-c.gate }()
	ctx, cancel := context.WithTimeout(ctx, c.config.RequestTimeout)
	defer cancel()
	c.mu.Lock()
	if c.shutting && !shutdown {
		c.mu.Unlock()
		return nil, errors.New("AEP shutdown in progress")
	}
	c.next++
	id := fmt.Sprintf("h-%d", c.next)
	if shutdown {
		c.shutdownID = id
	}
	c.pending = id
	c.response = make(chan reply, 1)
	result := c.response
	c.callbacks = 0
	c.mu.Unlock()
	defer func() { c.mu.Lock(); c.pending = ""; c.response = nil; c.mu.Unlock() }()
	if err := c.send(ctx, map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params}); err != nil {
		c.fail(err)
		return nil, errors.Join(err, ctx.Err())
	}
	select {
	case value := <-result:
		if value.err != nil {
			c.fail(value.err)
		}
		return value.data, errors.Join(value.err, ctx.Err())
	case <-ctx.Done():
		c.fail(ctx.Err())
		return nil, ctx.Err()
	case <-c.done:
		c.mu.Lock()
		err := c.failure
		c.mu.Unlock()
		return nil, errors.Join(err, ctx.Err())
	}
}
func (c *Client) Notify(ctx context.Context, method string, params any) error {
	c.mu.Lock()
	shutting := c.shutting
	c.mu.Unlock()
	if shutting {
		return errors.New("AEP shutdown in progress")
	}
	if method == "" || len(method) > 256 {
		return errors.New("invalid AEP notification")
	}
	return c.send(ctx, map[string]any{"jsonrpc": "2.0", "method": method, "params": params})
}
func (c *Client) stdout() {
	defer c.wg.Done()
	defer c.streams.Done()
	reader := bufio.NewReaderSize(c.p.Stdout, 4096)
	for {
		data, readErr := readFrame(reader)
		if readErr != nil {
			c.mu.Lock()
			expectedEOF := c.shutdownACK
			c.mu.Unlock()
			if expectedEOF && len(data) == 0 && errors.Is(readErr, io.EOF) {
				return
			}
			if len(data) > 0 {
				value := map[string]any{"complete": false, "truncated": len(data) > FrameLimit}
				if utf8.Valid(data) {
					value["text"] = string(data)
				} else {
					value["base64"] = base64.StdEncoding.EncodeToString(data)
				}
				encoded, _ := json.Marshal(value)
				cleanup, stop := context.WithTimeout(context.Background(), time.Second)
				err := c.recordContext(cleanup, "incomplete_incoming", encoded)
				stop()
				if err != nil {
					c.fail(err)
					return
				}
			}
			c.fail(errors.New("AEP stdout ended, incomplete or frame limit exceeded"))
			return
		}
		if !utf8.Valid(data) || !wire.ValidJSON(data) {
			value := map[string]any{"complete": true}
			if utf8.Valid(data) {
				value["text"] = string(data)
			} else {
				value["base64"] = base64.StdEncoding.EncodeToString(data)
			}
			encoded, _ := json.Marshal(value)
			if err := c.record("invalid_incoming", encoded); err != nil {
				c.fail(err)
				return
			}
			c.fail(errors.New("invalid AEP UTF-8/JSON frame"))
			return
		}
		if err := c.record("incoming", data); err != nil {
			c.fail(err)
			return
		}
		var envelope map[string]json.RawMessage
		if json.Unmarshal(data, &envelope) != nil || string(envelope["jsonrpc"]) != `"2.0"` {
			c.fail(errors.New("invalid AEP envelope"))
			return
		}
		var id, method string
		if len(envelope["id"]) > 0 && json.Unmarshal(envelope["id"], &id) != nil {
			c.fail(errors.New("AEP requires string request IDs"))
			return
		}
		if len(envelope["method"]) > 0 {
			if json.Unmarshal(envelope["method"], &method) != nil || method == "" || len(method) > 256 || len(id) > 128 || (id != "" && !strings.HasPrefix(id, "p-")) || (len(envelope["id"]) > 0 && id == "") || len(envelope["params"]) > 1<<20 {
				c.fail(errors.New("invalid AEP reverse call"))
				return
			}
			c.mu.Lock()
			duplicate := id != "" && c.seen[id]
			if id != "" {
				c.seen[id] = true
			}
			c.callbacks++
			c.callbacksTotal++
			c.callbackBytes += len(envelope["params"])
			count := c.callbacks
			total := c.callbacksTotal
			byteBudget := c.callbackBytes
			c.mu.Unlock()
			if duplicate || count > 128 || total > 4096 || byteBudget > 8<<20 {
				c.fail(errors.New("AEP reverse callback identity/budget exceeded"))
				return
			}
			select {
			case c.queue <- callback{id: id, method: method, params: envelope["params"]}:
			case <-c.done:
				return
			default:
				c.fail(errors.New("AEP callback queue exceeded"))
				return
			}
			continue
		}
		_, hasResult := envelope["result"]
		_, hasError := envelope["error"]
		c.mu.Lock()
		pending, response := c.pending, c.response
		c.mu.Unlock()
		if id == "" || id != pending || response == nil || hasResult == hasError {
			c.fail(errors.New("unexpected AEP response"))
			return
		}
		value := reply{data: envelope["result"]}
		if hasError {
			value.err = errors.New("AEP remote operation failed; inspect retained transcript")
		}
		c.mu.Lock()
		if id == c.shutdownID && !hasError {
			c.shutdownACK = true
		}
		c.mu.Unlock()
		select {
		case response <- value:
		default:
			c.fail(errors.New("duplicate AEP response"))
			return
		}
	}
}

func readFrame(reader *bufio.Reader) ([]byte, error) {
	var data []byte
	for {
		part, err := reader.ReadSlice('\n')
		remaining := FrameLimit + 1 - len(data)
		if len(part) > remaining {
			part = part[:remaining]
		}
		data = append(data, part...)
		if len(data) > FrameLimit {
			return data, errors.New("AEP frame limit exceeded")
		}
		if err == bufio.ErrBufferFull {
			continue
		}
		if err != nil {
			return data, err
		}
		return data[:len(data)-1], nil
	}
}
func (c *Client) stderr() {
	defer c.streams.Done()
	defer c.wg.Done()
	err := process.ReadLines(c.p.Stderr, 1<<20, func(line process.StreamLine) error {
		value := map[string]any{"complete": line.Complete, "truncated": line.Truncated}
		if utf8.Valid(line.Data) {
			value["text"] = string(line.Data)
		} else {
			value["base64"] = base64.StdEncoding.EncodeToString(line.Data)
		}
		data, _ := json.Marshal(value)
		if c.ctx.Err() != nil || !line.Complete {
			cleanup, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			return c.recordContext(cleanup, "stderr", data)
		}
		return c.record("stderr", data)
	})
	if err != nil && c.ctx.Err() == nil {
		c.fail(errors.Join(errors.New("AEP stderr framing/observer failed"), err))
	}
}
func (c *Client) callbackWorker() {
	defer c.wg.Done()
	for {
		select {
		case <-c.done:
			return
		case call := <-c.queue:
			c.handleCallback(call)
		}
	}
}
func (c *Client) handleCallback(call callback) {
	defer func() { c.mu.Lock(); c.callbackBytes -= len(call.params); c.mu.Unlock() }()
	if c.ctx.Err() != nil {
		return
	}
	response := map[string]any{"jsonrpc": "2.0", "id": call.id}
	if !c.grants[call.method] || c.handler == nil {
		response["error"] = map[string]any{"code": -32001, "message": "host capability denied"}
	} else {
		ctx, cancel := context.WithTimeout(c.ctx, c.config.RequestTimeout)
		value, err := c.handler.Handle(ctx, call.method, append(json.RawMessage(nil), call.params...))
		cancel()
		if err != nil {
			code := -32603
			var public *CallbackError
			if errors.As(err, &public) {
				switch public.Code {
				case HandleExpired, CapabilityDenied, InvalidParams, MethodNotFound:
					code = public.Code
				}
			}
			response["error"] = map[string]any{"code": code, "message": "host callback failed"}
		} else if len(value) > FrameLimit/2 || !wire.ValidJSON(value) {
			c.fail(errors.New("invalid/oversize AEP callback result"))
			return
		} else {
			response["result"] = value
		}
	}
	// Authorized notifications use the same bounded workers and handler, but
	// cannot fabricate a response ID or acquire any additional host capability.
	if call.id == "" {
		return
	}
	ctx, cancel := context.WithTimeout(c.ctx, c.config.RequestTimeout)
	err := c.send(ctx, response)
	cancel()
	if err != nil {
		c.fail(err)
		return
	}
}
func (c *Client) Close(ctx context.Context) error {
	c.fail(errors.New("AEP closed by owner"))
	err := c.p.Close(ctx)
	select {
	case <-c.workersDone:
		c.mu.Lock()
		observation := c.observerFailure
		c.mu.Unlock()
		return errors.Join(err, observation)
	case <-ctx.Done():
		c.mu.Lock()
		observation := c.observerFailure
		c.mu.Unlock()
		return errors.Join(err, ctx.Err(), observation)
	}
}

// Shutdown gives a healthy idle peer a bounded protocol shutdown and drains its
// final output before the unique process owner reaps it. Any failure still
// forces cleanup; cancellation/failed peers use Close directly. This never
// converts an acknowledgement or EOF into proof of process-group cleanup.
func (c *Client) Shutdown(ctx context.Context) error {
	select {
	case c.shutdownGate <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	}
	defer func() { <-c.shutdownGate }()
	c.mu.Lock()
	already, failed := c.shutting, c.failure != nil
	if !already && !failed {
		c.shutting = true
	}
	c.mu.Unlock()
	if already || failed || ctx.Err() != nil {
		return c.Close(ctx)
	}
	grace, cancel := context.WithTimeout(ctx, 2*time.Second)
	defer cancel()
	_, err := c.call(grace, "shutdown", nil, true)
	if err == nil {
		_ = c.p.Stdin.Close()
		select {
		case <-c.ioDone:
		case <-grace.Done():
			err = grace.Err()
		}
		if err == nil {
			err = c.p.Wait(grace)
		}
		if err == nil {
			if code, ok := c.p.ExitCode(); !ok || code != 0 {
				err = errors.New("AEP graceful shutdown did not exit successfully")
			}
		}
	}
	return errors.Join(err, c.Close(ctx))
}

// CleanupComplete separates actual worker/process cleanup from a retained
// journal failure, so callers can reclaim owned snapshots without hiding it.
func (c *Client) CleanupComplete() bool {
	select {
	case <-c.workersDone:
		return c.p.CleanupComplete()
	default:
		return false
	}
}

type observerError struct{ cause error }

func (*observerError) Error() string   { return "AEP observer failed; interaction stopped" }
func (e *observerError) Unwrap() error { return e.cause }
