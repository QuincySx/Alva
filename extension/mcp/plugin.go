// INPUT: context, errors, os, sync, time, agent/core, agent/security, kernel/abi, protocol/mcp, shared storage contracts.
// OUTPUT: Config, Plugin, New, Close, CleanupComplete, Stats.
// POS: [L4 extension] Explicit approved stdio snapshot with lazy owned MCP workers and one actual-session gateway.
package managedmcp

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	protocol "github.com/QuincySx/Alva/integration/protocol/mcp"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

// Servers is an already approved, immutable product snapshot. New never scans
// existing configuration, installs software, reads credentials or starts I/O.
type Config struct {
	Repository      Storage
	Owner, NativeID string
	Guard           *security.Guard
	Servers         []protocol.Config
}
type opener func(context.Context, protocol.Config, protocol.Observer) (protocol.Transport, error)
type server struct {
	config protocol.Config
	client protocol.Transport
	tools  []protocol.Tool
	failed bool
}
type Plugin struct {
	mu                         sync.Mutex
	gate                       chan struct{}
	guard                      *security.Guard
	id                         string
	servers                    []*server
	journal                    *journal
	open                       opener
	runID                      string
	runCtx                     context.Context
	cancel                     context.CancelFunc
	closed                     bool
	cleanupErr                 error
	catalogBytes, catalogTools int
	gateway                    *gateway
}

func New(ctx context.Context, config Config) (*Plugin, error) {
	if storage.Missing(config.Repository) || config.Guard == nil || config.NativeID == "" || len(config.Servers) > 32 {
		return nil, errors.New("MCP requires explicit durable native session, security guard and at most 32 approved servers")
	}
	log, err := config.Repository.OpenSession(ctx, config.Owner, config.NativeID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{gate: make(chan struct{}, 1), guard: config.Guard, id: config.NativeID, open: func(ctx context.Context, c protocol.Config, o protocol.Observer) (protocol.Transport, error) {
		client, err := protocol.Open(ctx, c, o)
		if err != nil {
			return nil, err
		}
		return client, nil
	}}
	p.journal = &journal{repository: config.Repository, log: log, owner: config.Owner, id: config.NativeID, maxBytes: journalBytes, maxRecords: journalRecords, secrets: map[string][]string{}}
	p.gateway = &gateway{plugin: p}
	seen := map[string]bool{}
	for _, item := range config.Servers {
		if item.ID == "" || len(item.ID) > 128 || !utf8.ValidString(item.ID) || strings.ContainsAny(item.ID, "\x00\r\n") || seen[item.ID] || !filepath.IsAbs(item.Process.Cwd) {
			return nil, errors.New("MCP server identity/cwd invalid or duplicated")
		}
		seen[item.ID] = true
		item.Process.Args = append([]string(nil), item.Process.Args...)
		item.Process.ReadFiles = append([]*os.File(nil), item.Process.ReadFiles...)
		item.Process.DirectoryFiles = append([]*os.File(nil), item.Process.DirectoryFiles...)
		environment := make(map[string]string, len(item.Process.Env))
		for key, value := range item.Process.Env {
			environment[key] = value
		}
		item.Process.Env = environment
		p.journal.secrets[item.ID] = redaction.EnvironmentSecrets(environment)
		p.servers = append(p.servers, &server{config: item})
	}
	return p, nil
}
func (*Plugin) Name() string { return "managed-mcp" }

// Tool is the same actual instance registered by Plugin. A trusted host may
// include it in its security recipe, never register it a second time or label
// it read-only. Its effect is still Unknown and still requires HITL.
func (p *Plugin) Tool() abi.Tool { return p.gateway }

// RebindChild is a trusted host recipe, not a model API. The actual repository
// must already identify a direct child; parent journal/worker handles are never
// copied. The caller must separately verify the direct-parent tool grant.
func (p *Plugin) RebindChild(ctx context.Context, childID string) (*Plugin, error) {
	parent, err := p.journal.repository.Parent(ctx, p.journal.owner, childID)
	if err != nil {
		return nil, err
	}
	if parent != p.id {
		return nil, errors.New("MCP child must be an actual direct child of this session")
	}
	configs := make([]protocol.Config, len(p.servers))
	for index, item := range p.servers {
		configs[index] = item.config
	}
	return New(ctx, Config{Repository: p.journal.repository, Owner: p.journal.owner, NativeID: childID, Guard: p.guard, Servers: configs})
}
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Middleware(p); err != nil {
		return err
	}
	if err := r.Tool(p.gateway); err != nil {
		return err
	}
	// No schemas, remote descriptions, process arguments or environment in prompt.
	return r.SystemPrompt("MCP is available only on demand through mcp_runtime. Its list_servers action lists approved server IDs without starting processes. Discovery/calls require host approval and lazily start only selected servers. Full redacted input/output, intermediate RPC, idle notifications and stderr remain in this actual session's durable journal, not automatically in context. Results contain small references: use context_read/context_search for bounded literal text retrieval, or native_history for record metadata/search. No JSON-path extraction, second AI, implicit credential discovery, automatic replay, media compression or understanding. " + sourceNotice)
}
func (p *Plugin) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if runtime.State == nil || !p.journal.log.SameAuthority(runtime.State.Session()) || runtime.RunID == "" {
		return errors.New("MCP actual session/run binding mismatch")
	}
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	default:
		return errors.New("MCP worker operation or cleanup still active")
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.runID != "" {
		return errors.New("MCP plugin is closed or already running")
	}
	if p.cleanupErr != nil {
		return p.cleanupErr
	}
	for _, item := range p.servers {
		if item.client != nil {
			return errors.New("MCP worker cleanup incomplete; explicit Close required before reuse")
		}
	}
	if p.journal.stats().Failed {
		return ErrJournal
	}
	p.runID = runtime.RunID
	p.runCtx, p.cancel = context.WithCancel(ctx)
	return nil
}
func (p *Plugin) OnRunEnd(ctx context.Context, runtime abi.RuntimeContext, _ error) error {
	return p.stop(ctx, false, runtime.RunID)
}
func (p *Plugin) Stats() JournalStats             { return p.journal.stats() }
func (p *Plugin) Close(ctx context.Context) error { return p.stop(ctx, true, "") }
func (p *Plugin) stop(ctx context.Context, permanent bool, expectedRun string) error {
	p.mu.Lock()
	if !permanent && (expectedRun == "" || p.runID != expectedRun) {
		p.mu.Unlock()
		return nil
	}
	if permanent {
		p.closed = true
	}
	if p.cancel != nil {
		p.cancel()
	}
	p.mu.Unlock()
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	if deadline, ok := ctx.Deadline(); ok {
		bounded, end := context.WithDeadline(cleanup, deadline)
		defer end()
		cleanup = bounded
	}
	select {
	case p.gate <- struct{}{}:
	case <-cleanup.Done():
		return cleanup.Err()
	}
	defer func() { <-p.gate }()
	p.mu.Lock()
	runID := p.runID
	if !permanent && runID != expectedRun {
		p.mu.Unlock()
		return nil
	}
	p.mu.Unlock()
	var result error
	for _, item := range p.servers {
		if item.client != nil {
			err := item.client.Close(cleanup)
			result = errors.Join(result, err)
			if !workerClean(item.client, err) {
				item.failed = true
				continue
			}
			p.mu.Lock()
			p.cleanupErr = errors.Join(p.cleanupErr, err)
			p.mu.Unlock()
			item.client = nil
			item.tools = nil
			_, err = p.journal.append(cleanup, runID, protocol.IOEvent{Server: item.config.ID, Direction: "worker_status", Frame: []byte(`{"status":"closed","replay":false}`)})
			result = errors.Join(result, err)
			p.mu.Lock()
			p.cleanupErr = errors.Join(p.cleanupErr, err)
			p.mu.Unlock()
		}
		item.failed = false
	}
	p.mu.Lock()
	p.runID = ""
	p.cancel = nil
	p.runCtx = nil
	p.catalogBytes = 0
	p.catalogTools = 0
	result = errors.Join(result, p.cleanupErr)
	p.mu.Unlock()
	return result
}

// Close may retain an observer error after real cleanup. A transport without
// independent physical evidence is conservatively retained on any Close error.
func workerClean(client protocol.Transport, closeErr error) bool {
	if owned, ok := client.(interface{ CleanupComplete() bool }); ok {
		return owned.CleanupComplete()
	}
	return closeErr == nil
}

// CleanupComplete includes the gateway/cleanup gate: an active operation may
// still write native facts even after its child process has exited.
func (p *Plugin) CleanupComplete() bool {
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	default:
		return false
	}
	for _, item := range p.servers {
		if item.client != nil {
			return false
		}
	}
	return true
}
func (p *Plugin) record(ctx context.Context, runID, server, direction string, value any) (receipt, error) {
	frame, err := marshal(value)
	if err != nil {
		return receipt{}, err
	}
	return p.journal.append(ctx, runID, protocol.IOEvent{Server: server, Direction: direction, Frame: frame})
}

// Called only under the gateway gate, after approval. Failed/cancelled workers
// cannot be automatically restarted within this run.
func (p *Plugin) connect(ctx context.Context, item *server, runID string) error {
	if item.failed {
		return errors.New("MCP worker failed; no automatic reconnect/replay")
	}
	if item.client != nil {
		if item.client.Connected() {
			return nil
		}
		item.failed = true
		return errors.New("MCP worker disconnected; no automatic replay")
	}
	observer := protocol.ObserverFunc(func(ctx context.Context, event protocol.IOEvent) (string, error) {
		entry, err := p.journal.append(ctx, runID, event)
		return entry.Value.URI, err
	})
	timeout := item.config.ConnectTimeout
	if timeout == 0 {
		timeout = 30 * time.Second
	}
	connection, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	client, err := p.open(connection, item.config, observer)
	if err == nil {
		item.client = client
		item.tools, err = client.ListTools(connection)
		if err == nil {
			bytes := 0
			for _, tool := range item.tools {
				bytes += len(tool.Name) + len(tool.Description) + len(tool.InputSchema) + 64
			}
			if bytes > protocol.CatalogLimit-p.catalogBytes || len(item.tools) > protocol.ToolLimit-p.catalogTools {
				err = errors.New("MCP aggregate catalog budget exceeded")
			} else {
				p.catalogBytes += bytes
				p.catalogTools += len(item.tools)
			}
		}
	}
	if err != nil {
		item.failed = true
		item.client = nil
		if client != nil {
			cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			closeErr := client.Close(cleanup)
			err = errors.Join(err, closeErr)
			cancel()
			if !workerClean(client, closeErr) {
				item.client = client
			} else {
				p.mu.Lock()
				p.cleanupErr = errors.Join(p.cleanupErr, closeErr)
				p.mu.Unlock()
			}
		}
		item.tools = nil
		cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer end()
		_, recordErr := p.record(cleanup, runID, item.config.ID, "worker_status", map[string]any{"status": "failed_or_cancelled_acceptance_unknown", "replay": false})
		return errors.Join(err, recordErr)
	}
	_, err = p.record(ctx, runID, item.config.ID, "worker_status", map[string]any{"status": "connected", "tools": len(item.tools)})
	return err
}
