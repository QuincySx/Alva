// INPUT: bounded JSON-RPC stdio, native/process, optional full-frame Observer
// OUTPUT: Client, Open, Connected, ListTools, CallTool, Resources, ReadResource, Prompts, GetPrompt, Close
// POS: [integration / external protocol] Continuous owned MCP I/O worker with cancellable serialized RPC and fail-closed logging
package mcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/system/process"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type rpcResponse struct {
	Result json.RawMessage
	Err    error
}
type Client struct {
	config          Config
	process         *process.Process
	observer        Observer
	ctx             context.Context
	cancel          context.CancelFunc
	done            chan struct{}
	once            sync.Once
	wg              sync.WaitGroup
	workersDone     chan struct{}
	mu              sync.Mutex
	writeGate       chan struct{}
	rpc             chan struct{}
	connected       bool
	hasTools        bool
	failure         error
	observerFailure error
	next            uint64
	pending         uint64
	response        chan rpcResponse
	notifications   atomic.Int32
}

func Open(ctx context.Context, config Config, observer Observer) (*Client, error) {
	if config.ID == "" || len(config.ID) > 256 || !utf8.ValidString(config.ID) || strings.ContainsAny(config.ID, "\x00\r\n") {
		return nil, errors.New("invalid MCP server identity")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if config.ConnectTimeout == 0 {
		config.ConnectTimeout = 30 * time.Second
	}
	if config.RequestTimeout == 0 {
		config.RequestTimeout = 60 * time.Second
	}
	if config.ConnectTimeout < time.Millisecond || config.ConnectTimeout > time.Minute || config.RequestTimeout < time.Millisecond || config.RequestTimeout > 10*time.Minute {
		return nil, errors.New("MCP timeout out of bounds")
	}
	p, err := process.Start(config.Process)
	if err != nil {
		return nil, err
	}
	worker, cancel := context.WithCancel(context.Background())
	c := &Client{config: config, process: p, observer: observer, ctx: worker, cancel: cancel, done: make(chan struct{}), workersDone: make(chan struct{}), rpc: make(chan struct{}, 1), writeGate: make(chan struct{}, 1)}
	c.wg.Add(2)
	go c.stdout()
	go c.stderr()
	go func() { c.wg.Wait(); close(c.workersDone) }()
	handshake, end := context.WithTimeout(ctx, config.ConnectTimeout)
	defer end()
	result, err := c.request(handshake, "initialize", map[string]any{"protocolVersion": versions[0], "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "alva", "version": "go-v1"}})
	if err == nil {
		var init struct {
			Version      string                     `json:"protocolVersion"`
			Capabilities map[string]json.RawMessage `json:"capabilities"`
			Server       struct {
				Name    string `json:"name"`
				Version string `json:"version"`
			} `json:"serverInfo"`
		}
		if json.Unmarshal(result, &init) != nil || init.Capabilities == nil || init.Server.Name == "" || init.Server.Version == "" {
			err = errors.New("invalid MCP initialization response")
		} else {
			valid := false
			for _, v := range versions {
				valid = valid || v == init.Version
			}
			if !valid {
				err = errors.New("unsupported MCP protocol version")
			} else if tools, ok := init.Capabilities["tools"]; ok {
				var value map[string]any
				if json.Unmarshal(tools, &value) != nil || value == nil {
					err = errors.New("invalid MCP tools capability")
				} else {
					c.hasTools = true
				}
			}
		}
	}
	if err == nil {
		err = c.send(handshake, map[string]any{"jsonrpc": "2.0", "method": "notifications/initialized"})
	}
	if err == nil {
		c.mu.Lock()
		select {
		case <-c.done:
			err = c.failure
		default:
			c.connected = true
		}
		c.mu.Unlock()
	}
	if err != nil {
		cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()
		return nil, errors.Join(err, c.Close(cleanup))
	}
	return c, nil
}
func (c *Client) Connected() bool { c.mu.Lock(); defer c.mu.Unlock(); return c.connected }
func (c *Client) fail(err error) {
	c.once.Do(func() {
		c.mu.Lock()
		c.failure = err
		c.connected = false
		c.mu.Unlock()
		close(c.done)
		c.cancel()
		c.process.BeginClose()
	})
}
func (c *Client) record(direction string, frame []byte) error {
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(c.ctx), 5*time.Second)
	defer cancel()
	return c.recordContext(cleanup, direction, frame)
}
func (c *Client) recordContext(ctx context.Context, direction string, frame []byte) error {
	if c.observer == nil {
		return nil
	}
	_, err := c.observer.Record(ctx, IOEvent{Server: c.config.ID, Direction: direction, Frame: append(json.RawMessage(nil), frame...), At: time.Now().UTC()})
	if err != nil {
		failure := &observerError{cause: err}
		c.mu.Lock()
		c.observerFailure = errors.Join(c.observerFailure, failure)
		c.mu.Unlock()
		return failure
	}
	return nil
}

type observerError struct{ cause error }

func (*observerError) Error() string   { return "MCP I/O observer failed; interaction stopped" }
func (e *observerError) Unwrap() error { return e.cause }
func (c *Client) send(ctx context.Context, value any) error {
	data, err := json.Marshal(value)
	if err != nil || len(data) > FrameLimit || !wire.ValidJSON(data) {
		return errors.New("outgoing MCP frame exceeds limit or is invalid")
	}
	select {
	case c.writeGate <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	case <-c.done:
		return errors.New("MCP transport closed")
	}
	defer func() { <-c.writeGate }()
	if err := ctx.Err(); err != nil {
		return err
	}
	select {
	case <-c.done:
		return errors.New("MCP transport closed")
	default:
	}
	// Closing the owned process on ctx cancellation interrupts a blocked pipe write.
	writeDone := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			select {
			case <-writeDone:
				return
			default:
				c.fail(ctx.Err())
			}
		case <-writeDone:
		}
	}()
	defer close(writeDone)
	if err = c.record("outgoing_attempt", data); err != nil {
		c.fail(err)
		return err
	}
	n, err := c.process.Stdin.Write(append(data, '\n'))
	if err == nil && n != len(data)+1 {
		err = io.ErrShortWrite
	}
	if err != nil {
		return errors.New("MCP stdin write failed")
	}
	return nil
}
func (c *Client) stdout() {
	defer c.wg.Done()
	reader := bufio.NewReaderSize(c.process.Stdout, 4096)
	for {
		data, readErr := readFrame(reader)
		if readErr != nil {
			if len(data) > 0 {
				prefix := map[string]any{"complete": false, "truncated": len(data) >= FrameLimit}
				if utf8.Valid(data) {
					prefix["text"] = string(data)
				} else {
					prefix["base64"] = base64.StdEncoding.EncodeToString(data)
				}
				safe, _ := json.Marshal(prefix)
				cleanup, stop := context.WithTimeout(context.Background(), time.Second)
				err := c.recordContext(cleanup, "incomplete_incoming", safe)
				stop()
				if err != nil {
					c.fail(err)
					return
				}
			}
			c.fail(errors.New("MCP stdout closed, incomplete or oversized frame"))
			return
		}
		if len(data) > FrameLimit || !utf8.Valid(data) {
			safe, _ := json.Marshal(map[string]any{"complete": true, "base64": base64.StdEncoding.EncodeToString(data), "invalid_utf8": !utf8.Valid(data)})
			if err := c.record("invalid_incoming", safe); err != nil {
				c.fail(err)
				return
			}
			c.fail(errors.New("invalid or oversized MCP frame"))
			return
		}
		if len(bytes.TrimSpace(data)) == 0 {
			c.fail(errors.New("empty MCP frame"))
			return
		}
		if !wire.ValidJSON(data) {
			safe, _ := json.Marshal(map[string]any{"text": string(data), "complete": true, "invalid_json": true})
			if err := c.record("invalid_incoming", safe); err != nil {
				c.fail(err)
				return
			}
			c.fail(errors.New("invalid MCP JSON frame"))
			return
		}
		if err := c.record("incoming", data); err != nil {
			c.fail(err)
			return
		}
		var envelope map[string]json.RawMessage
		if json.Unmarshal(data, &envelope) != nil || string(envelope["jsonrpc"]) != `"2.0"` {
			c.fail(errors.New("invalid MCP envelope"))
			return
		}
		if rawMethod, exists := envelope["method"]; exists {
			var method string
			if json.Unmarshal(rawMethod, &method) != nil || method == "" {
				c.fail(errors.New("invalid MCP method"))
				return
			}
			c.mu.Lock()
			pending := c.pending
			c.mu.Unlock()
			if pending != 0 && c.notifications.Add(1) > 128 {
				c.fail(errors.New("MCP intermediate message limit exceeded"))
				return
			}
			if id, ok := envelope["id"]; ok {
				reply := map[string]any{"jsonrpc": "2.0", "id": id}
				if method == "ping" {
					reply["result"] = map[string]any{}
				} else {
					reply["error"] = map[string]any{"code": -32601, "message": "Unsupported client method"}
				}
				deadline, cancel := context.WithTimeout(c.ctx, 5*time.Second)
				err := c.send(deadline, reply)
				cancel()
				if err != nil {
					c.fail(err)
					return
				}
			}
			continue
		}
		var id uint64
		if json.Unmarshal(envelope["id"], &id) != nil || id == 0 {
			c.fail(errors.New("invalid MCP response identity"))
			return
		}
		c.mu.Lock()
		pending, reply := c.pending, c.response
		c.mu.Unlock()
		if pending != id || reply == nil {
			c.fail(errors.New("unexpected MCP response identity"))
			return
		}
		result, hasResult := envelope["result"]
		remoteError, hasError := envelope["error"]
		if hasResult == hasError {
			c.fail(errors.New("MCP response requires exactly one result or error"))
			return
		}
		response := rpcResponse{Result: append(json.RawMessage(nil), result...)}
		if hasError {
			var e map[string]json.RawMessage
			if json.Unmarshal(remoteError, &e) != nil || e == nil {
				c.fail(errors.New("invalid MCP error envelope"))
				return
			}
			response.Err = errors.New("MCP server rejected the request")
		} else {
			var object map[string]json.RawMessage
			if json.Unmarshal(result, &object) != nil || object == nil {
				c.fail(errors.New("MCP result must be an object"))
				return
			}
		}
		select {
		case reply <- response:
		default:
			c.fail(errors.New("duplicate MCP response"))
			return
		}
	}
}
func readFrame(reader *bufio.Reader) ([]byte, error) {
	var frame []byte
	for {
		chunk, err := reader.ReadSlice('\n')
		if len(frame)+len(chunk) > FrameLimit+1 {
			remaining := FrameLimit - len(frame)
			if remaining > 0 {
				frame = append(frame, chunk[:remaining]...)
			}
			return frame, errors.New("MCP frame limit exceeded")
		}
		frame = append(frame, chunk...)
		if err == nil {
			return bytes.TrimSuffix(frame, []byte{'\n'}), nil
		}
		if errors.Is(err, bufio.ErrBufferFull) {
			continue
		}
		return frame, err
	}
}
func (c *Client) stderr() {
	defer c.wg.Done()
	err := process.ReadLines(c.process.Stderr, 1<<20, func(line process.StreamLine) error {
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
		c.fail(errors.Join(errors.New("MCP stderr framing/observer failed"), err))
	}
}
func (c *Client) request(ctx context.Context, method string, params any) (json.RawMessage, error) {
	select {
	case c.rpc <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-c.done:
		return nil, errors.New("MCP transport closed")
	}
	defer func() { <-c.rpc }()
	deadline, cancel := context.WithTimeout(ctx, c.config.RequestTimeout)
	defer cancel()
	c.mu.Lock()
	c.next++
	id := c.next
	c.pending = id
	c.response = make(chan rpcResponse, 1)
	reply := c.response
	c.notifications.Store(0)
	c.mu.Unlock()
	defer func() { c.mu.Lock(); c.pending = 0; c.response = nil; c.mu.Unlock() }()
	if err := c.send(deadline, map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params}); err != nil {
		c.fail(err)
		return nil, errors.Join(err, deadline.Err())
	}
	select {
	case result := <-reply:
		if result.Err != nil {
			c.fail(result.Err)
		}
		return result.Result, errors.Join(result.Err, deadline.Err())
	case <-deadline.Done():
		c.fail(deadline.Err())
		return nil, deadline.Err()
	case <-c.done:
		c.mu.Lock()
		err := c.failure
		c.mu.Unlock()
		return nil, errors.Join(err, deadline.Err())
	}
}
func (c *Client) ListTools(ctx context.Context) ([]Tool, error) {
	if !c.Connected() {
		return nil, errors.New("MCP is not connected")
	}
	if !c.hasTools {
		return []Tool{}, nil
	}
	var out []Tool
	cursor := ""
	seenCursor := map[string]bool{}
	names := map[string]bool{}
	total := 0
	for page := 0; page < PageLimit; page++ {
		params := map[string]string{}
		if cursor != "" {
			params["cursor"] = cursor
		}
		data, err := c.request(ctx, "tools/list", params)
		if err != nil {
			return nil, err
		}
		total += len(data)
		if total > CatalogLimit {
			c.fail(errors.New("MCP catalog exceeds 32 MiB"))
			return nil, errors.New("MCP catalog exceeds 32 MiB")
		}
		var result struct {
			Tools []Tool `json:"tools"`
			Next  string `json:"nextCursor"`
		}
		if err = json.Unmarshal(data, &result); err != nil || result.Tools == nil {
			return nil, errors.New("invalid MCP tool directory")
		}
		for _, tool := range result.Tools {
			var schema map[string]any
			if tool.Name == "" || len(tool.Name) > 256 || !utf8.ValidString(tool.Name) || len(tool.Description) > 16<<10 || !utf8.ValidString(tool.Description) || names[tool.Name] || len(tool.InputSchema) > SchemaLimit || json.Unmarshal(tool.InputSchema, &schema) != nil || schema == nil {
				return nil, errors.New("invalid MCP tool metadata")
			}
			names[tool.Name] = true
			out = append(out, tool)
			if len(out) > ToolLimit {
				return nil, errors.New("MCP catalog tool limit exceeded")
			}
		}
		if result.Next == "" {
			return out, nil
		}
		if len(result.Next) > 4096 || seenCursor[result.Next] {
			return nil, errors.New("invalid or repeated MCP cursor")
		}
		cursor = result.Next
		seenCursor[cursor] = true
	}
	return nil, errors.New("MCP catalog page limit exceeded")
}
func (c *Client) CallTool(ctx context.Context, name string, arguments json.RawMessage) (json.RawMessage, error) {
	if !c.Connected() {
		return nil, errors.New("MCP is not connected")
	}
	var object map[string]json.RawMessage
	if name == "" || len(name) > 256 || len(arguments) > FrameLimit/2 || json.Unmarshal(arguments, &object) != nil || object == nil {
		return nil, errors.New("MCP tool requires bounded name and object arguments")
	}
	return c.request(ctx, "tools/call", map[string]any{"name": name, "arguments": arguments})
}
func (c *Client) Resources(ctx context.Context, cursor string) (json.RawMessage, error) {
	if !c.Connected() || len(cursor) > 4096 {
		return nil, errors.New("invalid MCP resource request")
	}
	return c.request(ctx, "resources/list", map[string]string{"cursor": cursor})
}
func (c *Client) ReadResource(ctx context.Context, uri string) (json.RawMessage, error) {
	if !c.Connected() || uri == "" || len(uri) > 4096 {
		return nil, errors.New("invalid MCP resource URI")
	}
	return c.request(ctx, "resources/read", map[string]string{"uri": uri})
}
func (c *Client) Prompts(ctx context.Context, cursor string) (json.RawMessage, error) {
	if !c.Connected() || len(cursor) > 4096 {
		return nil, errors.New("invalid MCP prompt request")
	}
	return c.request(ctx, "prompts/list", map[string]string{"cursor": cursor})
}
func (c *Client) GetPrompt(ctx context.Context, name string, arguments map[string]string) (json.RawMessage, error) {
	if !c.Connected() || name == "" || len(name) > 256 || len(arguments) > 128 {
		return nil, errors.New("invalid MCP prompt request")
	}
	return c.request(ctx, "prompts/get", map[string]any{"name": name, "arguments": arguments})
}
func (c *Client) Close(ctx context.Context) error {
	c.fail(errors.New("MCP transport closed by owner"))
	err := c.process.Close(ctx)
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
func (c *Client) String() string { return fmt.Sprintf("MCP(%s)", c.config.ID) }
func (c *Client) CleanupComplete() bool {
	select {
	case <-c.workersDone:
		return c.process.CleanupComplete()
	default:
		return false
	}
}
