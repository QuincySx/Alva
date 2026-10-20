// INPUT: context, encoding/json, errors, filepath, sync/time, core/security, ABI/wire, shared storage contracts.
// OUTPUT: Explicit optional Config, SearchConfig, Activity and session-bound web Plugin with exact Tools.
// POS: [L4 extension] Web policy and durable source references stay in an optional Plugin, not the SDK kernel.
package web

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Storage is the capability contract supplied by the outer assembly.
type Storage interface {
	storage.Journal
}

type SearchConfig struct {
	// Only the actual DuckDuckGo Instant Answer JSON API is implemented.
	// The host explicitly chooses its endpoint; no automatic service or key discovery.
	Kind, Endpoint string
}
type Activity struct {
	ID, Name, SessionID, RunID, CallID string
	Effect                             abi.EffectKind
}
type Config struct {
	Repository                      Storage
	Owner, SessionID, Cwd           string
	Guard                           *security.Guard
	ApprovedNetworkAccess, ReadURLs bool
	Search                          *SearchConfig
	Network                         NetworkPolicy
	Timeout                         time.Duration
	Report                          func(context.Context, Activity) error
}
type cacheEntry struct {
	Receipt receipt
	Expires time.Time
}
type rateEntry struct {
	At    time.Time
	Count int
}
type Plugin struct {
	config     Config
	log        storage.Session
	tools      []*webTool
	mu         sync.Mutex
	runID      string
	closed     bool
	life       context.Context
	cancel     context.CancelFunc
	gate       chan struct{}
	cache      map[string]cacheEntry
	rates      map[string]rateEntry
	operations int
}

func New(ctx context.Context, config Config) (*Plugin, error) {
	if storage.Missing(config.Repository) || config.Guard == nil || config.Report == nil || !config.ApprovedNetworkAccess || !filepath.IsAbs(config.Cwd) || (!config.ReadURLs && config.Search == nil) {
		return nil, errors.New("explicit web authority, guard, risk reporter and approved network capability required")
	}
	if config.Timeout == 0 {
		config.Timeout = 30 * time.Second
	}
	if config.Timeout <= 0 || config.Timeout > time.Minute || len(config.Network.AllowedPrivateOrigins) > 16 {
		return nil, errors.New("bounded web timeout/private-origin policy required")
	}
	config.Network.AllowedPrivateOrigins = append([]string(nil), config.Network.AllowedPrivateOrigins...)
	for _, raw := range config.Network.AllowedPrivateOrigins {
		u, err := parseURL(raw)
		if err != nil || origin(u) != raw || u.Path != "" || u.RawQuery != "" {
			return nil, errors.New("private web grants must be exact normalized HTTP origins")
		}
	}
	if config.Search != nil {
		copy := *config.Search
		u, err := parseURL(copy.Endpoint)
		if copy.Kind != "duckduckgo_instant" || err != nil || u.RawQuery != "" {
			return nil, errors.New("explicit supported Instant Answer endpoint without embedded query required")
		}
		config.Search = &copy
	}
	log, err := config.Repository.OpenSession(ctx, config.Owner, config.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: config, log: log, gate: make(chan struct{}, 1), cache: map[string]cacheEntry{}, rates: map[string]rateEntry{}}
	if config.ReadURLs {
		p.tools = append(p.tools, &webTool{plugin: p, name: "read_url"})
	}
	if config.Search != nil {
		p.tools = append(p.tools, &webTool{plugin: p, name: "internet_search"})
	}
	return p, nil
}
func (*Plugin) Name() string { return "web" }
func (p *Plugin) Tools() []abi.Tool {
	result := make([]abi.Tool, len(p.tools))
	for i, t := range p.tools {
		result[i] = t
	}
	return result
}
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	for _, t := range p.tools {
		if err := r.Tool(t); err != nil {
			return err
		}
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	return r.SystemPrompt("Web tools access the network only when explicitly called and approved. Every full source and derived result is retained as an untrusted same-session reference; read/search selected evidence rather than importing whole bodies. read_url extracts HTML text only (not Markdown/reader-mode or media understanding). internet_search, if present, uses an explicitly configured DuckDuckGo Instant Answer/RelatedTopics API, not exhaustive web search. No ambient proxy, credentials, cookies or automatic retries. HTTP status and processing receipts are not claims of correctness or model delivery.")
}
func (p *Plugin) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.runID != "" || runtime.State == nil || !p.log.SameAuthority(runtime.State.Session()) || runtime.RunID == "" || runtime.Workspace != p.config.Cwd {
		return errors.New("web actual active session/workspace mismatch")
	}
	p.runID, p.operations = runtime.RunID, 0
	p.life, p.cancel = context.WithCancel(ctx)
	return nil
}
func (p *Plugin) check(e abi.ToolExecutionContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.runID == "" || p.runID != e.RunID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd || p.life.Err() != nil {
		return errors.New("web capability is not bound to this actual active native run")
	}
	return nil
}
func (p *Plugin) OnRunEnd(ctx context.Context, runtime abi.RuntimeContext, _ error) error {
	p.mu.Lock()
	if p.runID == runtime.RunID && p.cancel != nil {
		p.cancel()
	}
	p.mu.Unlock()
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	case <-cleanup.Done():
		return cleanup.Err()
	}
	p.mu.Lock()
	if p.runID == runtime.RunID {
		p.runID = ""
	}
	p.mu.Unlock()
	return nil
}
func (p *Plugin) Close(ctx context.Context) error {
	p.mu.Lock()
	p.closed = true
	if p.cancel != nil {
		p.cancel()
	}
	p.mu.Unlock()
	select {
	case p.gate <- struct{}{}:
		<-p.gate
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (p *Plugin) CleanupComplete() bool {
	select {
	case p.gate <- struct{}{}:
		<-p.gate
		return true
	default:
		return false
	}
}
func (p *Plugin) append(ctx context.Context, kind string, value any) error {
	payload, err := json.Marshal(value)
	if err != nil {
		return err
	}
	_, err = p.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.web", Type: kind, SchemaVersion: 1, Payload: payload}})
	if err != nil {
		return &abi.SessionError{Operation: "append web " + kind, SessionID: p.config.SessionID, Err: err}
	}
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush web " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
func (p *Plugin) retain(ctx context.Context, kind, name, media, processing string, data []byte) (wire.Reference, error) {
	clean, redacted, err := sanitize(data)
	if err != nil {
		return wire.Reference{}, err
	}
	if redacted {
		processing += "; known credentials redacted"
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, storage.ArtifactInput{Kind: kind, Name: name, MediaType: media, Processing: processing + "; untrusted source; no automatic model delivery", Data: clean, Text: json.Valid(clean) || isText(clean)})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain web " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
