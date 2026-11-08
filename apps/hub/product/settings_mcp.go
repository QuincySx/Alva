// INPUT: SettingsService scopes, unredacted MCP server snapshots and a program-installed MCPConnector
// OUTPUT: MCPConnector, MCPProbe, MCPCredential and explicit ProbeMCP/AuthorizeMCP/RevokeMCP actions
// POS: [L7 Hub product] Connection checks and authorization are explicit user actions on approved servers only
package product

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

// MCPConnector is installed by bootstrap; product code never imports the
// protocol, a keychain or a browser. Probe starts and closes one worker.
type MCPConnector interface {
	Probe(ctx context.Context, workspace string, server hub.MCPServer) MCPProbe
	CredentialStatus(ctx context.Context, server hub.MCPServer) MCPCredential
	Authorize(ctx context.Context, server hub.MCPServer, open func(string) error) (MCPCredential, error)
	Revoke(ctx context.Context, server hub.MCPServer) (MCPCredential, error)
}

const (
	MCPProbeConnected          = "connected"
	MCPProbeFailed             = "failed"
	MCPProbeNeedsAuthorization = "needs_authorization"

	MCPCredentialNone        = "none"
	MCPCredentialAuthorized  = "authorized"
	MCPCredentialExpired     = "expired"
	MCPCredentialUnsupported = "unsupported"
)

// MCPProbeToolLimit bounds the tool summaries kept per probe result.
const MCPProbeToolLimit = 256

type MCPToolSummary struct {
	Name        string `json:"name"`
	Description string `json:"description,omitempty"`
}

// MCPProbe is kept in memory only. Problem is already redacted.
type MCPProbe struct {
	State         string           `json:"state"`
	ServerName    string           `json:"server_name,omitempty"`
	ServerVersion string           `json:"server_version,omitempty"`
	Tools         []MCPToolSummary `json:"tools,omitempty"`
	TotalTools    int              `json:"total_tools"`
	Problem       string           `json:"problem,omitempty"`
	CheckedAt     int64            `json:"checked_at"`
}

// MCPCredential never carries token material.
type MCPCredential struct {
	State     string   `json:"state"`
	Issuer    string   `json:"issuer,omitempty"`
	Scopes    []string `json:"scopes,omitempty"`
	ExpiresAt int64    `json:"expires_at,omitempty"`
	Problem   string   `json:"problem,omitempty"`
}

type mcpState struct {
	mu          sync.Mutex
	probes      map[string]MCPProbe
	authorizing map[string]bool
}

func (s *SettingsService) forgetProbes() {
	s.mcp.mu.Lock()
	s.mcp.probes = nil
	s.mcp.mu.Unlock()
}

func probeKey(scope SettingsScope, serverID string) string {
	prefix := "session:"
	if scope.App {
		prefix = "app:"
	}
	return prefix + scope.SessionID + "\x00" + serverID
}

// Probe returns the last explicit check of one server in this scope.
func (s *SettingsService) Probe(scope SettingsScope, serverID string) (MCPProbe, bool) {
	s.mcp.mu.Lock()
	defer s.mcp.mu.Unlock()
	value, ok := s.mcp.probes[probeKey(scope, serverID)]
	return value, ok
}

// server resolves the unredacted snapshot and whether it may run for the
// context task: a workspace approval in application scope, the snapshot
// approval in a session override.
func (s *SettingsService) server(ctx context.Context, scope SettingsScope, serverID string) (hub.MCPServer, string, bool, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	source, err := s.source(ctx, scope)
	if err != nil {
		return hub.MCPServer{}, "", false, err
	}
	view := s.view(scope, source)
	for _, server := range source.capabilities(scope).MCP {
		if server.ID == serverID {
			item, _ := view.Item(hub.CapabilityMCP, serverID)
			return server, source.detail.Session.Workspace, item.Approved, nil
		}
	}
	return hub.MCPServer{}, "", false, errors.New("找不到这个 MCP 服务器，请重新载入。")
}

// ProbeMCP is an explicit action: it starts the approved server once, lists
// its tools and closes it. It never runs for an unapproved snapshot.
func (s *SettingsService) ProbeMCP(ctx context.Context, scope SettingsScope, serverID string) (MCPProbe, error) {
	if s.mcpConnector == nil {
		return MCPProbe{}, errors.New("当前程序没有安装 MCP 连接检测。")
	}
	server, workspace, approved, err := s.server(ctx, scope, serverID)
	if err != nil {
		return MCPProbe{}, err
	}
	if scope.SessionID == "" || workspace == "" {
		return MCPProbe{}, errors.New("请先选择任务：检测会在该任务的工作区里运行。")
	}
	if !approved {
		return MCPProbe{}, errors.New("请先在当前工作区启用并批准这个服务器。")
	}
	result := s.mcpConnector.Probe(ctx, workspace, server)
	if result.CheckedAt == 0 {
		result.CheckedAt = time.Now().UnixMilli()
	}
	if len(result.Tools) > MCPProbeToolLimit {
		result.Tools = result.Tools[:MCPProbeToolLimit]
	}
	s.mcp.mu.Lock()
	if s.mcp.probes == nil {
		s.mcp.probes = map[string]MCPProbe{}
	}
	s.mcp.probes[probeKey(scope, serverID)] = result
	s.mcp.mu.Unlock()
	return result, nil
}

func (s *SettingsService) remoteOAuth(ctx context.Context, scope SettingsScope, serverID string) (hub.MCPServer, error) {
	if s.mcpConnector == nil {
		return hub.MCPServer{}, errors.New("当前程序不支持 MCP 授权。")
	}
	server, _, _, err := s.server(ctx, scope, serverID)
	if err != nil {
		return hub.MCPServer{}, err
	}
	if !server.Remote() || server.Auth != hub.MCPAuthOAuth {
		return hub.MCPServer{}, errors.New("这个服务器没有使用 OAuth 授权。")
	}
	return server, nil
}

// MCPCredential reports the saved authorization without reading token values
// into the product.
func (s *SettingsService) MCPCredential(ctx context.Context, scope SettingsScope, serverID string) (MCPCredential, error) {
	server, err := s.remoteOAuth(ctx, scope, serverID)
	if err != nil {
		return MCPCredential{}, err
	}
	return s.mcpConnector.CredentialStatus(ctx, server), nil
}

// AuthorizeMCP runs the browser authorization once per server at a time.
// open receives only an https (or loopback http) authorization address.
func (s *SettingsService) AuthorizeMCP(ctx context.Context, scope SettingsScope, serverID string, open func(string) error) (MCPCredential, error) {
	server, err := s.remoteOAuth(ctx, scope, serverID)
	if err != nil {
		return MCPCredential{}, err
	}
	if open == nil {
		return MCPCredential{}, errors.New("这个入口无法打开系统浏览器。")
	}
	key := server.URL
	s.mcp.mu.Lock()
	if s.mcp.authorizing[key] {
		s.mcp.mu.Unlock()
		return MCPCredential{}, errors.New("这个服务器的授权正在进行。")
	}
	if s.mcp.authorizing == nil {
		s.mcp.authorizing = map[string]bool{}
	}
	s.mcp.authorizing[key] = true
	s.mcp.mu.Unlock()
	defer func() {
		s.mcp.mu.Lock()
		delete(s.mcp.authorizing, key)
		s.mcp.mu.Unlock()
	}()
	credential, err := s.mcpConnector.Authorize(ctx, server, open)
	s.forgetProbes()
	return credential, err
}

// RevokeMCP revokes at the authorization server when it supports RFC 7009
// and always deletes the local credential.
func (s *SettingsService) RevokeMCP(ctx context.Context, scope SettingsScope, serverID string) (MCPCredential, error) {
	server, err := s.remoteOAuth(ctx, scope, serverID)
	if err != nil {
		return MCPCredential{}, err
	}
	credential, err := s.mcpConnector.Revoke(ctx, server)
	s.forgetProbes()
	return credential, err
}

// InstallMCPConnector is called once by program assembly.
func (s *SettingsService) InstallMCPConnector(connector MCPConnector) {
	s.mu.Lock()
	s.mcpConnector = connector
	s.mu.Unlock()
}
