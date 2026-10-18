// INPUT: bufio, bytes, context, encoding/json, errors, fmt, io, strconv, sync/atomic, time, unicode/utf8, llm/wire, native/process
// OUTPUT: Client, Open, Initialize, Authenticate, Connected, Close, Done, Err
// POS: [integration / external protocol] Bounded bidirectional official ACP v1 JSON-RPC with owned native process and fail-closed observation.
package acp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strconv"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/process"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type rpcReply struct {
	data  json.RawMessage
	err   error
	bytes int
}
type pendingCall struct {
	reply  chan rpcReply
	accept func(json.RawMessage) error
}
type writeRequest struct {
	data    []byte
	reply   chan error
	prepare func() ([]byte, error)
}
type callbackRequest struct {
	id        json.RawMessage
	key       string
	params    json.RawMessage
	session   *Session
	turn      *promptTurn
	request   PermissionRequest
	mu        sync.Mutex
	responded bool
	committed atomic.Bool
}
type Client struct {
	config            Config
	p                 *process.Process
	observer          Observer
	handler           PermissionHandler
	ctx               context.Context
	cancel            context.CancelFunc
	done, workersDone chan struct{}
	closed            chan struct{}
	once              sync.Once
	wg                sync.WaitGroup
	mu                sync.Mutex
	observation       sync.Mutex
	failure           error
	closing           bool
	observerFailure   error
	handlerFailure    error
	initialized       bool
	initialization    InitializeResponse
	next              uint64
	pending           map[string]pendingCall
	sessions          map[string]*Session
	callbackIDs       map[string]*callbackRequest
	queuedBytes       int
	writes            chan writeRequest
	callbacks         chan *callbackRequest
}

func Open(ctx context.Context, config Config, handler PermissionHandler, observer Observer) (*Client, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if !identity(config.ID) {
		return nil, errors.New("explicit ACP connection identity required")
	}
	if config.MaxFrameBytes == 0 {
		config.MaxFrameBytes = 1 << 20
	}
	if config.MaxFrameBytes < 1024 || config.MaxFrameBytes > 4<<20 {
		return nil, errors.New("ACP frame limit must be 1 KiB..4 MiB")
	}
	if config.ConnectTimeout == 0 {
		config.ConnectTimeout = 10 * time.Second
	}
	if config.RequestTimeout == 0 {
		config.RequestTimeout = 10 * time.Minute
	}
	if config.CancelTimeout == 0 {
		config.CancelTimeout = 5 * time.Second
	}
	if config.ObserverTimeout == 0 {
		config.ObserverTimeout = 5 * time.Second
	}
	if config.ConnectTimeout < time.Millisecond || config.ConnectTimeout > time.Minute || config.RequestTimeout < time.Millisecond || config.RequestTimeout > 30*time.Minute || config.CancelTimeout < time.Millisecond || config.CancelTimeout > time.Minute || config.ObserverTimeout < time.Millisecond || config.ObserverTimeout > time.Minute {
		return nil, errors.New("ACP timeout bounds invalid")
	}
	if !identity(config.ClientInfo.Name) || !identity(config.ClientInfo.Version) || !utf8.ValidString(config.ClientInfo.Title) || len(config.ClientInfo.Title) > 1024 {
		return nil, errors.New("explicit ACP client implementation name/version required")
	}
	p, err := process.Start(config.Process)
	if err != nil {
		return nil, err
	}
	worker, cancel := context.WithCancel(context.Background())
	c := &Client{config: config, p: p, observer: observer, handler: handler, ctx: worker, cancel: cancel, done: make(chan struct{}), workersDone: make(chan struct{}), closed: make(chan struct{}), pending: map[string]pendingCall{}, sessions: map[string]*Session{}, callbackIDs: map[string]*callbackRequest{}, writes: make(chan writeRequest, 16), callbacks: make(chan *callbackRequest, 32)}
	c.wg.Add(7)
	go c.stdout()
	go c.stderr()
	go c.writer()
	for range 4 {
		go c.callbackWorker()
	}
	go func() { c.wg.Wait(); close(c.workersDone); <-p.Done(); close(c.closed) }()
	connect, stop := context.WithTimeout(ctx, config.ConnectTimeout)
	defer stop()
	_, err = c.request(connect, "initialize", InitializeRequest{ProtocolVersion: Version, ClientInfo: &config.ClientInfo}, func(data json.RawMessage) error {
		var response InitializeResponse
		if len(data) > 64<<10 || json.Unmarshal(data, &response) != nil || response.ProtocolVersion != Version {
			return errors.New("unsupported or malformed ACP protocol version")
		}
		c.mu.Lock()
		defer c.mu.Unlock()
		c.initialization = response
		c.initialized = true
		return nil
	}, nil)
	if err != nil {
		return nil, errors.Join(err, c.cleanup())
	}
	return c, nil
}
func (c *Client) Connected() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.initialized && c.failure == nil
}
func (c *Client) Initialize() InitializeResponse {
	c.mu.Lock()
	defer c.mu.Unlock()
	data, _ := json.Marshal(c.initialization)
	var result InitializeResponse
	_ = json.Unmarshal(data, &result)
	return result
}
func (c *Client) Authenticate(ctx context.Context, methodID string) error {
	if !identity(methodID) {
		return errors.New("invalid authentication method")
	}
	init := c.Initialize()
	found := false
	for _, raw := range init.AuthMethods {
		var method struct{ ID, Type string }
		if json.Unmarshal(raw, &method) == nil && method.ID == methodID && (method.Type == "" || method.Type == "agent") {
			found = true
		}
	}
	if !found {
		return errors.New("agent did not advertise this authenticate method")
	}
	_, err := c.request(ctx, "authenticate", AuthenticateRequest{MethodID: methodID}, objectResult, nil)
	return err
}
func (c *Client) fail(err error) {
	c.mu.Lock()
	if err != nil && !errors.Is(c.failure, err) {
		c.failure = errors.Join(c.failure, err)
	}
	c.closing = true
	c.initialized = false
	c.mu.Unlock()
	c.once.Do(func() {
		close(c.done)
		c.cancel()
		c.p.BeginClose()
	})
}
func (c *Client) failureError() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.failure == nil {
		return ErrClosed
	}
	return c.failure
}
func (c *Client) Close(ctx context.Context) error {
	c.fail(nil)
	err := c.p.Close(ctx)
	select {
	case <-c.closed:
		return errors.Join(err, c.sideFailures())
	case <-ctx.Done():
		return errors.Join(err, c.sideFailures(), fmt.Errorf("ACP workers not finished: %w", ctx.Err()))
	}
}
func (c *Client) sideFailures() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	return errors.Join(c.observerFailure, c.handlerFailure)
}
func (c *Client) cleanup() error {
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	err := c.Close(ctx)
	return errors.Join(err, c.Err())
}

// Done closes only after owned workers and the native process really exit.
// Err is final then; an earlier snapshot may gain cleanup/observer failures.
func (c *Client) Done() <-chan struct{} { return c.closed }
func (c *Client) Err() error            { c.mu.Lock(); defer c.mu.Unlock(); return c.failure }
func (c *Client) reserve(bytes int) bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	if bytes < 0 || bytes > maxQueuedBytes-c.queuedBytes {
		return false
	}
	c.queuedBytes += bytes
	return true
}
func (c *Client) release(bytes int) { c.mu.Lock(); c.queuedBytes -= bytes; c.mu.Unlock() }
func (c *Client) observe(direction, stage string, data []byte, written int) error {
	if c.observer == nil {
		return nil
	}
	c.observation.Lock()
	defer c.observation.Unlock()
	// Ownership cancellation must not manufacture a journal failure or discard
	// bytes already read from the child. The independent deadline remains real:
	// an observer that ignores it keeps workers pending, never fake-completed.
	ctx, cancel := context.WithTimeout(context.WithoutCancel(c.ctx), c.config.ObserverTimeout)
	defer cancel()
	err := c.observer.Observe(ctx, IOEvent{ConnectionID: c.config.ID, Direction: direction, Stage: stage, Data: append([]byte(nil), data...), BytesWritten: written, Time: time.Now().UTC()})
	if err != nil {
		failure := &ObserverError{Err: err}
		c.mu.Lock()
		c.observerFailure = errors.Join(c.observerFailure, failure)
		c.mu.Unlock()
		return failure
	}
	return nil
}
func (c *Client) send(ctx context.Context, value any, finalize ...func() (any, error)) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	select {
	case <-c.done:
		return c.failureError()
	default:
	}
	data, err := json.Marshal(value)
	if err != nil || !wire.ValidJSON(data) || len(data)+1 > c.config.MaxFrameBytes {
		return errors.New("invalid or oversized ACP outbound frame")
	}
	data = append(data, '\n')
	if !c.reserve(len(data)) {
		return errors.New("ACP queued byte budget exceeded")
	}
	request := writeRequest{data: data, reply: make(chan error, 1)}
	if len(finalize) > 0 {
		request.prepare = func() ([]byte, error) {
			value, err := finalize[0]()
			if err != nil {
				return nil, err
			}
			actual, err := json.Marshal(value)
			if err != nil || !wire.ValidJSON(actual) || len(actual)+1 > len(data) {
				return nil, errors.New("invalid ACP final write projection")
			}
			return append(actual, '\n'), nil
		}
	}
	select {
	case c.writes <- request:
	default:
		c.release(len(data))
		return errors.New("ACP write queue full")
	}
	select {
	case err := <-request.reply:
		return err
	case <-c.done:
		return c.failureError()
	case <-ctx.Done():
		c.fail(ctx.Err())
		return errors.Join(ctx.Err(), c.cleanup())
	}
}
func (c *Client) writer() {
	defer c.wg.Done()
	for {
		select {
		case <-c.ctx.Done():
			return
		case request := <-c.writes:
			actual := request.data
			var err error
			if request.prepare != nil {
				actual, err = request.prepare()
			}
			if err == nil {
				err = c.observe("stdin", "write_attempt", actual, 0)
			}
			if err == nil {
				var n int
				n, err = c.p.Stdin.Write(actual)
				if err == nil && n != len(actual) {
					err = io.ErrShortWrite
				}
				err = errors.Join(err, c.observe("stdin", "write_result", nil, n))
			}
			c.release(len(request.data))
			request.reply <- err
			if err != nil {
				c.fail(err)
				return
			}
		}
	}
}
func (c *Client) request(ctx context.Context, method string, params any, accept func(json.RawMessage) error, session *Session) (json.RawMessage, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	deadline, stop := context.WithTimeout(ctx, c.config.RequestTimeout)
	defer stop()
	ctx = deadline
	c.mu.Lock()
	if c.closing {
		err := c.failure
		if err == nil {
			err = ErrClosed
		}
		c.mu.Unlock()
		return nil, err
	}
	if len(c.pending) >= 32 {
		c.mu.Unlock()
		return nil, errors.New("ACP active request limit exceeded")
	}
	if c.next >= 1<<63-1 {
		c.mu.Unlock()
		return nil, errors.New("ACP request id space exhausted")
	}
	c.next++
	id := strconv.FormatUint(c.next, 10)
	pending := pendingCall{reply: make(chan rpcReply, 1), accept: accept}
	c.pending[id] = pending
	c.mu.Unlock()
	message := struct {
		JSONRPC string      `json:"jsonrpc"`
		ID      json.Number `json:"id"`
		Method  string      `json:"method"`
		Params  any         `json:"params"`
	}{"2.0", json.Number(id), method, params}
	var turn *promptTurn
	var turnDone <-chan struct{}
	if session != nil {
		session.mu.Lock()
		turn = session.turn
		session.mu.Unlock()
		if turn != nil {
			turnDone = turn.ctx.Done()
		}
	}
	if err := c.send(ctx, message, func() (any, error) {
		if turn != nil && turn.ctx.Err() != nil {
			return nil, turn.ctx.Err()
		}
		return message, nil
	}); err != nil {
		c.fail(err)
		return nil, errors.Join(err, c.cleanup())
	}
	readReply := func(reply rpcReply) (json.RawMessage, error) { c.release(reply.bytes); return reply.data, reply.err }
	select {
	case reply := <-pending.reply:
		return readReply(reply)
	case <-c.done:
		select {
		case reply := <-pending.reply:
			return readReply(reply)
		default:
			return nil, errors.Join(c.failureError(), c.cleanup())
		}
	case <-ctx.Done():
		if session != nil {
			return c.cancelPrompt(pending, session, turn, ctx.Err())
		}
		c.fail(ctx.Err())
		return nil, errors.Join(ctx.Err(), c.cleanup())
	case <-turnDone:
		return c.cancelPrompt(pending, session, turn, context.Canceled)
	}
}
func (c *Client) cancelPrompt(pending pendingCall, session *Session, turn *promptTurn, cause error) (json.RawMessage, error) {
	ctx, cancel := context.WithTimeout(context.Background(), c.config.CancelTimeout)
	defer cancel()
	err := session.cancelTurn(ctx, turn)
	if err == nil {
		select {
		case reply := <-pending.reply:
			c.release(reply.bytes)
			return reply.data, errors.Join(cause, reply.err)
		case <-ctx.Done():
			err = ctx.Err()
		case <-c.done:
			err = c.failureError()
		}
	}
	c.fail(err)
	return nil, errors.Join(cause, err, c.cleanup())
}
func (c *Client) stdout() {
	defer c.wg.Done()
	reader := bufio.NewReaderSize(c.p.Stdout, 4096)
	for {
		var frame []byte
		for {
			part, err := reader.ReadSlice('\n')
			frame = append(frame, part...)
			if len(frame) > c.config.MaxFrameBytes {
				observed := c.observe("stdout", "received_prefix_limit", frame, 0)
				c.fail(errors.Join(errors.New("ACP stdout frame exceeds limit; prefix only observed"), observed))
				return
			}
			if errors.Is(err, bufio.ErrBufferFull) {
				continue
			}
			if err != nil {
				var observed error
				if len(frame) > 0 {
					observed = c.observe("stdout", "received_incomplete", frame, 0)
				}
				if c.ctx.Err() == nil {
					c.fail(errors.Join(fmt.Errorf("ACP stdout ended: %w", err), observed))
				} else if observed != nil {
					c.fail(observed)
				}
				return
			}
			break
		}
		if err := c.observe("stdout", "received", frame, 0); err != nil {
			c.fail(err)
			return
		}
		if err := c.route(bytes.TrimSpace(frame)); err != nil {
			c.fail(err)
			return
		}
	}
}
func (c *Client) stderr() {
	defer c.wg.Done()
	if err := c.readStderr(c.p.Stderr); err != nil {
		c.fail(err)
	}
}
func (c *Client) readStderr(source io.Reader) error {
	err := process.ReadLines(source, 1<<20, func(line process.StreamLine) error {
		stage := "received"
		if line.Truncated {
			stage = "received_truncated"
		} else if !line.Complete {
			stage = "received_partial"
		}
		return c.observe("stderr", stage, line.Data, 0)
	})
	if err == nil {
		return nil
	}
	var observer *ObserverError
	if errors.As(err, &observer) || errors.Is(err, process.ErrLineLimit) {
		return err
	}
	if c.ctx.Err() == nil {
		return fmt.Errorf("ACP stderr failed: %w", err)
	}
	return nil
}
func rpcID(raw json.RawMessage) (string, error) {
	var text string
	if json.Unmarshal(raw, &text) == nil && identity(text) {
		return "s:" + text, nil
	}
	number, err := strconv.ParseInt(string(raw), 10, 64)
	if err != nil {
		return "", errors.New("invalid ACP request id")
	}
	return "n:" + strconv.FormatInt(number, 10), nil
}
func (c *Client) route(frame []byte) error {
	if !wire.ValidJSON(frame) {
		return errors.New("ACP frame is not valid UTF-8 JSON")
	}
	if err := uniqueEnvelopeFields(frame); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(frame, &fields) != nil || fields == nil {
		return errors.New("ACP object frame required")
	}
	var version, method string
	if json.Unmarshal(fields["jsonrpc"], &version) != nil || version != "2.0" {
		return errors.New("ACP jsonrpc=2.0 required")
	}
	if raw, exists := fields["method"]; exists {
		if json.Unmarshal(raw, &method) != nil || !identity(method) || fields["result"] != nil || fields["error"] != nil {
			return errors.New("invalid ACP request envelope")
		}
		if fields["id"] == nil {
			if method == "session/update" {
				return c.deliverUpdate(fields["params"])
			}
			return nil
		}
		key, err := rpcID(fields["id"])
		if err != nil {
			return err
		}
		if method != "session/request_permission" {
			return c.send(c.ctx, struct {
				JSONRPC string          `json:"jsonrpc"`
				ID      json.RawMessage `json:"id"`
				Error   *RPCError       `json:"error"`
			}{"2.0", fields["id"], &RPCError{Code: -32601, Message: "Client method not supported"}})
		}
		return c.queuePermission(fields["id"], key, fields["params"])
	}
	key, err := rpcID(fields["id"])
	if err != nil {
		return err
	}
	if len(key) < 2 || key[:2] != "n:" {
		return errors.New("ACP response id not issued by client")
	}
	c.mu.Lock()
	pending, exists := c.pending[key[2:]]
	if exists {
		delete(c.pending, key[2:])
	}
	c.mu.Unlock()
	if !exists {
		return errors.New("unsolicited or duplicate ACP response")
	}
	result, resultSet := fields["result"]
	failure, errorSet := fields["error"]
	if resultSet == errorSet {
		return errors.New("ACP response requires exactly result or error")
	}
	if !c.reserve(len(result) + len(failure)) {
		return errors.New("ACP response byte budget exceeded")
	}
	reply := rpcReply{data: append(json.RawMessage(nil), result...), bytes: len(result) + len(failure)}
	if errorSet {
		var rpc RPCError
		var members map[string]json.RawMessage
		_ = json.Unmarshal(failure, &members)
		if json.Unmarshal(failure, &rpc) != nil || rpc.Message == "" || members["code"] == nil {
			c.release(reply.bytes)
			return errors.New("invalid ACP RPC error")
		}
		reply.err = &rpc
	} else if pending.accept != nil {
		if err := pending.accept(result); err != nil {
			c.release(reply.bytes)
			return err
		}
	}
	pending.reply <- reply
	return nil
}

func objectResult(data json.RawMessage) error {
	var value map[string]json.RawMessage
	if json.Unmarshal(data, &value) != nil || value == nil {
		return errors.New("ACP response object required")
	}
	return nil
}
func uniqueEnvelopeFields(frame []byte) error {
	decoder := json.NewDecoder(bytes.NewReader(frame))
	start, err := decoder.Token()
	if err != nil || start != json.Delim('{') {
		return errors.New("ACP single object envelope required")
	}
	seen := map[string]bool{}
	for decoder.More() {
		token, err := decoder.Token()
		if err != nil {
			return err
		}
		key, ok := token.(string)
		if !ok || seen[key] {
			return errors.New("duplicate ACP envelope field")
		}
		seen[key] = true
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return err
		}
	}
	return nil
}
