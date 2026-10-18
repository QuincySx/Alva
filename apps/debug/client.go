// INPUT: Explicit authenticated diagnostic URL, exact bounded tool arguments and owned MCP stdio
// OUTPUT: Client read/capture/action/control and official-SDK UI diagnostic MCP serving
// POS: [L6 diagnostics] Read-only discovery stays cold; UI controls require both client opt-in and the actual server owner policy.
package debug

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"strconv"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/loopback"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

type ClientConfig struct {
	Address       string
	Token         string `json:"-"`
	AllowControls bool
}
type Client struct {
	address, token string
	controls       bool
	http           *http.Client
	gate           chan struct{}
}
type inspectArguments struct {
	Action    string `json:"action"`
	ViewID    string `json:"view_id,omitempty"`
	After     uint64 `json:"after,omitempty"`
	HighWater uint64 `json:"high_water,omitempty"`
	Level     string `json:"level,omitempty"`
	Query     string `json:"query,omitempty"`
}

func NewClient(config ClientConfig) (*Client, error) {
	u, err := url.Parse(config.Address)
	if err != nil || u.Scheme != "http" || u.Host == "" || u.Path != "" || u.RawQuery != "" || u.Fragment != "" || u.User != nil || u.String() != config.Address {
		return nil, errors.New("explicit canonical numeric-loopback diagnostic URL required")
	}
	if _, err := loopback.New(u.Host, config.Token); err != nil {
		return nil, err
	}
	transport := &http.Transport{Proxy: nil, MaxIdleConns: 4, MaxIdleConnsPerHost: 4, IdleConnTimeout: 15 * time.Second, ResponseHeaderTimeout: 5 * time.Second}
	return &Client{address: config.Address, token: config.Token, controls: config.AllowControls, http: &http.Client{Transport: transport, Timeout: 6 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}, gate: make(chan struct{}, 4)}, nil
}
func (c *Client) Close() { c.http.CloseIdleConnections() }
func (c *Client) request(ctx context.Context, method, path string, body []byte, maximum int) ([]byte, error) {
	select {
	case c.gate <- struct{}{}:
		defer func() { <-c.gate }()
	default:
		return nil, ErrBusy
	}
	request, err := http.NewRequestWithContext(ctx, method, c.address+path, bytes.NewReader(body))
	if err != nil {
		return nil, errors.New("diagnostic request invalid")
	}
	request.Header.Set("Authorization", "Bearer "+c.token)
	if body != nil {
		request.Header.Set("Content-Type", "application/json")
	}
	response, err := c.http.Do(request)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("diagnostic request failed; no operation was replayed")
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		body, _ := io.ReadAll(io.LimitReader(response.Body, 1025))
		var rejection struct {
			Error string `json:"error"`
		}
		if len(body) <= 1024 && exactJSON(body, &rejection) == nil {
			for _, known := range []error{ErrFramePending, ErrBusy, ErrUnavailable, ErrStale} {
				if rejection.Error == known.Error() {
					return nil, known
				}
			}
		}
		return nil, errors.New("diagnostic server rejected the request")
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, int64(maximum)+1))
	if err != nil || len(raw) > maximum {
		return nil, errors.New("diagnostic response exceeds budget or was interrupted")
	}
	return raw, nil
}
func exactJSON(raw []byte, out any) error {
	if len(raw) > 32<<10 {
		return errors.New("diagnostic arguments exceed 32 KiB")
	}
	if err := strictjson.ValidateObject(raw, out); err != nil {
		return errors.New("exact bounded diagnostic object required")
	}
	return json.Unmarshal(raw, out)
}

func (c *Client) Inspect(ctx context.Context, raw []byte) (json.RawMessage, error) {
	var args inspectArguments
	if err := exactJSON(raw, &args); err != nil {
		return nil, err
	}
	if len(args.Query) > 256 || !utf8.ValidString(args.Query) {
		return nil, errors.New("bounded literal log query required")
	}
	query := url.Values{}
	path := ""
	switch args.Action {
	case "health", "views", "level":
		if args.ViewID != "" || args.After != 0 || args.HighWater != 0 || args.Level != "" || args.Query != "" {
			return nil, errors.New("unexpected diagnostic query fields")
		}
		path = "/v1/debug/" + args.Action
	case "state", "tree":
		if !identifier.MatchString(args.ViewID) || args.After != 0 || args.HighWater != 0 || args.Level != "" || args.Query != "" {
			return nil, errors.New("exact registered view ID required")
		}
		query.Set("view", args.ViewID)
		path = "/v1/debug/" + args.Action
	case "logs":
		if args.ViewID != "" {
			return nil, errors.New("log query cannot select a view")
		}
		query.Set("after", strconv.FormatUint(args.After, 10))
		query.Set("high_water", strconv.FormatUint(args.HighWater, 10))
		query.Set("limit", "8")
		if args.Level != "" {
			query.Set("level", args.Level)
		}
		if args.Query != "" {
			query.Set("q", args.Query)
		}
		path = "/v1/debug/logs"
	default:
		return nil, errors.New("unknown diagnostic read action")
	}
	if len(query) > 0 {
		path += "?" + query.Encode()
	}
	result, err := c.request(ctx, http.MethodGet, path, nil, 128<<10)
	if err != nil {
		return nil, err
	}
	if err := strictjson.Validate(result); err != nil {
		return nil, errors.New("diagnostic server returned invalid JSON")
	}
	return result, nil
}
func (c *Client) Capture(ctx context.Context, raw []byte) ([]byte, error) {
	var args struct {
		ViewID string `json:"view_id"`
	}
	if err := exactJSON(raw, &args); err != nil {
		return nil, err
	}
	if !identifier.MatchString(args.ViewID) {
		return nil, errors.New("exact registered view required")
	}
	data, _ := json.Marshal(args)
	image, err := c.request(ctx, http.MethodPost, "/v1/debug/capture", data, 4<<20)
	if err != nil {
		return nil, err
	}
	if err := validateCapture(image); err != nil {
		return nil, err
	}
	return image, nil
}
func (c *Client) Action(ctx context.Context, raw []byte) (ActionResult, error) {
	if !c.controls {
		return ActionResult{}, errors.New("UI controls were not enabled for this MCP process")
	}
	var action Action
	if err := exactJSON(raw, &action); err != nil {
		return ActionResult{}, err
	}
	if err := action.validate(); err != nil {
		return ActionResult{}, err
	}
	data, _ := json.Marshal(action)
	result, err := c.request(ctx, http.MethodPost, "/v1/debug/actions", data, 32<<10)
	if err != nil {
		return ActionResult{}, err
	}
	var outcome ActionResult
	if err := exactJSON(result, &outcome); err != nil {
		return ActionResult{}, err
	}
	if outcome.OperationID != action.OperationID || (outcome.Status != "applied" && outcome.Status != "rejected" && outcome.Status != "cancelled" && outcome.Status != "pending") {
		return ActionResult{}, errors.New("invalid diagnostic operation outcome")
	}
	return outcome, nil
}
func (c *Client) Control(ctx context.Context, raw []byte) (json.RawMessage, error) {
	if !c.controls {
		return nil, errors.New("diagnostic controls disabled")
	}
	var args struct {
		Action string `json:"action"`
		Level  string `json:"level,omitempty"`
	}
	if err := exactJSON(raw, &args); err != nil {
		return nil, err
	}
	method, path, body := http.MethodPut, "/v1/debug/level", []byte(nil)
	if args.Action == "level" {
		body, _ = json.Marshal(map[string]string{"level": args.Level})
	} else if args.Action == "shutdown" && args.Level == "" {
		method, path, body = http.MethodPost, "/v1/debug/shutdown", []byte(`{}`)
	} else {
		return nil, errors.New("unknown diagnostic control")
	}
	result, err := c.request(ctx, method, path, body, 32<<10)
	if err != nil {
		return nil, err
	}
	if err := strictjson.Validate(result); err != nil {
		return nil, errors.New("invalid diagnostic control response")
	}
	return result, nil
}

func (c *Client) Serve(ctx context.Context, input io.ReadCloser, output io.WriteCloser) error {
	if input == nil || output == nil {
		return errors.New("owned diagnostic MCP stdio required")
	}
	server := mcp.NewServer(&mcp.Implementation{Name: "alva-ui-devtools", Version: "1"}, &mcp.ServerOptions{Logger: slog.New(slog.NewTextHandler(io.Discard, nil)), Instructions: "Inspect actual registered native views and revision before actions. This is a product semantic tree, not raw framework internals. Capture only registered own windows. A pending action must be queried with the exact same operation_id/request; never allocate another ID to retry. UI actions use normal product approval and billing; transport shutdown does not Stop Agent execution."})
	closedWorld := false
	textResult := func(data []byte, err error) *mcp.CallToolResult {
		if err != nil {
			return &mcp.CallToolResult{IsError: true, Content: []mcp.Content{&mcp.TextContent{Text: safeError(err)}}}
		}
		return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: string(data)}}}
	}
	inspectSchema := json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"enum":["health","views","state","tree","level","logs"]},"view_id":{"type":"string"},"after":{"type":"integer","minimum":0},"high_water":{"type":"integer","minimum":0},"level":{"type":"string"},"query":{"type":"string","maxLength":256}}}`)
	server.AddTool(&mcp.Tool{Name: "ui_inspect", Description: "Read bounded actual registered views, state/revision, product semantic tree, diagnostic logs and level. Construction/listing never submits model work.", InputSchema: inspectSchema, Annotations: &mcp.ToolAnnotations{ReadOnlyHint: true, IdempotentHint: true, OpenWorldHint: &closedWorld}}, func(ctx context.Context, r *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
		data, err := c.Inspect(ctx, r.Params.Arguments)
		return textResult(data, err), nil
	})
	server.AddTool(&mcp.Tool{Name: "ui_capture", Description: "Explicit PNG capture of the exact registered application window. No full-screen capture or host path is accepted; sensitive credential panes may refuse capture.", InputSchema: json.RawMessage(`{"type":"object","additionalProperties":false,"required":["view_id"],"properties":{"view_id":{"type":"string"}}}`), Annotations: &mcp.ToolAnnotations{ReadOnlyHint: true, IdempotentHint: true, OpenWorldHint: &closedWorld}}, func(ctx context.Context, r *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
		data, err := c.Capture(ctx, r.Params.Arguments)
		if err != nil {
			return textResult(nil, err), nil
		}
		return &mcp.CallToolResult{Content: []mcp.Content{&mcp.ImageContent{Data: data, MIMEType: "image/png"}}}, nil
	})
	if c.controls {
		server.AddTool(&mcp.Tool{Name: "ui_action", Description: "Call a declared UI method using exact current revision and operation_id. Replay of the same ID only returns its original outcome. Send additionally requires server execution opt-in; normal permissions still apply.", InputSchema: json.RawMessage(`{"type":"object","additionalProperties":false,"required":["operation_id","view_id","revision","method","args"],"properties":{"operation_id":{"type":"string"},"view_id":{"type":"string"},"revision":{"type":"string"},"method":{"type":"string"},"args":{"type":"object"}}}`), Annotations: &mcp.ToolAnnotations{IdempotentHint: true, OpenWorldHint: &closedWorld}}, func(ctx context.Context, r *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
			result, err := c.Action(ctx, r.Params.Arguments)
			data, _ := json.Marshal(result)
			reply := textResult(data, err)
			if result.Status == "rejected" || result.Status == "cancelled" {
				reply.IsError = true
			}
			return reply, nil
		})
		server.AddTool(&mcp.Tool{Name: "ui_diagnostic_control", Description: "Set bounded diagnostic logging level or shut down only this diagnostic transport. Agent execution ownership is unchanged.", InputSchema: json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"enum":["level","shutdown"]},"level":{"type":"string"}}}`)}, func(ctx context.Context, r *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
			data, err := c.Control(ctx, r.Params.Arguments)
			return textResult(data, err), nil
		})
	}
	defer c.Close()
	return server.Run(ctx, &mcp.IOTransport{Reader: input, Writer: output, MaxLineLength: 8 << 20})
}
