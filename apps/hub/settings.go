// INPUT: Bounded explicit next-turn product capability/process snapshots and session model overrides
// OUTPUT: Settings and capability approvals validated without protocol or process imports
// POS: [L7 Hub product] Private product configuration snapshots; not SDK policy or execution authority
package hub

import (
	"errors"
	"io/fs"
	"net/http"
	"net/netip"
	"net/url"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type Settings struct {
	ModelOptions        ModelOptions                `json:"model_options,omitempty"`
	SessionModelOptions SessionModelOptionsSnapshot `json:"session_model_options,omitempty"`
	DurableWaits        bool                        `json:"durable_waits"`
	PlanMode            bool                        `json:"plan_mode,omitempty"`
	PermissionPolicy    PermissionPolicy            `json:"permission_policy,omitempty"`
	Version             int                         `json:"version"`
	SessionID           string                      `json:"session_id"`
	MCP                 []MCPServer                 `json:"mcp"`
	AEP                 []AEPBundle                 `json:"aep,omitempty"`
	SkillsEnabled       bool                        `json:"skills_enabled"`
	CompanionSkills     *CompanionSkillScope        `json:"companion_skills,omitempty"`
	SubagentsEnabled    bool                        `json:"subagents_enabled"`
	PersistentMemory    bool                        `json:"persistent_memory"`
	MediaEnabled        bool                        `json:"media_enabled"`
	MediaDecoder        MediaDecoder                `json:"media_decoder"`
	Browser             BrowserSettings             `json:"browser"`
	Commands            CommandSettings             `json:"commands"`
	Tools               ToolSettings                `json:"tools"`
	SkillArchives       []string                    `json:"skill_archives,omitempty"`
	CompatibilityFile   string                      `json:"compatibility_file,omitempty"`
	// CapabilityOverride makes this session keep its own capability snapshot
	// instead of application settings; see ResolveSettings.
	CapabilityOverride bool `json:"capability_override,omitempty"`
}
type MCPServer struct {
	ID                 string            `json:"id"`
	Name               string            `json:"name,omitempty"`
	Transport          string            `json:"transport,omitempty"`
	Command            string            `json:"command"`
	Args               []string          `json:"args,omitempty"`
	Env                map[string]string `json:"env,omitempty"`
	URL                string            `json:"url,omitempty"`
	Headers            map[string]string `json:"headers,omitempty"`
	Auth               string            `json:"auth,omitempty"`
	OAuthClientID      string            `json:"oauth_client_id,omitempty"`
	Enabled            bool              `json:"enabled"`
	HostAccessApproved bool              `json:"host_access_approved"`
}

// MCP transports and authorization modes. Empty Transport is the original
// local process snapshot, so stored sessions keep their meaning.
const (
	MCPTransportStdio = "stdio"
	MCPTransportHTTP  = "http"
	MCPAuthNone       = "none"
	MCPAuthOAuth      = "oauth"
)

// Remote reports a Streamable HTTP server.
func (s MCPServer) Remote() bool { return s.Transport == MCPTransportHTTP }

func (s Settings) Validate() error {
	if err := s.CompanionSkills.Validate(); err != nil {
		return err
	}
	if err := s.SessionModelOptions.Validate(); err != nil {
		return err
	}
	if err := s.ModelOptions.Validate(); err != nil {
		return err
	}
	if err := s.PermissionPolicy.Validate(); err != nil {
		return err
	}
	if s.CompatibilityFile != "" && (len(s.CompatibilityFile) > 1024 || s.CompatibilityFile == "." || !fs.ValidPath(s.CompatibilityFile) || strings.ContainsAny(s.CompatibilityFile, "\\\x00\r\n") || len(strings.Split(s.CompatibilityFile, "/")) > 16) {
		return errors.New("compatibility config must be a confined project-relative file")
	}
	if s.Version != 1 || s.SessionID == "" || len(s.MCP) > 32 || len(s.SkillArchives) > 128 {
		return errors.New("versioned session settings and at most 32 MCP servers required")
	}
	if err := ValidateAEP(s.AEP); err != nil {
		return err
	}
	if err := s.Commands.Validate(); err != nil {
		return err
	}
	if err := s.Tools.Validate(); err != nil {
		return err
	}
	if err := s.MediaDecoder.Validate(); err != nil {
		return err
	}
	if err := s.Browser.Validate(); err != nil {
		return err
	}
	for _, archive := range s.SkillArchives {
		if len(archive) == 0 || len(archive) > 256 || strings.ContainsAny(archive, "\x00\r\n") {
			return errors.New("invalid managed skill archive receipt")
		}
	}
	seen := map[string]bool{}
	total := 0
	for _, server := range s.MCP {
		if server.ID == "" || len(server.ID) > 128 || !utf8.ValidString(server.ID) || strings.ContainsAny(server.ID, "\x00\r\n") || seen[server.ID] || len(server.Name) > 120 || !utf8.ValidString(server.Name) || strings.ContainsAny(server.Name, "\x00\r\n") {
			return errors.New("invalid or duplicated explicit MCP process snapshot")
		}
		seen[server.ID] = true
		if server.Enabled && !server.HostAccessApproved {
			return errors.New("enabled MCP requires explicit host access approval")
		}
		n, err := server.validateTransport()
		if err != nil {
			return err
		}
		total += n
	}
	if total > 64<<10 {
		return errors.New("MCP settings exceed 64 KiB total snapshot budget")
	}
	return nil
}

// validateTransport checks one transport's fields and returns its budget bytes.
func (server MCPServer) validateTransport() (int, error) {
	switch server.Transport {
	case "", MCPTransportStdio:
		if server.URL != "" || len(server.Headers) != 0 || server.Auth != "" && server.Auth != MCPAuthNone || server.OAuthClientID != "" {
			return 0, errors.New("local MCP process cannot carry remote address, headers or authorization")
		}
		if server.Command == "" || len(server.Command) > 4096 || strings.ContainsRune(server.Command, 0) || (!filepath.IsAbs(server.Command) && strings.ContainsAny(server.Command, "/\\")) || len(server.Args) > 128 || len(server.Env) > 64 {
			return 0, errors.New("invalid or duplicated explicit MCP process snapshot")
		}
		total := len(server.Command)
		for _, arg := range server.Args {
			if strings.ContainsRune(arg, 0) {
				return 0, errors.New("invalid MCP argument")
			}
			total += len(arg)
		}
		for key, value := range server.Env {
			if key == "" || len(key) > 128 || strings.ContainsAny(key, "=\x00") || len(value) > 8192 || strings.ContainsRune(value, 0) {
				return 0, errors.New("invalid explicit MCP environment")
			}
			total += len(key) + len(value)
		}
		return total, nil
	case MCPTransportHTTP:
	default:
		return 0, errors.New("unknown MCP transport")
	}
	if server.Command != "" || len(server.Args) != 0 || len(server.Env) != 0 {
		return 0, errors.New("remote MCP server cannot carry a local command")
	}
	address, err := url.Parse(server.URL)
	if err != nil || len(server.URL) > 2048 || !utf8.ValidString(server.URL) || strings.ContainsAny(server.URL, "\x00\r\n\t ") || address.Host == "" || address.User != nil || address.Fragment != "" || (address.Scheme != "https" && address.Scheme != "http") {
		return 0, errors.New("remote MCP requires a complete HTTP(S) address without credentials or fragment (maximum 2048 bytes)")
	}
	switch server.Auth {
	case "", MCPAuthNone:
		if server.OAuthClientID != "" {
			return 0, errors.New("OAuth client ID requires OAuth authorization")
		}
	case MCPAuthOAuth:
		if address.Scheme != "https" && !LoopbackHost(address.Hostname()) {
			return 0, errors.New("OAuth requires an HTTPS MCP address; plain HTTP is allowed only on loopback")
		}
		if len(server.OAuthClientID) > 256 || !utf8.ValidString(server.OAuthClientID) || strings.ContainsAny(server.OAuthClientID, "\x00\r\n\t ") {
			return 0, errors.New("invalid OAuth client ID")
		}
	default:
		return 0, errors.New("unknown MCP authorization")
	}
	if len(server.Headers) > 32 {
		return 0, errors.New("remote MCP allows at most 32 explicit headers")
	}
	total := len(server.URL) + len(server.OAuthClientID)
	for name, value := range server.Headers {
		if !validHeaderName(name) || len(value) > 8192 || strings.ContainsAny(value, "\x00\r\n") || !utf8.ValidString(value) {
			return 0, errors.New("invalid explicit MCP header")
		}
		switch http.CanonicalHeaderKey(name) {
		case "Host", "Content-Length", "Transfer-Encoding", "Connection", "Content-Type", "Accept", "Mcp-Session-Id", "Mcp-Protocol-Version", "Last-Event-Id":
			return 0, errors.New("transport-controlled MCP header")
		case "Authorization":
			if server.Auth == MCPAuthOAuth {
				return 0, errors.New("OAuth owns the Authorization header")
			}
		}
		total += len(name) + len(value)
	}
	return total, nil
}

func validHeaderName(name string) bool {
	if name == "" || len(name) > 128 {
		return false
	}
	for _, r := range name {
		if r > 127 || !(r >= 'a' && r <= 'z' || r >= 'A' && r <= 'Z' || r >= '0' && r <= '9' || strings.ContainsRune("!#$%&'*+-.^_`|~", r)) {
			return false
		}
	}
	return true
}

// LoopbackHost reports a literal loopback IP or localhost name.
func LoopbackHost(host string) bool {
	if strings.EqualFold(host, "localhost") {
		return true
	}
	address, err := netip.ParseAddr(strings.Trim(host, "[]"))
	return err == nil && address.Unmap().IsLoopback()
}
