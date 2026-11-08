// INPUT: net/http, one explicit Streamable HTTP endpoint, optional Authorizer and full-frame Observer
// OUTPUT: HTTPConfig, Authorizer, ErrAuthorizationRequired, ErrLegacyTransport, Connect, HTTPClient
// POS: [integration / external protocol] Serialized JSON-RPC over POST with JSON or bounded SSE replies; no replay, no redirects, no ambient proxy
package mcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// HTTPConfig is one remote endpoint. Header values are secrets of the caller;
// the transport never records them, only JSON-RPC frames.
type HTTPConfig struct {
	URL     string
	Headers map[string]string
	Auth    Authorizer
	// Client overrides the default no-proxy, no-redirect client (tests only).
	Client *http.Client
}

// Authorizer supplies a bearer credential. Unauthorized is called at most once
// per request after a 401/403; returning nil retries that request once. A
// runtime implementation may refresh but must never start interactive login.
type Authorizer interface {
	Header(context.Context) (string, error)
	Unauthorized(context.Context, int, []string) error
}

var (
	ErrAuthorizationRequired = errors.New("MCP server requires authorization")
	ErrLegacyTransport       = errors.New("MCP server does not accept Streamable HTTP; the deprecated HTTP+SSE transport is not supported")
	errSessionExpired        = errors.New("MCP session expired; no automatic replay")
)

// AuthorizationError carries the server challenge for discovery.
type AuthorizationError struct {
	Status    int
	Challenge []string
}

func (e *AuthorizationError) Error() string {
	return fmt.Sprintf("%v (HTTP %d)", ErrAuthorizationRequired, e.Status)
}
func (e *AuthorizationError) Unwrap() error { return ErrAuthorizationRequired }

// Connect opens the transport the configuration names.
// A failed open returns a nil interface, never a typed nil client.
func Connect(ctx context.Context, config Config, observer Observer) (Transport, error) {
	if config.HTTP != nil {
		client, err := OpenHTTP(ctx, config, observer)
		if err != nil {
			return nil, err
		}
		return client, nil
	}
	client, err := Open(ctx, config, observer)
	if err != nil {
		return nil, err
	}
	return client, nil
}

type HTTPClient struct {
	config    Config
	endpoint  string
	headers   map[string]string
	auth      Authorizer
	client    *http.Client
	observer  Observer
	rpc       chan struct{}
	mu        sync.Mutex
	session   string
	version   string
	connected bool
	hasTools  bool
	closed    bool
	failure   error
	next      uint64
	inflight  sync.WaitGroup
	active    atomic.Int32
	ctx       context.Context
	cancel    context.CancelFunc
}

// DefaultHTTPClient follows no redirects (a bearer must not reach another
// origin) and ignores environment proxies (no ambient configuration).
func DefaultHTTPClient() *http.Client {
	transport := &http.Transport{Proxy: nil, ForceAttemptHTTP2: true, TLSHandshakeTimeout: 15 * time.Second, IdleConnTimeout: 30 * time.Second, MaxIdleConnsPerHost: 2}
	return &http.Client{Transport: transport, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
}

// ValidEndpoint checks a Streamable HTTP endpoint address.
func ValidEndpoint(raw string) error {
	address, err := url.Parse(raw)
	if err != nil || len(raw) > 2048 || !utf8.ValidString(raw) || strings.ContainsAny(raw, "\x00\r\n\t ") || address.Host == "" || address.User != nil || address.Fragment != "" || (address.Scheme != "https" && address.Scheme != "http") {
		return errors.New("invalid MCP HTTP endpoint")
	}
	return nil
}

func OpenHTTP(ctx context.Context, config Config, observer Observer) (*HTTPClient, error) {
	if config.ID == "" || len(config.ID) > 256 || !utf8.ValidString(config.ID) || strings.ContainsAny(config.ID, "\x00\r\n") || config.HTTP == nil {
		return nil, errors.New("invalid MCP server identity")
	}
	if err := ValidEndpoint(config.HTTP.URL); err != nil {
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
	headers := make(map[string]string, len(config.HTTP.Headers))
	for name, value := range config.HTTP.Headers {
		if strings.ContainsAny(name, " :\r\n\x00") || strings.ContainsAny(value, "\r\n\x00") {
			return nil, errors.New("invalid explicit MCP header")
		}
		headers[http.CanonicalHeaderKey(name)] = value
	}
	client := config.HTTP.Client
	if client == nil {
		client = DefaultHTTPClient()
	}
	worker, cancel := context.WithCancel(context.Background())
	c := &HTTPClient{config: config, endpoint: config.HTTP.URL, headers: headers, auth: config.HTTP.Auth, client: client, observer: observer, rpc: make(chan struct{}, 1), ctx: worker, cancel: cancel}
	handshake, end := context.WithTimeout(ctx, config.ConnectTimeout)
	defer end()
	result, err := c.request(handshake, "initialize", map[string]any{"protocolVersion": versions[0], "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "alva", "version": "go-v1"}})
	if err == nil {
		var version string
		version, c.hasTools, err = parseInitialize(result)
		c.mu.Lock()
		c.version = version
		c.mu.Unlock()
	}
	if err == nil {
		err = c.notify(handshake, "notifications/initialized")
	}
	if err != nil {
		cleanup, stop := context.WithTimeout(context.Background(), 5*time.Second)
		defer stop()
		return nil, errors.Join(err, c.Close(cleanup))
	}
	c.mu.Lock()
	c.connected = true
	c.mu.Unlock()
	return c, nil
}

// parseInitialize validates the handshake and returns the negotiated version.
func parseInitialize(result json.RawMessage) (string, bool, error) {
	var init struct {
		Version      string                     `json:"protocolVersion"`
		Capabilities map[string]json.RawMessage `json:"capabilities"`
		Server       struct {
			Name    string `json:"name"`
			Version string `json:"version"`
		} `json:"serverInfo"`
	}
	if json.Unmarshal(result, &init) != nil || init.Capabilities == nil || init.Server.Name == "" || init.Server.Version == "" {
		return "", false, errors.New("invalid MCP initialization response")
	}
	valid := false
	for _, v := range versions {
		valid = valid || v == init.Version
	}
	if !valid {
		return "", false, errors.New("unsupported MCP protocol version")
	}
	tools, ok := init.Capabilities["tools"]
	if !ok {
		return init.Version, false, nil
	}
	var value map[string]any
	if json.Unmarshal(tools, &value) != nil || value == nil {
		return "", false, errors.New("invalid MCP tools capability")
	}
	return init.Version, true, nil
}

func (c *HTTPClient) Connected() bool { c.mu.Lock(); defer c.mu.Unlock(); return c.connected }

func (c *HTTPClient) fail(err error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.failure == nil {
		c.failure = err
	}
	c.connected = false
}

func (c *HTTPClient) record(ctx context.Context, direction string, frame []byte) error {
	if c.observer == nil {
		return nil
	}
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	if _, err := c.observer.Record(cleanup, IOEvent{Server: c.config.ID, Direction: direction, Frame: append(json.RawMessage(nil), frame...), At: time.Now().UTC()}); err != nil {
		return &observerError{cause: err}
	}
	return nil
}

func (c *HTTPClient) usable() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return errors.New("MCP transport closed")
	}
	if c.failure != nil {
		return c.failure
	}
	return nil
}

func (c *HTTPClient) request(ctx context.Context, method string, params any) (json.RawMessage, error) {
	select {
	case c.rpc <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	defer func() { <-c.rpc }()
	if err := c.usable(); err != nil {
		return nil, err
	}
	deadline, cancel := context.WithTimeout(ctx, c.config.RequestTimeout)
	defer cancel()
	stop := context.AfterFunc(c.ctx, cancel)
	defer stop()
	c.mu.Lock()
	c.next++
	id := c.next
	c.mu.Unlock()
	body, err := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": id, "method": method, "params": params})
	if err != nil || len(body) > FrameLimit || !wire.ValidJSON(body) {
		return nil, errors.New("outgoing MCP frame exceeds limit or is invalid")
	}
	result, err := c.exchange(deadline, body, id, method == "initialize")
	if err != nil {
		c.fail(err)
		return nil, errors.Join(err, deadline.Err())
	}
	return result, nil
}

func (c *HTTPClient) notify(ctx context.Context, method string) error {
	body, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "method": method})
	_, err := c.exchange(ctx, body, 0, false)
	return err
}

// exchange POSTs one message. id 0 is a notification or response that must be
// answered with 202. Otherwise the reply is one JSON response or an SSE stream
// ending with it; server requests on the stream are answered in between.
func (c *HTTPClient) exchange(ctx context.Context, body []byte, id uint64, initialize bool) (json.RawMessage, error) {
	c.inflight.Add(1)
	c.active.Add(1)
	defer func() { c.active.Add(-1); c.inflight.Done() }()
	if err := c.record(ctx, "outgoing_attempt", body); err != nil {
		return nil, err
	}
	response, err := c.post(ctx, body, true)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	defer io.Copy(io.Discard, io.LimitReader(response.Body, 64<<10))
	switch {
	case response.StatusCode == http.StatusNotFound && !initialize && c.sessionID() != "":
		return nil, errSessionExpired
	case initialize && (response.StatusCode == http.StatusNotFound || response.StatusCode == http.StatusMethodNotAllowed || response.StatusCode == http.StatusBadRequest):
		return nil, ErrLegacyTransport
	case response.StatusCode >= 300 && response.StatusCode < 400:
		return nil, fmt.Errorf("MCP server redirected (HTTP %d); redirects are not followed", response.StatusCode)
	case response.StatusCode < 200 || response.StatusCode > 299:
		return nil, fmt.Errorf("MCP server returned HTTP %d", response.StatusCode)
	}
	if initialize {
		if session := response.Header.Get("Mcp-Session-Id"); session != "" {
			if len(session) > 512 || strings.IndexFunc(session, func(r rune) bool { return r < 0x21 || r > 0x7e }) >= 0 {
				return nil, errors.New("invalid MCP session identity")
			}
			c.mu.Lock()
			c.session = session
			c.mu.Unlock()
		}
	}
	if id == 0 {
		if response.StatusCode != http.StatusAccepted && response.StatusCode != http.StatusOK {
			return nil, fmt.Errorf("MCP notification answered with HTTP %d", response.StatusCode)
		}
		return nil, nil
	}
	if response.StatusCode == http.StatusAccepted {
		return nil, errors.New("MCP request answered without a response")
	}
	mediaType := strings.ToLower(strings.TrimSpace(strings.Split(response.Header.Get("Content-Type"), ";")[0]))
	switch mediaType {
	case "application/json":
		data, err := io.ReadAll(io.LimitReader(response.Body, FrameLimit+1))
		if err != nil {
			return nil, errors.New("MCP response body failed")
		}
		if len(data) > FrameLimit {
			return nil, errors.New("invalid or oversized MCP frame")
		}
		result, done, err := c.message(ctx, data, id)
		if err == nil && !done {
			err = errors.New("MCP response did not answer the request")
		}
		return result, err
	case "text/event-stream":
		return c.stream(ctx, response.Body, id)
	}
	return nil, errors.New("MCP response has an unsupported content type")
}

func (c *HTTPClient) sessionID() string { c.mu.Lock(); defer c.mu.Unlock(); return c.session }

func (c *HTTPClient) post(ctx context.Context, body []byte, retry bool) (*http.Response, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodPost, c.endpoint, bytes.NewReader(body))
	if err != nil {
		return nil, err
	}
	for name, value := range c.headers {
		request.Header.Set(name, value)
	}
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("Accept", "application/json, text/event-stream")
	c.mu.Lock()
	session, version := c.session, c.version
	c.mu.Unlock()
	if session != "" {
		request.Header.Set("Mcp-Session-Id", session)
	}
	if version != "" {
		request.Header.Set("Mcp-Protocol-Version", version)
	}
	if c.auth != nil {
		header, err := c.auth.Header(ctx)
		if err != nil {
			return nil, err
		}
		if header != "" {
			request.Header.Set("Authorization", header)
		}
	}
	response, err := c.client.Do(request)
	if err != nil {
		return nil, errors.New("MCP HTTP request failed: " + sanitizeTransportError(err))
	}
	if response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden {
		challenge := append([]string(nil), response.Header.Values("WWW-Authenticate")...)
		io.Copy(io.Discard, io.LimitReader(response.Body, 64<<10))
		response.Body.Close()
		if c.auth == nil || !retry {
			return nil, &AuthorizationError{Status: response.StatusCode, Challenge: challenge}
		}
		if err := c.auth.Unauthorized(ctx, response.StatusCode, challenge); err != nil {
			return nil, errors.Join(&AuthorizationError{Status: response.StatusCode, Challenge: challenge}, err)
		}
		return c.post(ctx, body, false)
	}
	return response, nil
}

// sanitizeTransportError drops URLs from net/http errors; query strings may
// carry credentials the user embedded in an address.
func sanitizeTransportError(err error) string {
	var address *url.Error
	if errors.As(err, &address) {
		err = address.Err
	}
	if errors.Is(err, context.DeadlineExceeded) || errors.Is(err, context.Canceled) {
		return err.Error()
	}
	message := err.Error()
	if len(message) > 200 {
		message = message[:200]
	}
	return message
}

// message handles one incoming JSON-RPC message. done reports that it was
// the response to request id.
func (c *HTTPClient) message(ctx context.Context, data []byte, id uint64) (json.RawMessage, bool, error) {
	if len(bytes.TrimSpace(data)) == 0 || !utf8.Valid(data) || !wire.ValidJSON(data) {
		return nil, false, errors.New("invalid MCP JSON frame")
	}
	if err := c.record(ctx, "incoming", data); err != nil {
		return nil, false, err
	}
	var envelope map[string]json.RawMessage
	if json.Unmarshal(data, &envelope) != nil || string(envelope["jsonrpc"]) != `"2.0"` {
		return nil, false, errors.New("invalid MCP envelope")
	}
	if rawMethod, exists := envelope["method"]; exists {
		var method string
		if json.Unmarshal(rawMethod, &method) != nil || method == "" {
			return nil, false, errors.New("invalid MCP method")
		}
		if requestID, ok := envelope["id"]; ok {
			reply := map[string]any{"jsonrpc": "2.0", "id": requestID}
			if method == "ping" {
				reply["result"] = map[string]any{}
			} else {
				reply["error"] = map[string]any{"code": -32601, "message": "Unsupported client method"}
			}
			body, _ := json.Marshal(reply)
			if _, err := c.exchange(ctx, body, 0, false); err != nil {
				return nil, false, err
			}
		}
		return nil, false, nil
	}
	var responseID uint64
	if json.Unmarshal(envelope["id"], &responseID) != nil || responseID != id {
		return nil, false, errors.New("unexpected MCP response identity")
	}
	result, hasResult := envelope["result"]
	remoteError, hasError := envelope["error"]
	if hasResult == hasError {
		return nil, false, errors.New("MCP response requires exactly one result or error")
	}
	if hasError {
		var e map[string]json.RawMessage
		if json.Unmarshal(remoteError, &e) != nil || e == nil {
			return nil, false, errors.New("invalid MCP error envelope")
		}
		return nil, true, errors.New("MCP server rejected the request")
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(result, &object) != nil || object == nil {
		return nil, false, errors.New("MCP result must be an object")
	}
	return append(json.RawMessage(nil), result...), true, nil
}

// stream reads SSE events until the response arrives. Lines and events are
// bounded; a stream that ends first fails without replaying the request.
func (c *HTTPClient) stream(ctx context.Context, body io.Reader, id uint64) (json.RawMessage, error) {
	reader := bufio.NewReaderSize(body, 4096)
	var data []byte
	total, intermediate := 0, 0
	for {
		line, err := readFrame(reader)
		if err != nil {
			if len(line) >= FrameLimit {
				return nil, errors.New("MCP event line exceeds limit")
			}
			return nil, errors.New("MCP event stream ended before the response; not replayed")
		}
		total += len(line) + 1
		if total > CatalogLimit {
			return nil, errors.New("MCP event stream exceeds 32 MiB")
		}
		line = bytes.TrimSuffix(line, []byte{'\r'})
		if len(line) == 0 {
			if len(data) == 0 {
				continue
			}
			event := data
			data = nil
			result, done, err := c.message(ctx, event, id)
			if err != nil || done {
				return result, err
			}
			if intermediate++; intermediate > 128 {
				return nil, errors.New("MCP intermediate message limit exceeded")
			}
			continue
		}
		if field, value, ok := bytes.Cut(line, []byte{':'}); ok && string(field) == "data" {
			value = bytes.TrimPrefix(value, []byte{' '})
			if len(data)+len(value)+1 > FrameLimit {
				return nil, errors.New("invalid or oversized MCP frame")
			}
			if len(data) > 0 {
				data = append(data, '\n')
			}
			data = append(data, value...)
		}
	}
}

func (c *HTTPClient) ListTools(ctx context.Context) ([]Tool, error) {
	if !c.Connected() {
		return nil, errors.New("MCP is not connected")
	}
	if !c.hasTools {
		return []Tool{}, nil
	}
	return listTools(ctx, c.request, c.fail)
}

func (c *HTTPClient) CallTool(ctx context.Context, name string, arguments json.RawMessage) (json.RawMessage, error) {
	if !c.Connected() {
		return nil, errors.New("MCP is not connected")
	}
	var object map[string]json.RawMessage
	if name == "" || len(name) > 256 || len(arguments) > FrameLimit/2 || json.Unmarshal(arguments, &object) != nil || object == nil {
		return nil, errors.New("MCP tool requires bounded name and object arguments")
	}
	return c.request(ctx, "tools/call", map[string]any{"name": name, "arguments": arguments})
}

// Close cancels in-flight requests, waits for them and ends the server session.
func (c *HTTPClient) Close(ctx context.Context) error {
	c.mu.Lock()
	already := c.closed
	c.closed, c.connected = true, false
	session := c.session
	c.mu.Unlock()
	c.cancel()
	waited := make(chan struct{})
	go func() { c.inflight.Wait(); close(waited) }()
	select {
	case <-waited:
	case <-ctx.Done():
		return ctx.Err()
	}
	if already || session == "" {
		return nil
	}
	end, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	request, err := http.NewRequestWithContext(end, http.MethodDelete, c.endpoint, nil)
	if err != nil {
		return nil
	}
	for name, value := range c.headers {
		request.Header.Set(name, value)
	}
	request.Header.Set("Mcp-Session-Id", session)
	if c.version != "" {
		request.Header.Set("Mcp-Protocol-Version", c.version)
	}
	if c.auth != nil {
		if header, err := c.auth.Header(end); err == nil && header != "" {
			request.Header.Set("Authorization", header)
		}
	}
	// Ending the session is best effort; servers may answer 405.
	if response, err := c.client.Do(request); err == nil {
		io.Copy(io.Discard, io.LimitReader(response.Body, 64<<10))
		response.Body.Close()
	}
	return nil
}

// CleanupComplete reports that no request is still in flight after Close.
func (c *HTTPClient) CleanupComplete() bool {
	c.mu.Lock()
	closed := c.closed
	c.mu.Unlock()
	return closed && c.active.Load() == 0
}

func (c *HTTPClient) String() string { return fmt.Sprintf("MCP(%s)", c.config.ID) }
