// INPUT: Hub scope/approval policy, optional AppSettingsStore and the product session store
// OUTPUT: SettingsService (redacted form model, section CAS saves, switches, workspace approvals, overrides, reset) and EffectiveSettings
// POS: [L7 Hub product] The only settings entry for UI; secrets are write-only and saving never starts a process
package product

import (
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"

	"github.com/QuincySx/Alva/apps/hub"
)

// SecretKept stands for a saved environment/header value the form never sees.
// It contains NUL, which no stored value may contain, so it cannot collide.
const SecretKept = "\x00saved-secret\x00"

// Settings sections that a form saves independently.
const (
	SectionMCP      = "mcp"
	SectionAEP      = "aep"
	SectionTools    = "tools"
	SectionCommands = "commands"
	SectionBrowser  = "browser"
	SectionMedia    = "media"
	// SectionSwitches is reset-only: it turns the plain capability switches off.
	SectionSwitches = "switches"
)

var settingsSections = []string{SectionMCP, SectionAEP, SectionTools, SectionCommands, SectionBrowser, SectionMedia}

var ErrSettingsConflict = errors.New("设置已在其他地方更改，草稿已保留；请重新载入后再保存。")

// ErrUsesApplicationSettings is the refusal to write session-only settings
// for a task that still follows the application settings.
var ErrUsesApplicationSettings = errors.New("这个任务使用应用设置；要单独设置，请先选择“仅本会话”。")

// ErrHostApprovalRequired refuses to enable a host item without an explicit
// approval in the same request (HTTP callers have no separate approval click).
var ErrHostApprovalRequired = errors.New("启用需要宿主访问的开关时，请在同一请求里明确批准宿主访问（host_access_approved）。")

// ErrUnknownCapability: no such switch in the chosen scope (a server saved in
// a session draft is only found with session scope).
var ErrUnknownCapability = errors.New("找不到这个开关：应用设置里没有它；保存在会话草稿里的项目请指定 scope 为 session。")

// ErrFingerprintRequired: approval is bound to the exact configuration the
// caller read; without its fingerprint nothing is approved.
var ErrFingerprintRequired = errors.New("启用需要宿主访问的开关时，请带上刚读取到的该项指纹（fingerprint），批准只对这份配置有效。")

// ErrNoWorkspaceToApprove: application-level approvals are per workspace.
var ErrNoWorkspaceToApprove = errors.New("这个任务没有保存的工作区，无法批准宿主访问。")

// SettingsScope selects application settings (App) or one session's own
// override. SessionID also names the context task whose workspace approvals
// are shown while editing application settings.
type SettingsScope struct {
	App       bool
	SessionID string
}

type SettingsItem struct {
	hub.CapabilityItem
	// Approved reports whether this item would run for the context task:
	// a workspace approval in application scope, the snapshot flag otherwise.
	Approved      bool `json:"approved"`
	NeedsApproval bool `json:"needs_approval"`
}

type SessionSettingsView struct {
	SkillsEnabled     bool                 `json:"skills_enabled"`
	DurableWaits      bool                 `json:"durable_waits"`
	PlanMode          bool                 `json:"plan_mode"`
	PermissionPolicy  hub.PermissionPolicy `json:"permission_policy"`
	CompatibilityFile string               `json:"compatibility_file,omitempty"`
	SkillArchives     []string             `json:"skill_archives,omitempty"`
}

// SettingsView is the redacted form model; secret values are SecretKept.
type SettingsView struct {
	App          bool                 `json:"app"`
	SessionID    string               `json:"session_id,omitempty"`
	SessionTitle string               `json:"session_title,omitempty"`
	Workspace    string               `json:"workspace,omitempty"`
	Override     bool                 `json:"override"`
	Idle         bool                 `json:"idle"`
	Revision     uint64               `json:"revision"`
	Capabilities hub.Capabilities     `json:"capabilities"`
	Fingerprints map[string]string    `json:"fingerprints"`
	Items        []SettingsItem       `json:"items"`
	Session      *SessionSettingsView `json:"session,omitempty"`
}

// Item returns one switch state; ok is false when the item does not exist.
func (v SettingsView) Item(kind, item string) (SettingsItem, bool) {
	for _, value := range v.Items {
		if value.Kind == kind && value.Item == item {
			return value, true
		}
	}
	return SettingsItem{}, false
}

// PendingApprovals counts enabled host items that will not run in the context
// workspace until approved there.
func (v SettingsView) PendingApprovals() int {
	n := 0
	for _, item := range v.Items {
		if item.NeedsApproval {
			n++
		}
	}
	return n
}

type FieldError struct {
	Field   string `json:"field"`
	Message string `json:"message"`
}

// FieldErrors is returned by Save when the form needs correction.
type FieldErrors []FieldError

func (e FieldErrors) Error() string {
	parts := make([]string, len(e))
	for i, item := range e {
		parts[i] = item.Message
	}
	return strings.Join(parts, "；")
}

// For reports the first message for one field.
func (e FieldErrors) For(field string) string {
	for _, item := range e {
		if item.Field == field {
			return item.Message
		}
	}
	return ""
}

type SettingsService struct {
	service *HubService
	store   hub.AppSettingsStore
	key     [32]byte
	mu      sync.Mutex
	mcp     mcpState
	// mcpConnector is installed by program assembly; nil disables checks.
	mcpConnector MCPConnector
}

func newSettingsService(service *HubService, store hub.AppSettingsStore) (*SettingsService, error) {
	s := &SettingsService{service: service, store: store}
	if _, err := rand.Read(s.key[:]); err != nil {
		return nil, err
	}
	return s, nil
}

// SessionSettings is the session-scoped snapshot: plan mode, permission
// policy, model options and the session's own switches. It is not the
// effective capability set (EffectiveSettings, or SettingsService.Load). It
// lives on HubService so entries stay usable where no SettingsService is
// installed.
func (s *HubService) SessionSettings(ctx context.Context, sessionID string) (hub.Settings, error) {
	return s.Store.Settings(ctx, sessionID)
}

// SetSkillsSwitch is the session-scoped skills switch (never inherited from
// application settings), usable where no SettingsService is installed.
func (s *HubService) SetSkillsSwitch(ctx context.Context, sessionID string, enabled bool) error {
	return s.Store.SetSkillsEnabled(ctx, sessionID, enabled)
}

// EffectiveSettings is the next-turn snapshot for one session: its own
// override, otherwise application intent filtered by exact workspace approvals.
// Without an installed SettingsService the session snapshot is used as is.
func (s *HubService) EffectiveSettings(ctx context.Context, id string) (hub.Settings, error) {
	settings, err := s.Store.Settings(ctx, id)
	if err != nil {
		return hub.Settings{}, err
	}
	if s.Settings == nil || settings.OverridesCapabilities() {
		return settings, nil
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return hub.Settings{}, err
	}
	value, err := s.Settings.store.AppSettings(ctx)
	if err != nil {
		return hub.Settings{}, err
	}
	var approvals []hub.WorkspaceApproval
	if detail.Session.Workspace != "" {
		if approvals, err = s.Settings.store.WorkspaceApprovals(ctx, detail.Session.Workspace); err != nil {
			return hub.Settings{}, err
		}
	}
	effective := hub.ResolveSettings(settings, value, approvals)
	if err = effective.Validate(); err != nil {
		return hub.Settings{}, err
	}
	return effective, nil
}

func (s *SettingsService) fingerprint(c hub.Capabilities, section string) string {
	quiet := c.Clone()
	for _, item := range quiet.Items() {
		_ = quiet.SetEnabled(item.Kind, item.Item, false, false)
	}
	data, _ := json.Marshal(sectionValue(quiet, section))
	mac := hmac.New(sha256.New, s.key[:])
	mac.Write([]byte(section))
	mac.Write([]byte{0})
	mac.Write(data)
	return hex.EncodeToString(mac.Sum(nil))
}

func knownSection(section string) bool {
	for _, known := range settingsSections {
		if known == section {
			return true
		}
	}
	return false
}

func sectionValue(c hub.Capabilities, section string) any {
	switch section {
	case SectionMCP:
		return c.MCP
	case SectionAEP:
		return c.AEP
	case SectionTools:
		return c.Tools
	case SectionCommands:
		return c.Commands
	case SectionBrowser:
		return c.Browser
	case SectionMedia:
		return c.MediaDecoder
	}
	return nil
}

func replaceSection(current, desired hub.Capabilities, section string) (hub.Capabilities, error) {
	next := current.Clone()
	desired = desired.Clone()
	switch section {
	case SectionMCP:
		next.MCP = desired.MCP
	case SectionAEP:
		next.AEP = desired.AEP
	case SectionTools:
		next.Tools = desired.Tools
	case SectionCommands:
		next.Commands = desired.Commands
	case SectionBrowser:
		next.Browser = desired.Browser
	case SectionMedia:
		next.MediaDecoder = desired.MediaDecoder
	default:
		return hub.Capabilities{}, errors.New("unknown settings section")
	}
	return next, nil
}

func redactValues(values map[string]string) map[string]string {
	if values == nil {
		return nil
	}
	result := make(map[string]string, len(values))
	for key := range values {
		result[key] = SecretKept
	}
	return result
}

func redactCapabilities(c hub.Capabilities) hub.Capabilities {
	c = c.Clone()
	for i := range c.MCP {
		c.MCP[i].Env, c.MCP[i].Headers = redactValues(c.MCP[i].Env), redactValues(c.MCP[i].Headers)
	}
	for i := range c.AEP {
		c.AEP[i].Env = redactValues(c.AEP[i].Env)
	}
	for i := range c.Commands.Hooks {
		c.Commands.Hooks[i].Env = redactValues(c.Commands.Hooks[i].Env)
	}
	for i := range c.Commands.Diagnostics {
		c.Commands.Diagnostics[i].Env = redactValues(c.Commands.Diagnostics[i].Env)
	}
	c.Tools.Shell.Env = redactValues(c.Tools.Shell.Env)
	c.Browser.Env = redactValues(c.Browser.Env)
	return c
}

// restoreValues replaces SecretKept with the value saved for the same item
// and key. A placeholder that no longer matches anything must be re-entered.
func restoreValues(field string, desired, saved map[string]string) (map[string]string, error) {
	if desired == nil {
		return nil, nil
	}
	result := make(map[string]string, len(desired))
	for key, value := range desired {
		if value == SecretKept {
			old, ok := saved[key]
			if !ok {
				return nil, FieldErrors{{Field: field, Message: fmt.Sprintf("“%s”的值需要重新输入。", key)}}
			}
			value = old
		}
		result[key] = value
	}
	return result, nil
}

func restoreSecrets(desired, current hub.Capabilities, section string) (hub.Capabilities, error) {
	desired = desired.Clone()
	var err error
	switch section {
	case SectionMCP:
		saved := map[string]hub.MCPServer{}
		for _, server := range current.MCP {
			saved[server.ID] = server
		}
		for i := range desired.MCP {
			old := saved[desired.MCP[i].ID]
			field := "mcp." + desired.MCP[i].ID
			if desired.MCP[i].Env, err = restoreValues(field+".env", desired.MCP[i].Env, old.Env); err != nil {
				return desired, err
			}
			if desired.MCP[i].Headers, err = restoreValues(field+".headers", desired.MCP[i].Headers, old.Headers); err != nil {
				return desired, err
			}
		}
	case SectionAEP:
		saved := map[string]hub.AEPBundle{}
		for _, entry := range current.AEP {
			saved[entry.ID] = entry
		}
		for i := range desired.AEP {
			if desired.AEP[i].Env, err = restoreValues("aep."+desired.AEP[i].ID+".env", desired.AEP[i].Env, saved[desired.AEP[i].ID].Env); err != nil {
				return desired, err
			}
		}
	case SectionCommands:
		hooks := map[string]hub.HookSpec{}
		for _, hook := range current.Commands.Hooks {
			hooks[hook.ID] = hook
		}
		for i := range desired.Commands.Hooks {
			if desired.Commands.Hooks[i].Env, err = restoreValues("hook."+desired.Commands.Hooks[i].ID+".env", desired.Commands.Hooks[i].Env, hooks[desired.Commands.Hooks[i].ID].Env); err != nil {
				return desired, err
			}
		}
		profiles := map[string]hub.DiagnosticSpec{}
		for _, profile := range current.Commands.Diagnostics {
			profiles[profile.ID] = profile
		}
		for i := range desired.Commands.Diagnostics {
			if desired.Commands.Diagnostics[i].Env, err = restoreValues("diagnostic."+desired.Commands.Diagnostics[i].ID+".env", desired.Commands.Diagnostics[i].Env, profiles[desired.Commands.Diagnostics[i].ID].Env); err != nil {
				return desired, err
			}
		}
	case SectionTools:
		desired.Tools.Shell.Env, err = restoreValues("tools.shell.env", desired.Tools.Shell.Env, current.Tools.Shell.Env)
	case SectionBrowser:
		desired.Browser.Env, err = restoreValues("browser.env", desired.Browser.Env, current.Browser.Env)
	}
	return desired, err
}

type settingsSource struct {
	app       hub.AppSettings
	session   hub.Settings
	detail    hub.Detail
	approvals []hub.WorkspaceApproval
}

func (s *SettingsService) source(ctx context.Context, scope SettingsScope) (settingsSource, error) {
	var source settingsSource
	var err error
	if source.app, err = s.store.AppSettings(ctx); err != nil {
		return source, err
	}
	if scope.SessionID == "" {
		if !scope.App {
			return source, errors.New("请先选择任务。")
		}
		return source, nil
	}
	if source.detail, err = s.service.Store.Detail(ctx, scope.SessionID); err != nil {
		return source, err
	}
	if source.session, err = s.service.Store.Settings(ctx, scope.SessionID); err != nil {
		return source, err
	}
	if workspace := source.detail.Session.Workspace; workspace != "" {
		if source.approvals, err = s.store.WorkspaceApprovals(ctx, workspace); err != nil {
			return source, err
		}
	}
	if !scope.App && !source.session.OverridesCapabilities() {
		return source, ErrUsesApplicationSettings
	}
	return source, nil
}

func (source settingsSource) capabilities(scope SettingsScope) hub.Capabilities {
	if scope.App {
		return source.app.Capabilities.Clone()
	}
	return source.session.Capabilities()
}

// Load returns the redacted form model of one scope.
func (s *SettingsService) Load(ctx context.Context, scope SettingsScope) (SettingsView, error) {
	source, err := s.source(ctx, scope)
	if err != nil {
		return SettingsView{}, err
	}
	return s.view(scope, source), nil
}

func (s *SettingsService) view(scope SettingsScope, source settingsSource) SettingsView {
	c := source.capabilities(scope)
	view := SettingsView{App: scope.App, SessionID: scope.SessionID, Revision: source.app.Revision, Idle: true,
		Capabilities: redactCapabilities(c), Fingerprints: make(map[string]string, len(settingsSections))}
	for _, section := range settingsSections {
		view.Fingerprints[section] = s.fingerprint(c, section)
	}
	granted := make(map[string]string, len(source.approvals))
	for _, approval := range source.approvals {
		granted[approval.Kind+"\x00"+approval.Item] = approval.Fingerprint
	}
	if scope.SessionID != "" {
		view.SessionTitle, view.Workspace = source.detail.Session.Title, source.detail.Session.Workspace
		view.Override = source.session.OverridesCapabilities()
		view.Idle = !hub.BlocksInput(source.detail.Session.Status)
		view.Session = &SessionSettingsView{SkillsEnabled: source.session.SkillsEnabled, DurableWaits: source.session.DurableWaits, PlanMode: source.session.PlanMode,
			PermissionPolicy: source.session.PermissionPolicy.Effective(), CompatibilityFile: source.session.CompatibilityFile, SkillArchives: append([]string(nil), source.session.SkillArchives...)}
	}
	approvedSession := hostApprovedSnapshot(c)
	for _, item := range c.Items() {
		entry := SettingsItem{CapabilityItem: item}
		switch {
		case !item.Host:
			entry.Approved = item.Enabled
		case scope.App:
			entry.Approved = item.Fingerprint != "" && granted[item.Key()] == item.Fingerprint
			entry.NeedsApproval = item.Enabled && !entry.Approved && view.Workspace != ""
		default:
			entry.Approved = item.Enabled && approvedSession[item.Key()]
		}
		view.Items = append(view.Items, entry)
	}
	return view
}

func hostApprovedSnapshot(c hub.Capabilities) map[string]bool {
	settings := hub.Settings{}.WithCapabilities(c)
	result := map[string]bool{}
	for _, server := range settings.MCP {
		result[hub.CapabilityMCP+"\x00"+server.ID] = server.HostAccessApproved
	}
	for _, entry := range settings.AEP {
		result[hub.CapabilityAEP+"\x00"+entry.ID] = entry.HostAccessApproved
	}
	for _, hook := range settings.Commands.Hooks {
		result[hub.CapabilityHook+"\x00"+hook.ID] = hook.HostAccessApproved
	}
	for _, profile := range settings.Commands.Diagnostics {
		result[hub.CapabilityDiagnostic+"\x00"+profile.ID] = profile.HostAccessApproved
	}
	result[hub.CapabilityShell+"\x00"] = settings.Tools.Shell.HostAccessApproved
	result[hub.CapabilityWeb+"\x00"] = settings.Tools.Web.NetworkAccessApproved
	result[hub.CapabilityBrowser+"\x00"] = settings.Browser.HostAccessApproved
	result[hub.CapabilityDecoder+"\x00"] = settings.MediaDecoder.HostAccessApproved
	return result
}

// Save replaces one section's configuration at the expected fingerprint.
// Switches are never changed by Save: new items start off, and in a session
// override a host item whose configuration changed is switched off.
func (s *SettingsService) Save(ctx context.Context, scope SettingsScope, section, expected string, desired hub.Capabilities) (SettingsView, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	source, err := s.source(ctx, scope)
	if err != nil {
		return SettingsView{}, err
	}
	current := source.capabilities(scope)
	if !knownSection(section) {
		return SettingsView{}, errors.New("unknown settings section")
	}
	if s.fingerprint(current, section) != expected {
		return SettingsView{}, ErrSettingsConflict
	}
	desired, err = restoreSecrets(desired, current, section)
	if err != nil {
		return SettingsView{}, err
	}
	next, err := replaceSection(current, desired, section)
	if err != nil {
		return SettingsView{}, err
	}
	if problems := fieldErrors(next, section); len(problems) != 0 {
		return SettingsView{}, problems
	}
	if section == SectionAEP {
		if err = s.service.inspectAEP(ctx, next.AEP); err != nil {
			return SettingsView{}, FieldErrors{{Field: "aep", Message: "无法检查扩展插件源目录：" + err.Error()}}
		}
	}
	if scope.App {
		next = hub.CarryEnablement(current, next, false)
		if err = next.ValidateIntent(); err != nil {
			return SettingsView{}, FieldErrors{{Field: section, Message: "配置无效：" + err.Error()}}
		}
		if _, err = s.store.SaveAppSettings(ctx, source.app.Revision, next); err != nil {
			if errors.Is(err, hub.ErrAppSettingsConflict) {
				return SettingsView{}, ErrSettingsConflict
			}
			return SettingsView{}, err
		}
	} else {
		check := source.session.WithCapabilities(hub.CarryEnablement(current, next, true))
		if err = check.Validate(); err != nil {
			return SettingsView{}, FieldErrors{{Field: section, Message: "配置无效：" + err.Error()}}
		}
		if err = s.store.ReplaceSessionCapabilities(ctx, scope.SessionID, next); err != nil {
			return SettingsView{}, err
		}
	}
	s.forgetProbes()
	return s.reload(ctx, scope)
}

func (s *SettingsService) reload(ctx context.Context, scope SettingsScope) (SettingsView, error) {
	source, err := s.source(ctx, scope)
	if err != nil {
		return SettingsView{}, err
	}
	return s.view(scope, source), nil
}

// SetEnabled changes one switch. In application scope a host item enabled
// while a task is selected is also approved for that task's workspace; the
// caller must have shown that confirmation. In a session override enabling
// a host item approves exactly its current snapshot.
func (s *SettingsService) SetEnabled(ctx context.Context, scope SettingsScope, kind, item string, enabled bool) (SettingsView, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	source, err := s.source(ctx, scope)
	if err != nil {
		return SettingsView{}, err
	}
	return s.setEnabledLocked(ctx, scope, source, kind, item, enabled, true, false)
}

// SetCapability is the explicit form of SetEnabled for callers that have no
// separate approval click (the HTTP API). Enabling a host item requires
// hostApproved and the item's current fingerprint (as read from Load) in the
// same call; a changed configuration is refused with ErrSettingsChanged, and in application scope that approval is
// recorded for the task's workspace; without it nothing is changed. Scope
// rules match SetEnabled: application scope by default, session scope only
// for a task that keeps its own settings.
func (s *SettingsService) SetCapability(ctx context.Context, scope SettingsScope, kind, item string, enabled, hostApproved bool, fingerprint string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	source, err := s.source(ctx, scope)
	if err != nil {
		return err
	}
	if enabled {
		for _, candidate := range source.capabilities(scope).Items() {
			if candidate.Kind != kind || candidate.Item != item || !candidate.Host {
				continue
			}
			if !hostApproved {
				return ErrHostApprovalRequired
			}
			if fingerprint == "" {
				return ErrFingerprintRequired
			}
			// Compared under the service lock, before anything is written.
			if candidate.Fingerprint != fingerprint {
				return ErrSettingsChanged
			}
		}
	}
	_, err = s.setEnabledLocked(ctx, scope, source, kind, item, enabled, hostApproved, true)
	if err != nil && err.Error() == "unknown capability switch" {
		return ErrUnknownCapability
	}
	return err
}

func (s *SettingsService) setEnabledLocked(ctx context.Context, scope SettingsScope, source settingsSource, kind, item string, enabled, approve, requireWorkspace bool) (SettingsView, error) {
	var err error
	if !scope.App {
		if err = s.store.SetSessionCapabilityEnabled(ctx, scope.SessionID, kind, item, enabled); err != nil {
			return SettingsView{}, enableError(err)
		}
		return s.reload(ctx, scope)
	}
	next := source.app.Capabilities.Clone()
	if err = next.SetEnabled(kind, item, enabled, false); err != nil {
		return SettingsView{}, err
	}
	if err = next.ValidateIntent(); err != nil {
		return SettingsView{}, enableError(err)
	}
	if enabled && approve && requireWorkspace && source.detail.Session.Workspace == "" && hostSwitch(next, kind, item) {
		return SettingsView{}, ErrNoWorkspaceToApprove
	}
	if _, err = s.store.SaveAppSettings(ctx, source.app.Revision, next); err != nil {
		if errors.Is(err, hub.ErrAppSettingsConflict) {
			return SettingsView{}, ErrSettingsConflict
		}
		return SettingsView{}, err
	}
	if enabled && approve && source.detail.Session.Workspace != "" && hostSwitch(next, kind, item) {
		if err = s.approveLocked(ctx, source.detail.Session.Workspace, next, kind, item, true, ""); err != nil {
			return SettingsView{}, err
		}
	}
	return s.reload(ctx, scope)
}

func hostSwitch(c hub.Capabilities, kind, item string) bool {
	for _, candidate := range c.Items() {
		if candidate.Kind == kind && candidate.Item == item {
			return candidate.Host
		}
	}
	return false
}

func enableError(err error) error {
	message := err.Error()
	switch {
	case strings.Contains(message, "web capability requires"):
		return FieldErrors{{Field: "tools.web", Message: "启用网页工具前，请打开“读取网页”或填写搜索端点。"}}
	case strings.Contains(message, "AEP enablement requires"):
		return FieldErrors{{Field: "aep", Message: "请先保存，让 Alva 检查插件源目录后再启用。"}}
	case strings.Contains(message, "absolute explicit shell executable"), strings.Contains(message, "explicit absolute command"):
		return FieldErrors{{Field: "tools.shell", Message: "请先填写并保存可执行文件的绝对路径。"}}
	case strings.Contains(message, "browser requires an explicit absolute executable"):
		return FieldErrors{{Field: "browser", Message: "请先填写并保存浏览器可执行文件的绝对路径。"}}
	case strings.Contains(message, "media decoder requires two"):
		return FieldErrors{{Field: "media", Message: "请先填写并保存 ffmpeg 和 ffprobe 的绝对路径。"}}
	}
	return err
}

func (s *SettingsService) approveLocked(ctx context.Context, workspace string, c hub.Capabilities, kind, item string, approve bool, fingerprint string) error {
	if !approve {
		return s.store.DeleteWorkspaceApproval(ctx, workspace, kind, item)
	}
	for _, candidate := range c.Items() {
		if candidate.Kind == kind && candidate.Item == item {
			if !candidate.Host {
				return errors.New("这个开关不需要工作区批准。")
			}
			if fingerprint != "" && candidate.Fingerprint != fingerprint {
				return ErrSettingsChanged
			}
			return s.store.SaveWorkspaceApproval(ctx, hub.WorkspaceApproval{Workspace: workspace, Kind: kind, Item: item, Fingerprint: candidate.Fingerprint})
		}
	}
	return errors.New("找不到要批准的项目，请重新载入。")
}

// Approve records or withdraws consent for an application-level host item in
// the selected task's workspace. Consent is bound to the current fingerprint.
func (s *SettingsService) Approve(ctx context.Context, sessionID, kind, item string, approve bool) (SettingsView, error) {
	return s.approve(ctx, sessionID, kind, item, approve, "")
}

// ErrSettingsChanged means the configuration differs from what the user confirmed.
var ErrSettingsChanged = errors.New("配置已变化，请重新确认。")

// ApproveExact approves only if the item's current fingerprint is the one the
// user confirmed; the comparison and the write happen under one lock.
func (s *SettingsService) ApproveExact(ctx context.Context, sessionID, kind, item, fingerprint string) (SettingsView, error) {
	if fingerprint == "" {
		return SettingsView{}, ErrSettingsChanged
	}
	return s.approve(ctx, sessionID, kind, item, true, fingerprint)
}

func (s *SettingsService) approve(ctx context.Context, sessionID, kind, item string, approve bool, fingerprint string) (SettingsView, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	scope := SettingsScope{App: true, SessionID: sessionID}
	source, err := s.source(ctx, scope)
	if err != nil {
		return SettingsView{}, err
	}
	if source.detail.Session.Workspace == "" {
		return SettingsView{}, errors.New("这个任务没有保存的工作区，无法批准。")
	}
	if err = s.approveLocked(ctx, source.detail.Session.Workspace, source.app.Capabilities, kind, item, approve, fingerprint); err != nil {
		return SettingsView{}, err
	}
	return s.reload(ctx, scope)
}

// SetOverride starts a session-only snapshot from what this session would
// run now, or returns it to application settings (discarding its own copy).
func (s *SettingsService) SetOverride(ctx context.Context, sessionID string, override bool) (SettingsView, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	var initial hub.Capabilities
	if override {
		effective, err := s.service.EffectiveSettings(ctx, sessionID)
		if err != nil {
			return SettingsView{}, err
		}
		initial = effective.Capabilities()
	}
	if err := s.store.SetCapabilityOverride(ctx, sessionID, override, initial); err != nil {
		return SettingsView{}, err
	}
	s.forgetProbes()
	return s.reload(ctx, SettingsScope{App: !override, SessionID: sessionID})
}

// Reset restores a section's defaults. List sections keep their entries and
// only switch every item off; other sections return to empty values.
func (s *SettingsService) Reset(ctx context.Context, scope SettingsScope, section string) (SettingsView, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	source, err := s.source(ctx, scope)
	if err != nil {
		return SettingsView{}, err
	}
	current := source.capabilities(scope)
	next := current.Clone()
	var off []hub.CapabilityItem
	for _, item := range current.Items() {
		belongs := false
		switch section {
		case SectionMCP:
			belongs = item.Kind == hub.CapabilityMCP
		case SectionAEP:
			belongs = item.Kind == hub.CapabilityAEP
		case SectionCommands:
			belongs = item.Kind == hub.CapabilityHook || item.Kind == hub.CapabilityDiagnostic
		case SectionTools:
			belongs = item.Kind == hub.CapabilityShell || item.Kind == hub.CapabilityWeb || item.Kind == hub.CapabilityPlanning || item.Kind == hub.CapabilityNotebook || item.Kind == hub.CapabilityInteraction || item.Kind == hub.CapabilityUtility
		case SectionBrowser:
			belongs = item.Kind == hub.CapabilityBrowser
		case SectionMedia:
			belongs = item.Kind == hub.CapabilityDecoder || item.Kind == hub.CapabilityMedia
		case SectionSwitches:
			belongs = item.Kind == hub.CapabilitySubagents || item.Kind == hub.CapabilityMemory
		default:
			return SettingsView{}, errors.New("unknown settings section")
		}
		if belongs && item.Enabled {
			off = append(off, item)
		}
		if belongs {
			_ = next.SetEnabled(item.Kind, item.Item, false, false)
		}
	}
	switch section {
	case SectionTools:
		next.Tools = hub.ToolSettings{}
	case SectionBrowser:
		next.Browser = hub.BrowserSettings{}
	case SectionMedia:
		next.MediaDecoder = hub.MediaDecoder{}
	}
	if scope.App {
		if _, err = s.store.SaveAppSettings(ctx, source.app.Revision, next); err != nil {
			if errors.Is(err, hub.ErrAppSettingsConflict) {
				return SettingsView{}, ErrSettingsConflict
			}
			return SettingsView{}, err
		}
	} else {
		for _, item := range off {
			if err = s.store.SetSessionCapabilityEnabled(ctx, scope.SessionID, item.Kind, item.Item, false); err != nil {
				return SettingsView{}, err
			}
		}
		if section == SectionTools || section == SectionBrowser || section == SectionMedia {
			if err = s.store.ReplaceSessionCapabilities(ctx, scope.SessionID, next); err != nil {
				return SettingsView{}, err
			}
		}
	}
	s.forgetProbes()
	return s.reload(ctx, scope)
}

// SetSessionSwitch changes a session-scoped switch that is never inherited.
func (s *SettingsService) SetSessionSwitch(ctx context.Context, sessionID, kind string, enabled bool) error {
	switch kind {
	case "skills":
		return s.service.SetSkillsSwitch(ctx, sessionID, enabled)
	case "durable_waits":
		return s.service.EnableDurableWaits(ctx, sessionID, enabled)
	}
	return errors.New("unknown session switch")
}
