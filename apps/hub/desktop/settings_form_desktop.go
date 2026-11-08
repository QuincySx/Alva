// INPUT: Shared product SettingsService, the selected task and native MyGo controls
// OUTPUT: One settings form owner: scope bar, cold loads, section saves, switches, approvals, reset and shared field controls
// POS: [L7 Hub UI] Form drafts belong to the window; late results never reach a different pane, task or scope
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

type settingsEnvRow struct {
	name, value  string
	kept, reveal bool
}

// settingsEditor holds the text drafts of one form: a single-section page or
// the one list item being edited. Pointers keep control bindings stable.
type settingsEditor struct {
	kind  string
	index int
	text  map[string]*string
	flags map[string]*bool
	lists map[string]*[]string
	envs  map[string]*[]settingsEnvRow
}

func newSettingsEditor(kind string, index int) *settingsEditor {
	return &settingsEditor{kind: kind, index: index, text: map[string]*string{}, flags: map[string]*bool{}, lists: map[string]*[]string{}, envs: map[string]*[]settingsEnvRow{}}
}

func (e *settingsEditor) str(key string) *string {
	if e.text[key] == nil {
		e.text[key] = new(string)
	}
	return e.text[key]
}

func (e *settingsEditor) flag(key string) *bool {
	if e.flags[key] == nil {
		e.flags[key] = new(bool)
	}
	return e.flags[key]
}

func (e *settingsEditor) list(key string) *[]string {
	if e.lists[key] == nil {
		e.lists[key] = &[]string{}
	}
	return e.lists[key]
}

func (e *settingsEditor) env(key string) *[]settingsEnvRow {
	if e.envs[key] == nil {
		e.envs[key] = &[]settingsEnvRow{}
	}
	return e.envs[key]
}

func (e *settingsEditor) setNumber(key string, value int) {
	if value == 0 {
		*e.str(key) = ""
		return
	}
	*e.str(key) = strconv.Itoa(value)
}

// number parses an optional non-negative integer; empty means 0 (default).
func (e *settingsEditor) number(key, field string, problems *product.FieldErrors) int {
	raw := strings.TrimSpace(*e.str(key))
	if raw == "" {
		return 0
	}
	n, err := strconv.Atoi(raw)
	if err != nil || n < 0 {
		*problems = append(*problems, product.FieldError{Field: field, Message: "请输入非负整数（毫秒），留空使用默认值。"})
		return 0
	}
	return n
}

func (e *settingsEditor) setEnv(key string, values map[string]string) {
	rows := e.env(key)
	*rows = nil
	for _, name := range sortedKeys(values) {
		value := values[name]
		row := settingsEnvRow{name: name}
		if value == product.SecretKept {
			row.kept = true
		} else {
			row.value = value
		}
		*rows = append(*rows, row)
	}
}

// environment rebuilds a map; a kept row left empty keeps its saved value.
func (e *settingsEditor) environment(key, field string, problems *product.FieldErrors) map[string]string {
	rows := *e.env(key)
	if len(rows) == 0 {
		return nil
	}
	result := make(map[string]string, len(rows))
	for _, row := range rows {
		name := strings.TrimSpace(row.name)
		if name == "" && row.value == "" && !row.kept {
			continue
		}
		if name == "" {
			*problems = append(*problems, product.FieldError{Field: field, Message: "名称不能为空。"})
			continue
		}
		if _, exists := result[name]; exists {
			*problems = append(*problems, product.FieldError{Field: field, Message: "“" + name + "”重复。"})
			continue
		}
		value := row.value
		if row.kept && value == "" {
			value = product.SecretKept
		}
		result[name] = value
	}
	return result
}

func (e *settingsEditor) setList(key string, values []string) {
	*e.list(key) = append([]string(nil), values...)
}

func (e *settingsEditor) values(key string) []string {
	var result []string
	for _, value := range *e.list(key) {
		if strings.TrimSpace(value) != "" {
			result = append(result, value)
		}
	}
	return result
}

func sortedKeys(values map[string]string) []string {
	keys := make([]string, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	for i := 1; i < len(keys); i++ {
		for j := i; j > 0 && keys[j] < keys[j-1]; j-- {
			keys[j], keys[j-1] = keys[j-1], keys[j]
		}
	}
	return keys
}

type settingsConfirm struct {
	key, message, action string
	danger               bool
	run                  func()
}

type settingsFormState struct {
	pane, session  string
	generation     uint64
	sessionOnly    bool
	scope          product.SettingsScope
	view           product.SettingsView
	loaded         bool
	loading        bool
	saving         bool
	epoch          uint64
	problem        string
	notice         string
	errors         product.FieldErrors
	editor         *settingsEditor
	confirm        *settingsConfirm
	busyItem       string
	mcp            settingsMCPState
	resetEditorFor func(*NativePage)
}

// settingsFormScope edits application settings unless the user explicitly
// chose this task's own override. The selected task is the approval context.
func (p *NativePage) settingsFormScope() product.SettingsScope {
	if p.selected != "" && p.settingsForm.sessionOnly {
		return product.SettingsScope{SessionID: p.selected}
	}
	return product.SettingsScope{App: true, SessionID: p.selected}
}

// ensureSettingsForm (re)loads the form when its pane, task, generation or
// scope changed. A different owner's draft is never carried over.
func (p *NativePage) ensureSettingsForm(init func(*NativePage)) {
	f := &p.settingsForm
	scope := p.settingsFormScope()
	if f.pane == p.pane && f.session == p.selected && f.generation == p.generation && f.scope == scope && (f.loaded || f.loading || f.problem != "") {
		return
	}
	sessionOnly := f.sessionOnly && p.selected != "" && f.session == p.selected
	if f.mcp.cancel != nil {
		f.mcp.cancel() // Leaving the form ends a pending browser authorization.
	}
	*f = settingsFormState{pane: p.pane, session: p.selected, generation: p.generation, sessionOnly: sessionOnly, scope: scope, epoch: f.epoch + 1, resetEditorFor: init}
	p.loadSettingsForm()
}

func (p *NativePage) loadSettingsForm() {
	f := &p.settingsForm
	if p.Service.Settings == nil {
		f.problem = "当前程序没有提供设置服务。"
		return
	}
	f.loading = true
	epoch, pane, session, generation, scope := f.epoch, f.pane, f.session, f.generation, f.scope
	go func() {
		view, err := p.Service.Settings.Load(p.ctx, scope)
		p.dispatch(func() {
			if f.epoch != epoch || p.pane != pane || p.selected != session || p.generation != generation {
				return
			}
			f.loading = false
			if err != nil {
				// A task whose override was removed elsewhere falls back to application scope.
				if !scope.App && f.sessionOnly {
					f.sessionOnly = false
				}
				f.problem = err.Error()
				return
			}
			p.acceptSettingsView(view)
		})
	}()
}

func (p *NativePage) acceptSettingsView(view product.SettingsView) {
	f := &p.settingsForm
	f.view, f.loaded = view, true
	f.scope = product.SettingsScope{App: view.App, SessionID: view.SessionID}
	if view.SessionID != "" && !view.App {
		f.sessionOnly = true
	}
	if f.resetEditorFor != nil {
		f.resetEditorFor(p)
	}
}

// runSettings performs one service call. The result is published only to the
// same pane, task, generation and form epoch that issued it.
func (p *NativePage) runSettings(notice string, keepEditor bool, action func(context.Context, product.SettingsScope) (product.SettingsView, error)) {
	f := &p.settingsForm
	if f.saving || f.loading || p.Service.Settings == nil {
		return
	}
	f.saving, f.problem, f.notice, f.errors, f.confirm = true, "", "", nil, nil
	f.epoch++
	epoch, pane, session, generation, scope := f.epoch, f.pane, f.session, f.generation, f.scope
	p.paneBusy = true
	go func() {
		view, err := action(p.ctx, scope)
		p.dispatch(func() {
			p.paneBusy = false
			if f.epoch != epoch || p.pane != pane || p.selected != session || p.generation != generation {
				return
			}
			f.saving, f.busyItem = false, ""
			var problems product.FieldErrors
			switch {
			case errors.As(err, &problems):
				f.errors = problems
				return
			case err != nil:
				f.problem = err.Error()
				return
			}
			editor := f.editor
			p.acceptSettingsView(view)
			if keepEditor {
				f.editor = editor
			}
			f.notice = notice
		})
	}()
}

func (p *NativePage) saveSettingsSection(section string, desired hub.Capabilities, keepEditor bool) {
	expected := p.settingsForm.view.Fingerprints[section]
	p.runSettings("已保存，下一轮生效。", keepEditor, func(ctx context.Context, scope product.SettingsScope) (product.SettingsView, error) {
		return p.Service.Settings.Save(ctx, scope, section, expected, desired)
	})
}

// toggleSettingsItem changes one switch. Enabling a host item asks first:
// in application scope it also approves the selected task's workspace.
func (p *NativePage) toggleSettingsItem(kind, item, label string, enabled bool) {
	f := &p.settingsForm
	current, _ := f.view.Item(kind, item)
	run := func() {
		f.busyItem = kind + "\x00" + item
		p.runSettings("", true, func(ctx context.Context, scope product.SettingsScope) (product.SettingsView, error) {
			return p.Service.Settings.SetEnabled(ctx, scope, kind, item, enabled)
		})
	}
	if !enabled || !current.Host {
		run()
		return
	}
	message := "启用后，" + label + " 会以你的权限访问本机或网络。"
	switch {
	case !f.view.App:
		message += "确认只对这个任务启用当前配置。"
	case f.view.Workspace != "":
		message += "确认在工作区 " + f.view.Workspace + " 中允许它运行；其他工作区第一次使用时仍要单独批准。"
	default:
		message += "现在没有选择任务，它会在每个工作区第一次使用前请求批准。"
	}
	f.confirm = &settingsConfirm{key: "enable-" + kind + "-" + item, message: message, action: "确认启用", run: run}
}

func (p *NativePage) approveSettingsItem(kind, item, label string, approve bool) {
	f := &p.settingsForm
	run := func() {
		f.busyItem = kind + "\x00" + item
		session := f.view.SessionID
		p.runSettings("", true, func(ctx context.Context, _ product.SettingsScope) (product.SettingsView, error) {
			return p.Service.Settings.Approve(ctx, session, kind, item, approve)
		})
	}
	if !approve {
		run()
		return
	}
	f.confirm = &settingsConfirm{key: "approve-" + kind + "-" + item, message: "允许 " + label + " 在工作区 " + f.view.Workspace + " 中运行当前配置？配置改变后需要重新批准。", action: "批准", run: run}
}

func (p *NativePage) resetSettingsSection(section, description string) {
	f := &p.settingsForm
	f.confirm = &settingsConfirm{key: "reset-" + section, message: description, action: "恢复默认", danger: true, run: func() {
		p.runSettings("已恢复默认。", false, func(ctx context.Context, scope product.SettingsScope) (product.SettingsView, error) {
			return p.Service.Settings.Reset(ctx, scope, section)
		})
	}}
}

func (p *NativePage) setSettingsOverride(override bool) {
	f := &p.settingsForm
	session := f.view.SessionID
	message := "为这个任务单独保存一份插件与工具设置？会复制它现在实际会用到的配置；之后修改应用设置不再影响它。"
	action := "仅本会话"
	if !override {
		message, action = "让这个任务改回使用应用设置？它自己的那份设置会被丢弃。", "改回应用设置"
	}
	f.confirm = &settingsConfirm{key: "override", message: message, action: action, danger: !override, run: func() {
		f.sessionOnly = override
		p.runSettings("", false, func(ctx context.Context, _ product.SettingsScope) (product.SettingsView, error) {
			return p.Service.Settings.SetOverride(ctx, session, override)
		})
	}}
}

// buildSettingsScope shows what is being edited and, for the selected task,
// whether it uses application settings or its own override.
func (p *NativePage) buildSettingsScope(c *ui.Context) {
	f := &p.settingsForm
	t := c.Theme()
	v := f.view
	ui.Column(c.Key("settings-scope-bar")).Label("Settings scope").FillWidth().Gap(4).Padding(12).Radius(8).Background(workspacePalette(c).subtle).Children(func() {
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			title := "正在编辑：应用设置（所有会话）"
			if !v.App {
				title = "正在编辑：仅本会话 · " + v.SessionTitle
			}
			ui.Text(c, title).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).Grow(1).MinWidth(0)
			busy := f.saving || f.loading || !f.loaded
			switch {
			case v.SessionID == "":
			case !v.App:
				workspaceControlButton(c, "settings-scope-app", "编辑应用设置", "ghost", busy).Label("Edit application settings").OnClick(func() { f.sessionOnly = false })
				workspaceControlButton(c, "settings-scope-revert", "改回应用设置", "secondary", busy || !v.Idle).Label("Return task to application settings").OnClick(func() { p.setSettingsOverride(false) })
			case v.Override:
				workspaceControlButton(c, "settings-scope-session", "编辑本会话设置", "secondary", busy).Label("Edit this task's own settings").OnClick(func() { f.sessionOnly = true })
			default:
				workspaceControlButton(c, "settings-scope-override", "仅本会话…", "ghost", busy || !v.Idle).Label("Use session-only settings for this task").OnClick(func() { p.setSettingsOverride(true) })
			}
		})
		var line string
		switch {
		case v.SessionID == "":
			line = "下一轮对话开始时生效。启用会运行本机命令或访问网络的项目时，每个工作区第一次使用前都要批准。"
		case !v.App:
			line = "这个任务使用自己的设置，修改只影响它，下一轮生效。"
		case v.Override:
			line = "当前任务“" + v.SessionTitle + "”使用自己的设置，这里的修改不会影响它。"
		default:
			line = "当前任务“" + v.SessionTitle + "”使用应用设置。"
			if n := v.PendingApprovals(); n > 0 {
				line += "有 " + strconv.Itoa(n) + " 项需要在它的工作区批准后才会运行。"
			}
		}
		ui.Text(c, line).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted).Wrap()
		if v.SessionID != "" && !v.Idle {
			ui.Text(c, "任务正在运行；本轮已固定的设置不会改变。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Warning).Wrap()
		}
	})
}

// buildSettingsStatus renders the confirmation, problem and save notice rows.
func (p *NativePage) buildSettingsStatus(c *ui.Context) {
	f := &p.settingsForm
	t := c.Theme()
	if f.loading && !f.loaded {
		ui.Text(c, "正在读取设置…").Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(t.TextMuted)
	}
	if f.confirm != nil {
		confirm := f.confirm
		ui.Column(c.Key("settings-confirm")).Label("Settings confirmation").FillWidth().Gap(8).Padding(12).Radius(8).Border(1, t.Border).Children(func() {
			ui.Text(c, confirm.message).FontSize(workspaceFontBody).LineHeight(1.5).Wrap()
			ui.Row(c).Gap(8).Children(func() {
				kind := "primary"
				if confirm.danger {
					kind = "danger"
				}
				workspaceControlButton(c, "settings-confirm-run", confirm.action, kind, f.saving).Label("Confirm " + confirm.key).OnClick(confirm.run)
				workspaceControlButton(c, "settings-confirm-cancel", "取消", "secondary").Label("Cancel confirmation").OnClick(func() { f.confirm = nil })
			})
		})
	}
	if f.problem != "" {
		ui.Text(c, f.problem).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.Danger).Wrap()
	}
	if len(f.errors) > 0 {
		ui.Text(c, "请先修正标红的字段。").Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.Danger)
		for _, problem := range f.errors {
			if !strings.Contains(problem.Field, ".") {
				ui.Text(c, problem.Message).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Danger).Wrap()
			}
		}
	}
	if f.notice != "" {
		ui.Text(c, f.notice).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.Success)
	}
}

func settingsPageTitle(c *ui.Context, title, description string) {
	ui.Text(c, title).FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5)
	if description != "" {
		ui.Text(c, description).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted).Wrap()
	}
}

func settingsGroup(c *ui.Context, key, title, description string, children func()) {
	ui.Column(c.Key(key)).Label(title).FillWidth().Gap(8).Children(func() {
		ui.Text(c, title).FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5)
		if description != "" {
			ui.Text(c, description).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).Wrap()
		}
		children()
	})
}

// settingsSwitchRow is a SettingRow with a switch bound to one capability item.
func (p *NativePage) settingsSwitchRow(c *ui.Context, kind, item, title, description string) {
	f := &p.settingsForm
	state, ok := f.view.Item(kind, item)
	enabled := state.Enabled
	ui.Row(c.Key("switch-"+kind+"-"+item)).FillWidth().AlignItems(ui.Center).Padding(8, 0).Gap(12).Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
			ui.Text(c, title).FontSize(workspaceFontBody).LineHeight(1.5)
			if description != "" {
				ui.Text(c, description).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).Wrap()
			}
			p.settingsApprovalLine(c, state, title)
		})
		value := enabled
		busy := f.saving && f.busyItem == kind+"\x00"+item
		label := "Toggle " + kind
		if item != "" {
			label += " " + item
		}
		workspaceControlSwitch(c, "toggle-"+kind+"-"+item, &value, !ok || f.saving || f.loading || !f.view.Idle && !f.view.App).Label(label).Tooltip(title).OnChange(func() {
			p.toggleSettingsItem(kind, item, title, value)
		})
		if busy {
			p.workspaceLoadingIcon(c, workspaceIconSmall, c.Theme().TextMuted)
		}
	})
}

// settingsApprovalLine explains whether an enabled host item runs for the
// selected task and offers the explicit per-workspace approval.
func (p *NativePage) settingsApprovalLine(c *ui.Context, state product.SettingsItem, label string) {
	f := &p.settingsForm
	t := c.Theme()
	if !state.Host || !state.Enabled || !f.view.App || f.view.SessionID == "" {
		return
	}
	if f.view.Override {
		ui.Text(c, "当前任务使用自己的设置，不受这里影响。").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
		return
	}
	ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
		if state.Approved {
			ui.Text(c, "已在当前任务的工作区批准").FontSize(workspaceFontCaption).TextColor(t.Success)
			workspaceControlButton(c, "unapprove-"+state.Kind+"-"+state.Item, "撤销批准", "ghost", f.saving).Label("Withdraw workspace approval " + state.Kind + " " + state.Item).
				OnClick(func() { p.approveSettingsItem(state.Kind, state.Item, label, false) })
			return
		}
		ui.Text(c, "当前任务的工作区尚未批准，下一轮不会运行").FontSize(workspaceFontCaption).TextColor(t.Warning)
		workspaceControlButton(c, "approve-"+state.Kind+"-"+state.Item, "在此工作区批准", "secondary", f.saving).Label("Approve in workspace " + state.Kind + " " + state.Item).
			OnClick(func() { p.approveSettingsItem(state.Kind, state.Item, label, true) })
	})
}

func (p *NativePage) settingsFieldError(c *ui.Context, field string) {
	if message := p.settingsForm.errors.For(field); message != "" {
		ui.Text(c, message).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().Danger).Wrap()
	}
}

// settingsTextField is a labelled input with its field error underneath.
func (p *NativePage) settingsTextField(c *ui.Context, key, label, field, placeholder string, value *string) {
	ui.Column(c.Key("field-" + key)).FillWidth().Gap(4).Children(func() {
		ui.Text(c, label).FontSize(workspaceFontBody).LineHeight(1.5)
		input := workspaceControlInput(c, key, value).Label(label + "输入").FillWidth().Disabled(p.settingsForm.saving)
		if placeholder != "" {
			input.Placeholder(placeholder)
		}
		p.settingsFieldError(c, field)
	})
}

// settingsPathField adds an explicit file/folder chooser; choosing never runs anything.
func (p *NativePage) settingsPathField(c *ui.Context, key, label, field, placeholder string, value *string, directory bool) {
	ui.Column(c.Key("field-" + key)).FillWidth().Gap(4).Children(func() {
		ui.Text(c, label).FontSize(workspaceFontBody).LineHeight(1.5)
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			input := workspaceControlInput(c, key, value).Label(label + "输入").Grow(1).MinWidth(0).Disabled(p.settingsForm.saving)
			if placeholder != "" {
				input.Placeholder(placeholder)
			}
			workspaceControlButton(c, key+"-choose", "选择…", "secondary", p.window == nil || p.settingsForm.saving).Label("Choose " + label).
				OnClick(func() { p.chooseSettingsPath(value, label, directory) })
		})
		p.settingsFieldError(c, field)
	})
}

func (p *NativePage) settingsCheckRow(c *ui.Context, key, label string, value *bool) {
	ui.Row(c.Key("check-" + key)).FillWidth().AlignItems(ui.Center).Gap(12).Children(func() {
		ui.Text(c, label).FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0)
		workspaceControlSwitch(c, key, value, p.settingsForm.saving).Label(label)
	})
}

// settingsListField edits an ordered list of plain strings (arguments, origins).
func (p *NativePage) settingsListField(c *ui.Context, key, label, field, placeholder string, values *[]string) {
	ui.Column(c.Key("list-" + key)).FillWidth().Gap(4).Children(func() {
		ui.Text(c, label).FontSize(workspaceFontBody).LineHeight(1.5)
		for i := range *values {
			index := i
			ui.Row(c.Key(key + "-row-" + strconv.Itoa(index))).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				input := workspaceControlInput(c, key+"-"+strconv.Itoa(index), &(*values)[index]).Label(label + " " + strconv.Itoa(index+1)).Grow(1).MinWidth(0).Disabled(p.settingsForm.saving)
				if placeholder != "" {
					input.Placeholder(placeholder)
				}
				workspaceControlIconButton(c, key+"-remove-"+strconv.Itoa(index), "", "trash-2", "ghost", p.settingsForm.saving).Label("Remove " + label + " " + strconv.Itoa(index+1)).
					OnClick(func() { *values = append((*values)[:index:index], (*values)[index+1:]...) })
			})
		}
		workspaceControlIconButton(c, key+"-add", "添加", "plus", "ghost", p.settingsForm.saving || len(*values) >= 128).Label("Add " + label).
			OnClick(func() { *values = append(*values, "") })
		p.settingsFieldError(c, field)
	})
}

// settingsEnvField edits names and write-only values. A saved value is never
// shown; leaving its box empty keeps it, typing replaces it.
func (p *NativePage) settingsEnvField(c *ui.Context, key, label, field, namePlaceholder string, rows *[]settingsEnvRow) {
	ui.Column(c.Key("env-" + key)).FillWidth().Gap(4).Children(func() {
		ui.Text(c, label).FontSize(workspaceFontBody).LineHeight(1.5)
		for i := range *rows {
			index := i
			row := &(*rows)[index]
			ui.Row(c.Key(key + "-row-" + strconv.Itoa(index))).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				workspaceControlInput(c, key+"-name-"+strconv.Itoa(index), &row.name).Label(label + " name " + strconv.Itoa(index+1)).Placeholder(namePlaceholder).Width(200).Shrink(0).Disabled(p.settingsForm.saving)
				value := workspaceControlInput(c, key+"-value-"+strconv.Itoa(index), &row.value).Label(label + " value " + strconv.Itoa(index+1)).Grow(1).MinWidth(0).Disabled(p.settingsForm.saving)
				if !row.reveal {
					value.Password()
				}
				if row.kept {
					value.Placeholder("已保存（不显示），留空保持不变")
				} else {
					value.Placeholder("值")
				}
				workspaceControlIconButton(c, key+"-reveal-"+strconv.Itoa(index), "", "eye", "ghost").Label("Toggle value visibility " + strconv.Itoa(index+1)).
					Tooltip("只显示本窗口刚输入的内容").OnClick(func() { row.reveal = !row.reveal })
				workspaceControlIconButton(c, key+"-remove-"+strconv.Itoa(index), "", "trash-2", "ghost", p.settingsForm.saving).Label("Remove " + label + " " + strconv.Itoa(index+1)).
					OnClick(func() { *rows = append((*rows)[:index:index], (*rows)[index+1:]...) })
			})
		}
		workspaceControlIconButton(c, key+"-add", "添加", "plus", "ghost", p.settingsForm.saving || len(*rows) >= 64).Label("Add " + label).
			OnClick(func() { *rows = append(*rows, settingsEnvRow{}) })
		p.settingsFieldError(c, field)
	})
}

// settingsSaveBar is the shared Save / Reset row of a configuration form.
func (p *NativePage) settingsSaveBar(c *ui.Context, key string, save func(), reset func()) {
	f := &p.settingsForm
	ui.Row(c.Key(key)).FillWidth().Gap(8).Justify(ui.End).AlignItems(ui.Center).Children(func() {
		if reset != nil {
			workspaceControlButton(c, key+"-reset", "恢复默认", "ghost", f.saving || !f.loaded).Label("Reset " + key + " to defaults").OnClick(reset)
		}
		workspaceControlButton(c, key+"-reload", "重新载入", "secondary", f.saving).Label("Reload " + key).OnClick(func() {
			f.loaded, f.problem, f.errors, f.notice = false, "", nil, ""
			f.epoch++
			p.loadSettingsForm()
		})
		p.workspaceControlAction(c, key+"-save", "保存", "", "primary", f.saving, save, !f.loaded || f.loading).Label("Save " + key)
	})
}

// sessionReady reports whether a task-bound page may act now.
func (p *NativePage) settingsNeedsTask(c *ui.Context, purpose string) bool {
	if p.selected != "" && p.workspaceNavigationDetailReady() {
		return false
	}
	ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		ui.Text(c, purpose).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted).Grow(1).MinWidth(0).Wrap()
		workspaceControlButton(c, "settings-choose-session", "选择任务", "secondary", p.busy || p.paneBusy).Label("Choose settings session").
			OnClick(func() { p.openWorkspaceSessionChooser(p.workspaceSettingsCategory()) })
	})
	return true
}
