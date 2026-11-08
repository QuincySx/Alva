// INPUT: Explicit native model favorites, shared defaults and session override drafts.
// OUTPUT: Scoped model saves, preserved conflicts, clear/reload and immutable next-Attempt options.
// POS: [L7 Hub UI] Saving never selects a route or starts a Turn; physical saves guard window closure.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type workspaceModelDefaultsState struct {
	open                                              bool
	target                                            hub.Target
	draft                                             hub.ModelPreference
	temperature, tokens, effort, turns, prompt, agent string
	problem                                           string
	sessionID, branchID, scope                        string
	overrides                                         hub.SessionModelOptionsSnapshot
	overridden                                        bool
	pendingEffort                                     string
	hasPendingEffort                                  bool
	ready                                             bool
}

func (p *NativePage) workspaceModelConfig(agent string) engine.ConfigSurface {
	if description, ok := p.Service.Turns.Engine(agent); ok {
		return description.Config
	}
	return engine.ConfigSurface{}
}

func (p *NativePage) toggleWorkspaceModelFavorite(target hub.Target) {
	s := &p.composer.modelPicker
	if s.saving || p.Service.Connections == nil {
		return
	}
	pref := p.Service.Connections.ModelPreference(target)
	pref.Favorite = !pref.Favorite
	p.saveWorkspaceModelPreference(target, pref, false)
}

func (p *NativePage) saveWorkspaceModelPreference(target hub.Target, pref hub.ModelPreference, defaults bool) {
	s := &p.composer.modelPicker
	if s.saving || p.Service.Connections == nil {
		return
	}
	s.saving = true
	sequence := s.sequence
	modalSession := s.defaults.sessionID
	go func() {
		saved, err := p.Service.Connections.SaveModelPreference(p.ctx, target, pref)
		p.dispatch(func() {
			s.saving = false
			if sequence != s.sequence {
				return
			}
			if defaults && (s.defaults.sessionID != modalSession || p.selected != modalSession) {
				return
			}
			if defaults {
				if err != nil {
					s.defaults.problem = err.Error()
					return
				}
				s.defaults.open = false
			}
			if err != nil {
				s.problem = err.Error()
				return
			}
			for i := range s.rows {
				if hub.ModelPreferenceKey(s.rows[i].routes[0]) == saved.Key {
					s.rows[i].favorite = saved.Favorite
				}
			}
			if s.provider == "favorites" {
				s.filter()
			}
		})
	}()
}

func (p *NativePage) openWorkspaceModelDefaults(target hub.Target) {
	p.openWorkspaceModelOptionScope(target, "模型默认")
}

func (p *NativePage) openWorkspaceModelOptionScope(target hub.Target, scope string) {
	s := &p.composer.modelPicker
	if s.saving || p.Service.Connections == nil {
		return
	}
	pref := p.Service.Connections.ModelPreference(target)
	v := workspaceModelDefaultsState{open: true, target: target, draft: pref, agent: pref.PreferredEngine, prompt: pref.Options.SystemPrompt, effort: pref.Options.Generation.ReasoningEffort}
	if v.agent == "" {
		v.agent = target.EngineID
	}
	v.scope, v.ready = "模型默认", true
	if p.selected != "" && p.detail.Session.ID == p.selected {
		v.sessionID, v.branchID = p.selected, p.detail.Session.ActiveBranchID
	}
	p.fillWorkspaceModelOptionFields(&v, pref.Options)
	s.defaults = v
	if scope == "当前会话" && v.sessionID != "" {
		s.defaults.scope = scope
		s.defaults.agent = target.EngineID
		s.defaults.ready = false
		p.reloadWorkspaceSessionModelOptions()
	}
}

func (p *NativePage) fillWorkspaceModelOptionFields(v *workspaceModelDefaultsState, options hub.ModelOptions) {
	v.temperature, v.tokens, v.turns = "", "", ""
	v.prompt, v.effort = options.SystemPrompt, options.Generation.ReasoningEffort
	if x := options.Generation.Temperature; x != nil {
		v.temperature = strconv.FormatFloat(*x, 'f', -1, 64)
	}
	if x := options.Generation.MaxTokens; x != nil {
		v.tokens = strconv.FormatUint(uint64(*x), 10)
	}
	if options.MaxTurns != 0 {
		v.turns = strconv.Itoa(options.MaxTurns)
	}
}

func (p *NativePage) reloadWorkspaceSessionModelOptions() {
	s := &p.composer.modelPicker
	if s.saving || s.defaults.sessionID == "" {
		return
	}
	s.saving = true
	s.defaults.ready = false
	id, target, branch := s.defaults.sessionID, s.defaults.target, s.defaults.branchID
	generation, optionsEpoch := p.generation, p.sessionModelOptions.epoch
	go func() {
		err := p.Service.Connections.ReloadModelPreferences(p.ctx)
		var value hub.SessionModelOptionsSnapshot
		if err == nil {
			value, err = p.Service.SessionModelOptions(p.ctx, id)
		}
		p.dispatch(func() {
			s.saving = false
			v := &s.defaults
			if v.sessionID != id || p.selected != id || p.generation != generation || p.detail.Session.ActiveBranchID != branch || v.branchID != branch || hub.RouteKey(v.target) != hub.RouteKey(target) {
				return
			}
			if err != nil {
				p.failWorkspaceSessionModelOptions(id, generation, optionsEpoch, err.Error())
				v.problem = err.Error()
				return
			}
			if p.acceptWorkspaceSessionModelOptions(id, generation, branch, optionsEpoch, id, value) {
				p.sessionModelOptions.epoch++
			} else if p.workspaceSessionModelOptionsProblem() != "" {
				v.problem = "会话参数观察已更改，请重新加载。"
				return
			}
			// A newer poll may have arrived while this explicit reload was
			// reading. Show its committed value rather than the older response.
			value = p.sessionModelOptions.value.Clone()
			v.overrides, v.problem, v.ready = value, "", true
			options, found := value.Options(target)
			v.overridden = found
			if !found {
				options = p.Service.Connections.ModelPreference(target).Options
			}
			p.fillWorkspaceModelOptionFields(v, options)
			if v.hasPendingEffort {
				v.effort, v.hasPendingEffort = v.pendingEffort, false
			}
		})
	}()
}

func (p *NativePage) saveWorkspaceSessionModelOptions(clear bool) {
	s := &p.composer.modelPicker
	v := &s.defaults
	if s.saving || v.sessionID == "" || p.selected != v.sessionID {
		return
	}
	if !v.ready {
		v.problem = "请重新加载会话参数后再保存。"
		return
	}
	pref, err := v.preference()
	if err != nil && !clear {
		v.problem = err.Error()
		return
	}
	edit := hub.SessionModelOptionsUpdate{SessionID: v.sessionID, BranchID: v.branchID, Target: v.target, ExpectedRevision: v.overrides.Revision}
	if !clear {
		options := pref.Options.Clone()
		edit.Options = &options
	}
	s.saving = true
	generation, optionsEpoch := p.generation, p.sessionModelOptions.epoch
	go func() {
		value, err := p.Service.SaveSessionModelOptions(p.ctx, edit)
		p.dispatch(func() {
			s.saving = false
			if s.defaults.sessionID != edit.SessionID || p.selected != edit.SessionID || p.generation != generation || p.detail.Session.ActiveBranchID != edit.BranchID || s.defaults.branchID != edit.BranchID || hub.RouteKey(s.defaults.target) != hub.RouteKey(edit.Target) {
				return
			}
			if err != nil {
				p.failWorkspaceSessionModelOptions(edit.SessionID, generation, optionsEpoch, err.Error())
				s.defaults.problem = err.Error()
				return
			}
			if p.acceptWorkspaceSessionModelOptions(edit.SessionID, generation, edit.BranchID, optionsEpoch, edit.SessionID, value) {
				p.sessionModelOptions.epoch++
			}
			s.inlineOptions.effortDirty = false
			s.defaults.open = false
		})
	}()
}

func (p *NativePage) reloadWorkspaceModelPreferences(defaults bool) {
	s := &p.composer.modelPicker
	if s.saving || p.Service.Connections == nil {
		return
	}
	s.saving = true
	target := s.defaults.target
	sequence := s.sequence
	modalSession := s.defaults.sessionID
	go func() {
		err := p.Service.Connections.ReloadModelPreferences(p.ctx)
		p.dispatch(func() {
			s.saving = false
			if sequence != s.sequence || defaults && (s.defaults.sessionID != modalSession || p.selected != modalSession) {
				return
			}
			if defaults {
				if err != nil {
					s.defaults.problem = err.Error()
					return
				}
				p.openWorkspaceModelDefaults(target)
			}
			if err != nil {
				s.problem = err.Error()
				return
			}
			s.problem = ""
			for i := range s.rows {
				s.rows[i].favorite = p.Service.Connections.ModelPreference(s.rows[i].routes[0]).Favorite
			}
			if s.provider == "favorites" {
				s.filter()
			}
		})
	}()
}

func (v *workspaceModelDefaultsState) preference() (hub.ModelPreference, error) {
	pref := v.draft.Clone()
	pref.PreferredEngine = v.agent
	pref.Options = hub.ModelOptions{SystemPrompt: v.prompt}
	if text := strings.TrimSpace(v.temperature); text != "" {
		n, err := strconv.ParseFloat(text, 64)
		if err != nil {
			return pref, fmt.Errorf("温度需要有效数字")
		}
		pref.Options.Generation.Temperature = &n
	}
	if text := strings.TrimSpace(v.tokens); text != "" {
		n, err := strconv.ParseUint(text, 10, 32)
		if err != nil || n == 0 {
			return pref, fmt.Errorf("最大输出需要正整数")
		}
		x := uint32(n)
		pref.Options.Generation.MaxTokens = &x
	}
	if text := strings.TrimSpace(v.turns); text != "" {
		n, err := strconv.Atoi(text)
		if err != nil {
			return pref, fmt.Errorf("迭代上限需要整数")
		}
		pref.Options.MaxTurns = n
	}
	pref.Options.Generation.ReasoningEffort = v.effort
	return pref, pref.Validate()
}

func (p *NativePage) buildWorkspaceModelDefaults(c *ui.Context) {
	s := &p.composer.modelPicker
	v := &s.defaults
	if !v.open {
		return
	}
	if v.sessionID != "" && p.selected != v.sessionID {
		v.open = false
		return
	}
	_, height := c.Size()
	modal := ui.DialogBase(c.Key("model-defaults"), &v.open, func(backdrop, panel ui.Element) {
		backdrop.Background(ui.RGBA(0, 0, 0, .35))
		panel.Width(480).MaxWidth(480).Height(min(600, height-32)).Padding(24).Gap(16).Radius(16).Background(c.Theme().Surface)
		ui.Text(c, v.target.ModelID).FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5).SingleLine().FillWidth()
		ui.Row(c.Key("scope")).FillWidth().Gap(8).Children(func() {
			for _, scope := range []string{"当前会话", "模型默认"} {
				label := "Model options scope: " + scope
				workspaceControlButton(c, scope, scope, "secondary", s.saving || scope == "当前会话" && v.sessionID == "").Label(label).Checked(v.scope == scope).OnClick(func() { p.openWorkspaceModelOptionScope(v.target, scope) })
			}
		})
		ui.Scroll(c.Key("model-defaults-fields")).FillWidth().Grow(1).MinHeight(0).Children(func() {
			if v.scope == "当前会话" {
				scope := "当前会话 · 此连接与模型 · 下一次新 Turn 生效；运行与等待恢复沿用已冻结参数"
				if !v.ready {
					scope += " · 会话覆盖尚未读取，重新加载后可保存"
				} else if !v.overridden {
					scope += " · 目前沿用模型默认"
				}
				ui.Text(c, scope).Label("Session model options scope").FillWidth().FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			} else {
				ui.Text(c, "模型默认 · 此连接与模型的所有未覆盖会话 · 下一次新 Turn 生效").Label("Default model options scope").FillWidth().FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			}
			agentTitle := "默认执行 Agent"
			if v.scope == "当前会话" {
				agentTitle = "执行 Agent（所选路由）"
			}
			ui.Text(c, agentTitle).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			agents := []string{}
			for _, target := range p.Service.AvailableTargets() {
				if hub.ModelPreferenceKey(target) == v.draft.Key {
					found := false
					for _, id := range agents {
						found = found || id == target.EngineID
					}
					if !found {
						agents = append(agents, target.EngineID)
					}
				}
			}
			workspaceControlSelect(c, "default-agent", &v.agent, agents).Label("Default model Agent").FillWidth().Disabled(s.saving || v.scope == "当前会话")
			field := func(key, label string, value *string, enabled bool) {
				ui.Text(c, label).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Padding(8, 0, 4, 0)
				workspaceControlInput(c, key, value).Label("Model default " + key).Placeholder("留空沿用 Agent").FillWidth().Disabled(s.saving || !enabled)
			}
			config := p.workspaceModelConfig(v.agent)
			field("temperature", "温度", &v.temperature, config.SupportsGeneration)
			field("max-tokens", "最大输出 Tokens", &v.tokens, config.SupportsGeneration)
			ui.Text(c, "推理强度").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Padding(8, 0, 4, 0)
			effort := v.effort
			if effort == "" {
				effort = "沿用 Agent"
			}
			workspaceControlSelect(c, "reasoning", &effort, []string{"沿用 Agent", "none", "minimal", "low", "medium", "high", "xhigh", "max"}).Label("Model reasoning effort").FillWidth().Disabled(s.saving || !config.SupportsGeneration).OnChange(func() {
				v.effort = effort
				if effort == "沿用 Agent" {
					v.effort = ""
				}
			})
			field("max-turns", "迭代上限", &v.turns, config.SupportsMaxTurns)
			ui.Text(c, "系统提示词").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Padding(8, 0, 4, 0)
			workspaceControlTextArea(c, "system-prompt", &v.prompt).Label("Model system prompt").FillWidth().Height(96).Disabled(s.saving || !config.SupportsSystemPrompt)
			if v.problem != "" {
				ui.Text(c, v.problem).Label("Model defaults error").FontSize(workspaceFontCaption).TextColor(c.Theme().Danger).FillWidth()
			}
		})
		ui.Row(c).FillWidth().Justify(ui.End).Gap(8).Shrink(0).Children(func() {
			if v.problem != "" {
				workspaceControlButton(c, "reload-model-defaults", "重新加载", "secondary", s.saving).Label("Reload model defaults").OnClick(func() {
					if v.scope == "当前会话" {
						p.reloadWorkspaceSessionModelOptions()
					} else {
						p.reloadWorkspaceModelPreferences(true)
					}
				})
			}
			if v.scope == "当前会话" {
				workspaceControlButton(c, "clear-session-model-options", "清除覆盖", "secondary", s.saving || !v.ready).Label("Clear session model options").OnClick(func() { p.saveWorkspaceSessionModelOptions(true) })
			} else {
				workspaceControlButton(c, "reset-model-defaults", "重置", "secondary", s.saving).Label("Reset model defaults").OnClick(func() {
					v.temperature, v.tokens, v.effort, v.turns, v.prompt = "", "", "", "", ""
				})
			}
			workspaceControlButton(c, "cancel-model-defaults", "取消", "secondary", s.saving).Label("Cancel model defaults").OnClick(func() { v.open = false })
			workspaceControlButton(c, "save-model-defaults", "保存", "primary", s.saving || v.scope == "当前会话" && !v.ready).Label("Save model defaults").OnClick(func() {
				if v.scope == "当前会话" {
					p.saveWorkspaceSessionModelOptions(false)
					return
				}
				pref, err := v.preference()
				if err != nil {
					v.problem = err.Error()
					return
				}
				target := v.target
				valid := false
				for _, route := range p.Service.AvailableTargets() {
					if hub.ModelPreferenceKey(route) == v.draft.Key && route.EngineID == v.agent {
						target, valid = route, true
						break
					}
				}
				if !valid {
					v.problem = "这条模型路由已更改，请重新打开配置。"
					return
				}
				p.saveWorkspaceModelPreference(target, pref, true)
			})
		})
	})
	if modal.Dismissed() && s.saving {
		v.open = true
	}
}
