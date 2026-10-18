// INPUT: Explicit loopback Hub address/token, bounded read-only arguments and owned stdio streams
// OUTPUT: New diagnostic Client, Inspect and Serve using the official MCP server
// POS: [L7 Hub product] Read-only diagnostic transport never constructs an Agent or accepts arbitrary host/HTTP paths.
package devtools

import (
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

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	"github.com/QuincySx/Alva/go/platform/system/loopback"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

type Config struct {
	Address string
	Token   string `json:"-"`
}
type Client struct {
	address, token string
	http           *http.Client
	gate           chan struct{}
}
type childHop struct {
	LinkSequence uint64 `json:"link_sequence"`
}
type Arguments struct {
	Action    string     `json:"action"`
	Session   string     `json:"session_id,omitempty"`
	Branch    string     `json:"branch_id,omitempty"`
	Query     string     `json:"query,omitempty"`
	After     uint64     `json:"after,omitempty"`
	HighWater uint64     `json:"high_water,omitempty"`
	URI       string     `json:"uri,omitempty"`
	Offset    int64      `json:"offset,omitempty"`
	ChildPath []childHop `json:"child_path,omitempty"`
}

func New(config Config) (*Client, error) {
	u, err := url.Parse(config.Address)
	if err != nil || u.Scheme != "http" || u.Host == "" || u.Path != "" || u.RawQuery != "" || u.Fragment != "" || u.User != nil || u.String() != config.Address {
		return nil, errors.New("explicit canonical numeric loopback Hub address required")
	}
	if _, err = loopback.New(u.Host, config.Token); err != nil {
		return nil, err
	}
	transport := &http.Transport{Proxy: nil, MaxIdleConns: 4, MaxIdleConnsPerHost: 4, IdleConnTimeout: 30 * time.Second, ResponseHeaderTimeout: 5 * time.Second}
	return &Client{address: config.Address, token: config.Token, http: &http.Client{Transport: transport, Timeout: 5 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}, gate: make(chan struct{}, 4)}, nil
}
func validID(id string) bool {
	if id == "" || len(id) > 128 {
		return false
	}
	for _, c := range id {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}
func (a Arguments) route() (string, error) {
	if len(a.Query) > 256 || !utf8.ValidString(a.Query) || len(a.URI) > 2048 || a.After > 1<<63-1 || a.HighWater > 1<<63-1 || a.Offset < 0 || len(a.ChildPath) > 8 {
		return "", errors.New("diagnostic query exceeds bounds")
	}
	q := url.Values{}
	if a.Action == "sessions" {
		if a.Session != "" || a.Branch != "" || a.After != 0 || a.HighWater != 0 || a.URI != "" || a.Offset != 0 || len(a.ChildPath) > 0 {
			return "", errors.New("sessions accepts query only")
		}
		q.Set("limit", "4")
		if a.Query != "" {
			q.Set("q", a.Query)
		}
		return "/v1/sessions?" + q.Encode(), nil
	}
	if !validID(a.Session) {
		return "", errors.New("actual product session required")
	}
	base := "/v1/sessions/" + a.Session
	if a.Action == "history" {
		if a.Branch != "" || a.Query != "" || a.HighWater != 0 || a.URI != "" || a.Offset != 0 || len(a.ChildPath) > 0 {
			return "", errors.New("history accepts product session and after only")
		}
		q.Set("limit", "8")
		q.Set("after", strconv.FormatUint(a.After, 10))
		return base + "/history?" + q.Encode(), nil
	}
	if a.Action != "inspector" && a.Action != "record" {
		return "", errors.New("unknown read-only diagnostic action")
	}
	if !validID(a.Branch) {
		return "", errors.New("actual saved branch required")
	}
	q.Set("branch", a.Branch)
	if len(a.ChildPath) > 0 {
		for _, hop := range a.ChildPath {
			if hop.LinkSequence == 0 || hop.LinkSequence > 1<<63-1 {
				return "", errors.New("typed child link sequence required")
			}
		}
		raw, _ := json.Marshal(a.ChildPath)
		q.Set("child_path", string(raw))
	}
	if a.Action == "record" {
		if a.URI == "" || a.Query != "" || a.After != 0 || a.HighWater != 0 {
			return "", errors.New("record accepts exact URI and bounded offset, not history query")
		}
		q.Set("uri", a.URI)
		q.Set("offset", strconv.FormatInt(a.Offset, 10))
		return base + "/artifact?" + q.Encode(), nil
	}
	if a.URI != "" || a.Offset != 0 {
		return "", errors.New("Inspector index cannot import a record body")
	}
	q.Set("q", a.Query)
	q.Set("after", strconv.FormatUint(a.After, 10))
	q.Set("high_water", strconv.FormatUint(a.HighWater, 10))
	return base + "/inspector?" + q.Encode(), nil
}
func (c *Client) Inspect(ctx context.Context, raw json.RawMessage) (json.RawMessage, error) {
	var args Arguments
	if len(raw) > 16<<10 || !wire.ValidJSON(raw) || httpjson.ValidateObject(raw, &args) != nil || json.Unmarshal(raw, &args) != nil {
		return nil, errors.New("strict bounded diagnostic arguments required")
	}
	path, err := args.route()
	if err != nil {
		return nil, err
	}
	select {
	case c.gate <- struct{}{}:
		defer func() { <-c.gate }()
	default:
		return nil, errors.New("diagnostic read concurrency limit reached")
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	r, err := http.NewRequestWithContext(ctx, http.MethodGet, c.address+path, nil)
	if err != nil {
		return nil, errors.New("diagnostic request construction failed")
	}
	r.Header.Set("Authorization", "Bearer "+c.token)
	response, err := c.http.Do(r)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("Hub diagnostic read failed")
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return nil, errors.New("Hub diagnostic read rejected; verify actual session/branch, permissions and server availability")
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, 1<<20+1))
	if err != nil || len(body) > 1<<20 || !wire.ValidJSON(body) {
		return nil, errors.New("Hub diagnostic response invalid or exceeds 1 MiB")
	}
	var value any
	if args.Action == "history" {
		var entries []hub.HistoryEntry
		if json.Unmarshal(body, &entries) != nil || len(entries) > 8 {
			return nil, errors.New("invalid bounded product history")
		}
		index := make([]map[string]any, 0, len(entries))
		for _, entry := range entries {
			preview := entry.Input
			if preview == "" {
				preview = entry.Event.Text
			}
			if entry.Timeline != nil {
				preview = entry.Timeline.Text
			}
			truncated := len(preview) > 256
			if truncated {
				preview = preview[:256]
				for !utf8.ValidString(preview) {
					preview = preview[:len(preview)-1]
				}
			}
			index = append(index, map[string]any{"sequence": entry.Sequence, "turn_id": entry.TurnID, "attempt_id": entry.AttemptID, "kind": entry.Event.Kind, "name": entry.Event.Name, "preview": preview, "preview_truncated": truncated, "reference": entry.Event.Reference})
		}
		value = map[string]any{"entries": index, "processing": "Bounded metadata/previews only. Read native Inspector records explicitly; no transcript replay or execution."}
	} else {
		if err = json.Unmarshal(body, &value); err != nil {
			return nil, errors.New("invalid Hub diagnostic response")
		}
	}
	result, err := json.Marshal(value)
	if err != nil || len(result) > 32<<10 {
		return nil, errors.New("diagnostic result exceeds 32 KiB; narrow query or use a smaller page/range")
	}
	return result, nil
}
func (c *Client) Serve(ctx context.Context, input io.ReadCloser, output io.WriteCloser) error {
	if input == nil || output == nil {
		return errors.New("owned diagnostic stdio streams required")
	}
	server := mcp.NewServer(&mcp.Implementation{Name: "alva-devtools", Version: "1"}, &mcp.ServerOptions{Capabilities: &mcp.ServerCapabilities{Tools: &mcp.ToolCapabilities{}}, Logger: slog.New(slog.NewTextHandler(io.Discard, nil)), Instructions: "Read-only actual Hub evidence. Begin with bounded sessions/history/Inspector indexes and read selected record ranges. No model, tool, approval or filesystem mutation is exposed."})
	schema := json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"type":"string","enum":["sessions","history","inspector","record"]},"session_id":{"type":"string"},"branch_id":{"type":"string"},"query":{"type":"string","maxLength":256},"after":{"type":"integer","minimum":0},"high_water":{"type":"integer","minimum":0},"uri":{"type":"string","maxLength":2048},"offset":{"type":"integer","minimum":0},"child_path":{"type":"array","maxItems":8,"items":{"type":"object","additionalProperties":false,"required":["link_sequence"],"properties":{"link_sequence":{"type":"integer","minimum":1}}}}}}`)
	closedWorld := false
	server.AddTool(&mcp.Tool{Name: "hub_inspect", Description: "Read bounded existing Hub product/native evidence. sessions discovers product IDs; history returns 8 metadata/previews; inspector requires saved branch and optional typed child-link sequence path; record reads explicit 2 KiB text. Never restores or executes an Agent.", InputSchema: schema, Annotations: &mcp.ToolAnnotations{ReadOnlyHint: true, IdempotentHint: true, OpenWorldHint: &closedWorld}}, func(ctx context.Context, request *mcp.CallToolRequest) (*mcp.CallToolResult, error) {
		result, err := c.Inspect(ctx, request.Params.Arguments)
		if err != nil {
			return &mcp.CallToolResult{IsError: true, Content: []mcp.Content{&mcp.TextContent{Text: err.Error()}}}, nil
		}
		return &mcp.CallToolResult{Content: []mcp.Content{&mcp.TextContent{Text: string(result)}}}, nil
	})
	defer c.http.CloseIdleConnections()
	return server.Run(ctx, &mcp.IOTransport{Reader: input, Writer: output, MaxLineLength: 1 << 20})
}
