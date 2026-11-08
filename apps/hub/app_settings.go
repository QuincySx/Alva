// INPUT: Session next-turn Settings, application capability defaults and per-workspace approval facts
// OUTPUT: Capabilities, AppSettings, WorkspaceApproval, CapabilityItem, ResolveSettings and CarryEnablement
// POS: [L7 Hub product] One pure scope/approval policy; application intent never grants host access by itself
package hub

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

var ErrAppSettingsConflict = errors.New("application settings changed; reload before saving")

// Capabilities are the next-turn plugin/tool sections that a session either
// takes from application settings or replaces with its own override snapshot.
// Skills, compatibility files, durable waits, plan/permission and model
// parameters stay session-scoped because they bind project files or checkpoints.
type Capabilities struct {
	MCP              []MCPServer     `json:"mcp"`
	AEP              []AEPBundle     `json:"aep,omitempty"`
	SubagentsEnabled bool            `json:"subagents_enabled"`
	PersistentMemory bool            `json:"persistent_memory"`
	MediaEnabled     bool            `json:"media_enabled"`
	MediaDecoder     MediaDecoder    `json:"media_decoder"`
	Browser          BrowserSettings `json:"browser"`
	Commands         CommandSettings `json:"commands"`
	Tools            ToolSettings    `json:"tools"`
}

// AppSettings stores application intent only. Host-access approval flags are
// always false at rest; a workspace approval with the exact fingerprint grants them.
type AppSettings struct {
	Revision     uint64       `json:"revision"`
	Capabilities Capabilities `json:"capabilities"`
}

// WorkspaceApproval binds one host item to one saved workspace and the exact
// configuration fingerprint the user approved there.
type WorkspaceApproval struct {
	Workspace   string `json:"workspace"`
	Kind        string `json:"kind"`
	Item        string `json:"item"`
	Fingerprint string `json:"fingerprint"`
	ApprovedAt  int64  `json:"approved_at"`
}

// AppSettingsStore is an optional configuration port beside SettingsStore; it
// never widens the six-method ExecutionStore.
type AppSettingsStore interface {
	AppSettings(context.Context) (AppSettings, error)
	SaveAppSettings(context.Context, uint64, Capabilities) (AppSettings, error)
	WorkspaceApprovals(context.Context, string) ([]WorkspaceApproval, error)
	SaveWorkspaceApproval(context.Context, WorkspaceApproval) error
	DeleteWorkspaceApproval(context.Context, string, string, string) error
	ReplaceSessionCapabilities(context.Context, string, Capabilities) error
	SetSessionCapabilityEnabled(context.Context, string, string, string, bool) error
	SetCapabilityOverride(context.Context, string, bool, Capabilities) error
}

const (
	CapabilityMCP         = "mcp"
	CapabilityAEP         = "aep"
	CapabilityHook        = "hook"
	CapabilityDiagnostic  = "diagnostic"
	CapabilityShell       = "shell"
	CapabilityWeb         = "web"
	CapabilityBrowser     = "browser"
	CapabilityDecoder     = "decoder"
	CapabilityPlanning    = "planning"
	CapabilityNotebook    = "notebook"
	CapabilityInteraction = "interaction"
	CapabilityUtility     = "utility"
	CapabilityMedia       = "media"
	CapabilitySubagents   = "subagents"
	CapabilityMemory      = "memory"
)

// CapabilityItem is one switchable unit. Host items start processes, network
// or host code and need approval; their Fingerprint excludes enablement flags.
type CapabilityItem struct {
	Kind        string `json:"kind"`
	Item        string `json:"item"`
	Host        bool   `json:"host"`
	Enabled     bool   `json:"enabled"`
	Fingerprint string `json:"fingerprint,omitempty"`
}

func (i CapabilityItem) Key() string { return i.Kind + "\x00" + i.Item }

func (s Settings) Capabilities() Capabilities {
	return Capabilities{MCP: s.MCP, AEP: s.AEP, SubagentsEnabled: s.SubagentsEnabled, PersistentMemory: s.PersistentMemory,
		MediaEnabled: s.MediaEnabled, MediaDecoder: s.MediaDecoder, Browser: s.Browser, Commands: s.Commands, Tools: s.Tools}.Clone()
}

func (s Settings) WithCapabilities(c Capabilities) Settings {
	c = c.Clone()
	s.MCP, s.AEP, s.SubagentsEnabled, s.PersistentMemory = c.MCP, c.AEP, c.SubagentsEnabled, c.PersistentMemory
	s.MediaEnabled, s.MediaDecoder, s.Browser, s.Commands, s.Tools = c.MediaEnabled, c.MediaDecoder, c.Browser, c.Commands, c.Tools
	if s.MCP == nil {
		s.MCP = []MCPServer{}
	}
	return s
}

// OverridesCapabilities is the single scope rule: an explicit override, or a
// session that already stored any capability configuration, keeps its own snapshot.
func (s Settings) OverridesCapabilities() bool {
	return s.CapabilityOverride || !s.Capabilities().Empty()
}

// Empty compares the normalized snapshot, so nil and empty collections agree.
func (c Capabilities) Empty() bool {
	return fingerprint(c.Clone()) == fingerprint(Capabilities{})
}

func cloneStrings(source map[string]string) map[string]string {
	if source == nil {
		return nil
	}
	result := make(map[string]string, len(source))
	for k, v := range source {
		result[k] = v
	}
	return result
}

func (c Capabilities) Clone() Capabilities {
	c.MCP = append([]MCPServer(nil), c.MCP...)
	for i := range c.MCP {
		c.MCP[i].Args = append([]string(nil), c.MCP[i].Args...)
		c.MCP[i].Env = cloneStrings(c.MCP[i].Env)
		c.MCP[i].Headers = cloneStrings(c.MCP[i].Headers)
	}
	c.AEP = append([]AEPBundle(nil), c.AEP...)
	for i := range c.AEP {
		c.AEP[i].InterpreterArgs = append([]string(nil), c.AEP[i].InterpreterArgs...)
		c.AEP[i].Env = cloneStrings(c.AEP[i].Env)
	}
	c.Browser.Env = cloneStrings(c.Browser.Env)
	c.Tools.Shell.Env = cloneStrings(c.Tools.Shell.Env)
	c.Tools.Web.AllowedPrivateOrigins = append([]string(nil), c.Tools.Web.AllowedPrivateOrigins...)
	c.Commands.Hooks = append([]HookSpec(nil), c.Commands.Hooks...)
	for i := range c.Commands.Hooks {
		c.Commands.Hooks[i].Args = append([]string(nil), c.Commands.Hooks[i].Args...)
		c.Commands.Hooks[i].Env = cloneStrings(c.Commands.Hooks[i].Env)
	}
	c.Commands.Diagnostics = append([]DiagnosticSpec(nil), c.Commands.Diagnostics...)
	for i := range c.Commands.Diagnostics {
		c.Commands.Diagnostics[i].Args = append([]string(nil), c.Commands.Diagnostics[i].Args...)
		c.Commands.Diagnostics[i].Env = cloneStrings(c.Commands.Diagnostics[i].Env)
	}
	return c
}

func fingerprint(value any) string {
	data, err := json.Marshal(value)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(data)
	return hex.EncodeToString(sum[:])
}

// Items lists every switch in a stable order with host fingerprints. Display
// names are not part of a fingerprint, so renaming keeps an approval valid.
func (c Capabilities) Items() []CapabilityItem {
	var items []CapabilityItem
	for _, server := range c.MCP {
		config := server
		config.Enabled, config.HostAccessApproved, config.Name = false, false, ""
		items = append(items, CapabilityItem{Kind: CapabilityMCP, Item: server.ID, Host: true, Enabled: server.Enabled, Fingerprint: fingerprint(config)})
	}
	for _, entry := range c.AEP {
		config := entry
		config.Enabled, config.HostAccessApproved = false, false
		items = append(items, CapabilityItem{Kind: CapabilityAEP, Item: entry.ID, Host: true, Enabled: entry.Enabled, Fingerprint: fingerprint(config)})
	}
	for _, hook := range c.Commands.Hooks {
		config := hook
		config.Enabled, config.HostAccessApproved = false, false
		items = append(items, CapabilityItem{Kind: CapabilityHook, Item: hook.ID, Host: true, Enabled: hook.Enabled, Fingerprint: fingerprint(config)})
	}
	for _, profile := range c.Commands.Diagnostics {
		config := profile
		config.Enabled, config.HostAccessApproved = false, false
		items = append(items, CapabilityItem{Kind: CapabilityDiagnostic, Item: profile.ID, Host: true, Enabled: profile.Enabled, Fingerprint: fingerprint(config)})
	}
	shell := c.Tools.Shell
	shell.Enabled, shell.HostAccessApproved = false, false
	web := c.Tools.Web
	web.Enabled, web.NetworkAccessApproved = false, false
	browser := c.Browser
	browser.Enabled, browser.HostAccessApproved = false, false
	decoder := c.MediaDecoder
	decoder.Enabled, decoder.HostAccessApproved = false, false
	items = append(items,
		CapabilityItem{Kind: CapabilityShell, Host: true, Enabled: c.Tools.Shell.Enabled, Fingerprint: fingerprint(shell)},
		CapabilityItem{Kind: CapabilityWeb, Host: true, Enabled: c.Tools.Web.Enabled, Fingerprint: fingerprint(web)},
		CapabilityItem{Kind: CapabilityBrowser, Host: true, Enabled: c.Browser.Enabled, Fingerprint: fingerprint(browser)},
		CapabilityItem{Kind: CapabilityDecoder, Host: true, Enabled: c.MediaDecoder.Enabled, Fingerprint: fingerprint(decoder)},
		CapabilityItem{Kind: CapabilityPlanning, Enabled: c.Tools.Planning},
		CapabilityItem{Kind: CapabilityNotebook, Enabled: c.Tools.Notebook},
		CapabilityItem{Kind: CapabilityInteraction, Enabled: c.Tools.Interaction},
		CapabilityItem{Kind: CapabilityUtility, Enabled: c.Tools.Utility},
		CapabilityItem{Kind: CapabilityMedia, Enabled: c.MediaEnabled},
		CapabilityItem{Kind: CapabilitySubagents, Enabled: c.SubagentsEnabled},
		CapabilityItem{Kind: CapabilityMemory, Enabled: c.PersistentMemory},
	)
	return items
}

// SetEnabled changes one switch. For host items approved is stored with the
// enablement; callers decide whether approval comes from the user or a workspace.
func (c *Capabilities) SetEnabled(kind, item string, enabled, approved bool) error {
	on, granted := enabled, enabled && approved
	found := false
	switch kind {
	case CapabilityMCP:
		for i := range c.MCP {
			if c.MCP[i].ID == item {
				c.MCP[i].Enabled, c.MCP[i].HostAccessApproved, found = on, granted, true
			}
		}
	case CapabilityAEP:
		for i := range c.AEP {
			if c.AEP[i].ID == item {
				c.AEP[i].Enabled, c.AEP[i].HostAccessApproved, found = on, granted, true
			}
		}
	case CapabilityHook:
		for i := range c.Commands.Hooks {
			if c.Commands.Hooks[i].ID == item {
				c.Commands.Hooks[i].Enabled, c.Commands.Hooks[i].HostAccessApproved, found = on, granted, true
			}
		}
	case CapabilityDiagnostic:
		for i := range c.Commands.Diagnostics {
			if c.Commands.Diagnostics[i].ID == item {
				c.Commands.Diagnostics[i].Enabled, c.Commands.Diagnostics[i].HostAccessApproved, found = on, granted, true
			}
		}
	case CapabilityShell:
		c.Tools.Shell.Enabled, c.Tools.Shell.HostAccessApproved, found = on, granted, item == ""
	case CapabilityWeb:
		c.Tools.Web.Enabled, c.Tools.Web.NetworkAccessApproved, found = on, granted, item == ""
	case CapabilityBrowser:
		c.Browser.Enabled, c.Browser.HostAccessApproved, found = on, granted, item == ""
	case CapabilityDecoder:
		c.MediaDecoder.Enabled, c.MediaDecoder.HostAccessApproved, found = on, granted, item == ""
	case CapabilityPlanning:
		c.Tools.Planning, found = on, item == ""
	case CapabilityNotebook:
		c.Tools.Notebook, found = on, item == ""
	case CapabilityInteraction:
		c.Tools.Interaction, found = on, item == ""
	case CapabilityUtility:
		c.Tools.Utility, found = on, item == ""
	case CapabilityMedia:
		c.MediaEnabled, found = on, item == ""
	case CapabilitySubagents:
		c.SubagentsEnabled, found = on, item == ""
	case CapabilityMemory:
		c.PersistentMemory, found = on, item == ""
	}
	if !found {
		return errors.New("unknown capability switch")
	}
	return nil
}

// Intent clears host-access approvals. It is the only shape stored as
// application settings, so application edits alone never grant host access.
func (c Capabilities) Intent() Capabilities {
	c = c.Clone()
	for _, item := range c.Items() {
		if item.Host {
			_ = c.SetEnabled(item.Kind, item.Item, item.Enabled, false)
		}
	}
	return c
}

// ValidateIntent checks application intent with each enabled host item treated as
// approved, so the ordinary section bounds and pairing rules apply unchanged.
func (c Capabilities) ValidateIntent() error {
	projected := c.Clone()
	for _, item := range projected.Items() {
		if item.Host {
			_ = projected.SetEnabled(item.Kind, item.Item, item.Enabled, true)
		}
	}
	return (Settings{Version: 1, SessionID: "application"}).WithCapabilities(projected).Validate()
}

func (a AppSettings) Validate() error {
	if a.Revision == 0 {
		return errors.New("application settings revision required")
	}
	if fingerprint(a.Capabilities.Clone()) != fingerprint(a.Capabilities.Intent()) {
		return errors.New("application settings cannot store host access approval")
	}
	return a.Capabilities.ValidateIntent()
}

// Validate keeps workspace approval facts bounded and identity-exact.
func (w WorkspaceApproval) Validate() error {
	if !filepath.IsAbs(w.Workspace) || len(w.Workspace) > 4096 || !utf8.ValidString(w.Workspace) || strings.ContainsAny(w.Workspace, "\x00\r\n") {
		return errors.New("approval requires an absolute saved workspace")
	}
	switch w.Kind {
	case CapabilityMCP, CapabilityAEP, CapabilityHook, CapabilityDiagnostic:
		if w.Item == "" || len(w.Item) > 128 || !utf8.ValidString(w.Item) || strings.ContainsAny(w.Item, "\x00\r\n") {
			return errors.New("approval requires a bounded item identity")
		}
	case CapabilityShell, CapabilityWeb, CapabilityBrowser, CapabilityDecoder:
		if w.Item != "" {
			return errors.New("singleton capability approval has no item identity")
		}
	default:
		return errors.New("unknown host capability approval")
	}
	if len(w.Fingerprint) != 64 {
		return errors.New("approval requires an exact configuration fingerprint")
	}
	if _, err := hex.DecodeString(w.Fingerprint); err != nil {
		return errors.New("approval requires an exact configuration fingerprint")
	}
	return nil
}

// ResolveSettings is the only effective-settings rule. An overriding session
// keeps its snapshot and its own approvals. Otherwise application intent is
// used and each host item runs only with an exact workspace approval.
func ResolveSettings(session Settings, app AppSettings, approvals []WorkspaceApproval) Settings {
	if session.OverridesCapabilities() {
		return session
	}
	granted := make(map[string]string, len(approvals))
	for _, approval := range approvals {
		granted[approval.Kind+"\x00"+approval.Item] = approval.Fingerprint
	}
	c := app.Capabilities.Intent()
	for _, item := range c.Items() {
		if item.Host {
			on := item.Enabled && item.Fingerprint != "" && granted[item.Key()] == item.Fingerprint
			_ = c.SetEnabled(item.Kind, item.Item, on, on)
		}
	}
	return session.WithCapabilities(c)
}

// CarryEnablement applies a configuration edit without changing any switch:
// existing items keep their state, new items start off, and with
// revokeChanged a host item whose configuration changed is switched off.
func CarryEnablement(current, next Capabilities, revokeChanged bool) Capabilities {
	previous := make(map[string]CapabilityItem)
	for _, item := range current.Items() {
		previous[item.Key()] = item
	}
	approved := hostApprovals(current)
	next = next.Clone()
	for _, item := range next.Items() {
		old, ok := previous[item.Key()]
		on := ok && old.Enabled
		if item.Host && revokeChanged && (!ok || old.Fingerprint != item.Fingerprint) {
			on = false
		}
		_ = next.SetEnabled(item.Kind, item.Item, on, on && approved[item.Key()])
	}
	return next
}

func hostApprovals(c Capabilities) map[string]bool {
	result := make(map[string]bool)
	for _, server := range c.MCP {
		result[CapabilityMCP+"\x00"+server.ID] = server.HostAccessApproved
	}
	for _, entry := range c.AEP {
		result[CapabilityAEP+"\x00"+entry.ID] = entry.HostAccessApproved
	}
	for _, hook := range c.Commands.Hooks {
		result[CapabilityHook+"\x00"+hook.ID] = hook.HostAccessApproved
	}
	for _, profile := range c.Commands.Diagnostics {
		result[CapabilityDiagnostic+"\x00"+profile.ID] = profile.HostAccessApproved
	}
	result[CapabilityShell+"\x00"] = c.Tools.Shell.HostAccessApproved
	result[CapabilityWeb+"\x00"] = c.Tools.Web.NetworkAccessApproved
	result[CapabilityBrowser+"\x00"] = c.Browser.HostAccessApproved
	result[CapabilityDecoder+"\x00"] = c.MediaDecoder.HostAccessApproved
	return result
}
