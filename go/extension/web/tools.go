// INPUT: bytes, context, crypto/sha256, encoding/hex/json, errors, io, mime/url, strings, time/utf8, ABI/wire, platform/system/httpjson.
// OUTPUT: Actual read_url and optional internet_search tools with durable receipts and bounded source references.
// POS: [L4 extension] Explicit identity/session/risk admission precedes any network side effect and source bodies stay out of model context.
package web

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func isText(b []byte) bool { return utf8.Valid(b) }

type receipt struct {
	Status   int             `json:"http_status"`
	Cached   bool            `json:"cached"`
	Source   wire.Reference  `json:"source"`
	Derived  *wire.Reference `json:"derived,omitempty"`
	Metadata wire.Reference  `json:"metadata"`
	Results  int             `json:"results,omitempty"`
	Notice   string          `json:"notice"`
}
type webTool struct {
	plugin *Plugin
	name   string
}

func (t *webTool) Name() string { return t.name }
func (t *webTool) Description() string {
	if t.name == "read_url" {
		return "Fetch an explicit HTTP/HTTPS URL with strict destination/redirect/size limits. Retain full source and optional HTML text as references; does not understand media or automatically send body to the model."
	}
	return "Query the host-configured DuckDuckGo Instant Answer/RelatedTopics endpoint, not exhaustive web search; return source and parsed-results references. No configured provider means this tool is absent."
}
func (*webTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal, ManagesOwnTimeout: true}
}
func (*webTool) IsReadOnly(wire.ToolCall) bool { return false }
func (*webTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown, Description: "Explicit network request can disclose URL/query or incur external effects; no ambient proxy/account access"}
}
func (t *webTool) ParametersSchema() json.RawMessage {
	if t.name == "read_url" {
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["url"],"properties":{"url":{"type":"string","maxLength":8192}}}`)
	}
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["query"],"properties":{"query":{"type":"string","maxLength":4096},"max_results":{"type":"integer","minimum":1,"maximum":20},"allowed_domains":{"type":"array","maxItems":20,"items":{"type":"string"}},"blocked_domains":{"type":"array","maxItems":20,"items":{"type":"string"}}}}`)
}
func (t *webTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var result wire.ToolResult
	if e.ActualTool != t || call.Name != t.name || call.ID == "" || e.CallID != call.ID {
		return result, errors.New("web requires actual dispatched tool/call authority")
	}
	if len(call.Arguments) > 16<<10 || !utf8.Valid(call.Arguments) {
		return result, errors.New("web input exceeds 16 KiB UTF-8")
	}
	if err := httpjson.Validate(call.Arguments); err != nil {
		return result, err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(call.Arguments, &fields) != nil || fields == nil {
		return result, errors.New("web input must be an object")
	}
	var schema struct{ Properties map[string]any }
	json.Unmarshal(t.ParametersSchema(), &schema)
	for key, value := range fields {
		if _, ok := schema.Properties[key]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return result, errors.New("unknown/null web field")
		}
	}
	var args struct {
		URL, Query string
		MaxResults int      `json:"max_results"`
		Allowed    []string `json:"allowed_domains"`
		Blocked    []string `json:"blocked_domains"`
	}
	d := json.NewDecoder(bytes.NewReader(call.Arguments))
	d.DisallowUnknownFields()
	if err := d.Decode(&args); err != nil {
		return result, err
	}
	if d.Decode(new(any)) != io.EOF {
		return result, errors.New("one web input object required")
	}
	raw := args.URL
	if t.name == "internet_search" {
		if len(args.Query) == 0 || len(args.Query) > 4096 || len(args.Allowed) > 20 || len(args.Blocked) > 20 {
			return result, errors.New("bounded explicit search query/domain filters required")
		}
		for _, domains := range [][]string{args.Allowed, args.Blocked} {
			for _, domain := range domains {
				if len(domain) == 0 || len(domain) > 253 || strings.ToLower(domain) != domain || strings.ContainsAny(domain, "/:@% \\?#") {
					return result, errors.New("domain filters require lowercase plain host names")
				}
			}
		}
		if args.MaxResults == 0 {
			args.MaxResults = 5
		}
		if args.MaxResults < 1 || args.MaxResults > 20 {
			return result, errors.New("search result limit 1..20 required")
		}
		u, _ := url.Parse(t.plugin.config.Search.Endpoint)
		q := u.Query()
		q.Set("q", args.Query)
		q.Set("format", "json")
		q.Set("no_html", "1")
		q.Set("skip_disambig", "1")
		u.RawQuery = q.Encode()
		raw = u.String()
	}
	u, err := parseURL(raw)
	if err != nil {
		return result, err
	}
	p := t.plugin
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return result, ctx.Err()
	}
	defer func() { <-p.gate }()
	if err = p.check(e); err != nil {
		return result, err
	}
	p.mu.Lock()
	life := p.life
	p.mu.Unlock()
	operation, stopOperation := context.WithCancel(ctx)
	defer stopOperation()
	stopOwner := context.AfterFunc(life, stopOperation)
	defer stopOwner()
	if life.Err() != nil {
		stopOperation()
	}
	ctx = operation
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if err = p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return result, err
	}
	if err = p.check(e); err != nil {
		return result, err
	}
	p.mu.Lock()
	p.operations++
	operations := p.operations
	p.mu.Unlock()
	if operations > 64 {
		return result, errors.New("web operation budget exceeds 64 per run")
	}
	keySum := sha256.Sum256(append([]byte(t.name+"\x00"), call.Arguments...))
	key := hex.EncodeToString(keySum[:])
	if item, ok := p.cache[key]; ok && time.Now().Before(item.Expires) {
		value := item.Receipt
		value.Cached = true
		data, _ := json.Marshal(value)
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}, IsError: value.Status < 200 || value.Status >= 300}, nil
	}
	now := time.Now()
	host := strings.ToLower(u.Hostname())
	rate := p.rates[host]
	if now.Sub(rate.At) >= time.Minute {
		rate = rateEntry{At: now}
	}
	if rate.Count >= 10 {
		return result, errors.New("web per-host request budget exceeds 10 per minute")
	}
	if len(p.rates) >= 128 {
		if _, ok := p.rates[host]; !ok {
			return result, errors.New("web host budget exceeds 128")
		}
	}
	rate.Count++
	p.rates[host] = rate
	idSum := sha256.Sum256([]byte(e.RunID + "\x00" + call.ID))
	id := "web-" + hex.EncodeToString(idSum[:16])
	if err = p.config.Report(ctx, Activity{ID: id, Name: t.name, SessionID: e.SessionID, RunID: e.RunID, CallID: e.CallID, Effect: abi.EffectUnknown}); err != nil {
		return result, err
	}
	if err = p.append(ctx, "request_intent", map[string]any{"run_id": e.RunID, "call_id": call.ID, "tool": t.name, "arguments": json.RawMessage(call.Arguments), "state": "request_attempted"}); err != nil {
		return result, err
	}
	deadline, cancel := context.WithTimeout(life, p.config.Timeout)
	stop := context.AfterFunc(ctx, cancel)
	defer cancel()
	defer stop()
	data, runErr := fetch(deadline, raw, p.config.Network)
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	value := receipt{Status: data.Status, Notice: "Full untrusted response retained only as same-session references; known-credential redaction applied; no automatic model delivery or media understanding."}
	if runErr == nil {
		media, _, parseErr := mime.ParseMediaType(data.MediaType)
		if parseErr != nil {
			media = "application/octet-stream"
		}
		value.Source, runErr = p.retain(cleanup, "web-source", id, media, "Complete HTTP response bytes; content encoding identity; no compression or frame extraction", data.Body)
		if runErr == nil && t.name == "read_url" && media == "text/html" && utf8.Valid(data.Body) {
			text, convertErr := htmlText(data.Body)
			if convertErr != nil {
				runErr = convertErr
			} else {
				ref, retainErr := p.retain(cleanup, "web-html-text", id, "text/plain; charset=utf-8", "HTML5-tokenizer text only; script/style/noscript/template skipped, whitespace normalized; not Markdown or semantic reader mode", text)
				runErr = retainErr
				value.Derived = &ref
			}
		}
		if runErr == nil && t.name == "internet_search" {
			results, parseErr := instantAnswers(data.Body, args.MaxResults, args.Allowed, args.Blocked)
			runErr = parseErr
			if runErr == nil {
				encoded, _ := json.Marshal(results)
				ref, retainErr := p.retain(cleanup, "web-search-results", id, "application/json", "Parsed Instant Answer abstract/related topics with requested exact/suffix domain filters; not exhaustive search", encoded)
				value.Results = len(results)
				value.Derived = &ref
				runErr = retainErr
			}
		}
		if runErr == nil {
			metadata, _ := json.Marshal(map[string]any{"initial_url": data.InitialURL, "final_url": data.FinalURL, "http_status": data.Status, "content_type": data.MediaType, "source_bytes": len(data.Body), "source": value.Source, "derived": value.Derived})
			value.Metadata, runErr = p.retain(cleanup, "web-response-metadata", id, "application/json", "Actual response provenance, raw/derived hashes and processing references", metadata)
		}
	}
	outcome := map[string]any{"run_id": e.RunID, "call_id": call.ID, "tool": t.name, "receipt": value, "succeeded": runErr == nil}
	if runErr != nil {
		outcome["error"] = "Web request or evidence retention failed; no retry and no successful prefix"
	}
	runErr = errors.Join(runErr, p.append(cleanup, "request_outcome", outcome))
	if runErr != nil {
		return result, runErr
	}
	if len(p.cache) >= 50 {
		for cacheKey, item := range p.cache {
			if now.After(item.Expires) {
				delete(p.cache, cacheKey)
			}
		}
		if len(p.cache) >= 50 {
			for cacheKey := range p.cache {
				delete(p.cache, cacheKey)
				break
			}
		}
	}
	p.cache[key] = cacheEntry{Receipt: value, Expires: now.Add(15 * time.Minute)}
	encoded, _ := json.Marshal(value)
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(encoded)}}, IsError: value.Status < 200 || value.Status >= 300}, nil
}
