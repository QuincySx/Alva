// INPUT: The shared settings form owner and SettingsService views
// OUTPUT: Chinese forms for the plugins overview, tools, Hooks/diagnostics, browser, media and AEP pages
// POS: [L7 Hub UI] Saving never starts a process; switches and approvals are separate explicit actions
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) chooseSettingsPath(target *string, label string, directory bool) {
	if p.window == nil || p.settingsForm.saving {
		return
	}
	epoch, pane := p.settingsForm.epoch, p.pane
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Directory: directory, Title: "选择" + label + "（只记录路径，不会运行）"})
		p.dispatch(func() {
			if p.settingsForm.epoch != epoch || p.pane != pane {
				return
			}
			if err != nil {
				p.settingsForm.problem = err.Error()
				return
			}
			if len(paths) > 0 {
				*target = paths[0]
			}
		})
	}()
}

func (p *NativePage) openSettingsPane(pane string) {
	if p.paneBusy {
		return
	}
	p.setPane(pane)
}

// openSettings opens the plugins and tools overview for the selected task.
func (p *NativePage) openSettings()    { p.openSettingsPane("settings-plugins") }
func (p *NativePage) openMCPSettings() { p.openSettingsPane("settings") }
func (p *NativePage) openTools() {
	p.openSettingsPane("tools")
	if p.pane == "tools" && p.selected != "" {
		p.shellJobs = nil
		p.shellJobAction("", "", false) // Owned background jobs of the selected task.
	}
}
func (p *NativePage) openCommands()        { p.openSettingsPane("commands") }
func (p *NativePage) openBrowserSettings() { p.openSettingsPane("browser-settings") }
func (p *NativePage) openMediaSettings()   { p.openSettingsPane("media-settings") }
func (p *NativePage) openAEP()             { p.openSettingsPane("plugins") }
func (p *NativePage) settingsItemsOn(kind string) (on, total int) {
	for _, item := range p.settingsForm.view.Items {
		if item.Kind == kind {
			total++
			if item.Enabled {
				on++
			}
		}
	}
	return on, total
}

func (p *NativePage) buildPluginsCategory(c *ui.Context) {
	p.ensureSettingsForm(nil)
	f := &p.settingsForm
	settingsPageTitle(c, "插件与工具", "")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded {
		return
	}
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(24).Children(func() {
			settingsGroup(c, "settings-capabilities", "能力", "", func() {
				p.settingsSwitchRow(c, hub.CapabilitySubagents, "", "原生子 Agent", "允许 Alva 派生子 Agent 分担任务（最多 8 个，并行 2 个），每次派生仍需审批。")
				p.settingsSwitchRow(c, hub.CapabilityMemory, "", "持久记忆", "按工作区保存记忆，同一项目的会话共享；写入仍需审批。")
			})
			settingsGroup(c, "settings-sections", "管理", "", func() {
				count := func(kind string) string {
					on, total := p.settingsItemsOn(kind)
					if total == 0 {
						return "未配置"
					}
					return fmt.Sprintf("%d 个，%d 个已启用", total, on)
				}
				hooksOn, hooks := p.settingsItemsOn(hub.CapabilityHook)
				diagOn, diags := p.settingsItemsOn(hub.CapabilityDiagnostic)
				toolsOn := 0
				for _, kind := range []string{hub.CapabilityPlanning, hub.CapabilityNotebook, hub.CapabilityInteraction, hub.CapabilityUtility, hub.CapabilityShell, hub.CapabilityWeb} {
					on, _ := p.settingsItemsOn(kind)
					toolsOn += on
				}
				state := func(kind string) string {
					if on, _ := p.settingsItemsOn(kind); on > 0 {
						return "已启用"
					}
					return "未启用"
				}
				for _, entry := range []struct {
					key, title, icon, summary string
					open                      func()
				}{
					{"mcp", "MCP 服务器", "plug", count(hub.CapabilityMCP), p.openMCPSettings},
					{"aep", "扩展插件", "puzzle", count(hub.CapabilityAEP), p.openAEP},
					{"tools", "工具", "sliders-horizontal", fmt.Sprintf("%d 项已启用", toolsOn), p.openTools},
					{"commands", "Hooks 与诊断", "terminal", fmt.Sprintf("Hooks %d/%d，诊断 %d/%d", hooksOn, hooks, diagOn, diags), p.openCommands},
					{"browser", "浏览器工具", "globe", state(hub.CapabilityBrowser), p.openBrowserSettings},
					{"media", "媒体工具", "folder", state(hub.CapabilityMedia), p.openMediaSettings},
				} {
					entry := entry
					ui.Row(c.Key("settings-entry-"+entry.key)).FillWidth().AlignItems(ui.Center).Gap(12).Padding(8, 0).Children(func() {
						ui.Icon(c, workspaceIcon(entry.icon)).Size(workspaceIconSmall, workspaceIconSmall).Shrink(0).TextColor(c.Theme().TextMuted)
						ui.Text(c, entry.title).FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0)
						ui.Text(c, entry.summary).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
						workspaceControlIconButton(c, "settings-open-"+entry.key, "管理", "chevron-right", "secondary", p.paneBusy).Label("Open " + entry.title).OnClick(entry.open)
					})
				}
			})
		})
	})
}

func initToolsEditor(p *NativePage) {
	c := p.settingsForm.view.Capabilities
	e := newSettingsEditor("tools", 0)
	*e.str("shell") = c.Tools.Shell.Executable
	e.setEnv("shell.env", c.Tools.Shell.Env)
	*e.flag("web.read") = c.Tools.Web.ReadURLs
	*e.str("web.search") = c.Tools.Web.SearchEndpoint
	e.setList("web.origins", c.Tools.Web.AllowedPrivateOrigins)
	e.setNumber("web.timeout", c.Tools.Web.TimeoutMS)
	p.settingsForm.editor = e
}

func (p *NativePage) saveTools() {
	f := &p.settingsForm
	e := f.editor
	var problems product.FieldErrors
	desired := f.view.Capabilities.Clone()
	desired.Tools.Shell.Executable = strings.TrimSpace(*e.str("shell"))
	desired.Tools.Shell.Env = e.environment("shell.env", "tools.shell.env", &problems)
	desired.Tools.Web.ReadURLs = *e.flag("web.read")
	desired.Tools.Web.SearchEndpoint = strings.TrimSpace(*e.str("web.search"))
	desired.Tools.Web.AllowedPrivateOrigins = e.values("web.origins")
	desired.Tools.Web.TimeoutMS = e.number("web.timeout", "tools.web.timeout", &problems)
	if len(problems) > 0 {
		f.errors, f.notice = problems, ""
		return
	}
	p.saveSettingsSection(product.SectionTools, desired, true)
}

func (p *NativePage) buildTools(c *ui.Context) {
	p.ensureSettingsForm(initToolsEditor)
	f := &p.settingsForm
	settingsPageTitle(c, "工具", "可选工具从下一轮对话开始可用；每次调用仍按权限设置询问。")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded || f.editor == nil {
		return
	}
	e := f.editor
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(24).Children(func() {
			settingsGroup(c, "tools-builtin", "内置工具", "", func() {
				p.settingsSwitchRow(c, hub.CapabilityPlanning, "", "任务与笔记", "让 Alva 维护任务清单和计划笔记。")
				p.settingsSwitchRow(c, hub.CapabilityNotebook, "", "Notebook 单元格", "读取和编辑 .ipynb 单元格。")
				p.settingsSwitchRow(c, hub.CapabilityInteraction, "", "向你提问", "需要澄清时在对话里提问。")
				p.settingsSwitchRow(c, hub.CapabilityUtility, "", "实用工具", "偏好记录、工具搜索与等待。")
			})
			settingsGroup(c, "tools-shell", "Shell", "以你的完整权限在工作区运行命令。", func() {
				p.settingsSwitchRow(c, hub.CapabilityShell, "", "启用 Shell", "")
				p.settingsPathField(c, "tools-shell-executable", "Shell 可执行文件", "tools.shell.executable", "/bin/zsh", e.str("shell"), false)
				p.settingsEnvField(c, "tools-shell-env", "环境变量", "tools.shell.env", "名称", e.env("shell.env"))
			})
			settingsGroup(c, "tools-web", "网页", "读取网页和调用搜索端点会访问网络。", func() {
				p.settingsSwitchRow(c, hub.CapabilityWeb, "", "启用网页工具", "")
				p.settingsCheckRow(c, "tools-web-read", "读取网页", e.flag("web.read"))
				p.settingsTextField(c, "tools-web-search", "搜索端点", "tools.web.search", "https://search.example.com/api", e.str("web.search"))
				p.settingsListField(c, "tools-web-origins", "允许访问的内网来源", "tools.web.origins", "http://192.168.1.10:8080", e.list("web.origins"))
				p.settingsTextField(c, "tools-web-timeout", "超时（毫秒）", "tools.web.timeout", "留空使用默认值，最多 60000", e.str("web.timeout"))
			})
			p.settingsSaveBar(c, "tools", p.saveTools, func() {
				p.resetSettingsSection(product.SectionTools, "关闭全部工具并清空 Shell 与网页配置？")
			})
			if p.selected != "" {
				p.buildShellJobs(c)
			}
		})
	})
}

func initCommandsEditor(p *NativePage) { p.settingsForm.editor = nil }

func commandEditor(kind string, index int, command hub.NativeCommand, id, extra string) *settingsEditor {
	e := newSettingsEditor(kind, index)
	*e.str("id") = id
	*e.str("command") = command.Command
	e.setList("args", command.Args)
	e.setEnv("env", command.Env)
	e.setNumber("timeout", command.TimeoutMS)
	*e.str("extra") = extra
	return e
}

var hookEvents = []string{"PreToolUse", "PostToolUse", "PostToolUseFailure", "SessionStart", "SessionEnd", "Stop", "UserPromptSubmit", "Notification"}

func (p *NativePage) editCommand(kind string, index int) {
	f := &p.settingsForm
	c := f.view.Capabilities
	f.errors, f.notice = nil, ""
	switch {
	case kind == hub.CapabilityHook && index >= 0:
		hook := c.Commands.Hooks[index]
		f.editor = commandEditor(kind, index, hook.NativeCommand, hook.ID, hook.Matcher)
		*f.editor.str("event") = hook.Event
	case kind == hub.CapabilityHook:
		f.editor = commandEditor(kind, -1, hub.NativeCommand{}, "", "")
		*f.editor.str("event") = hookEvents[0]
	case index >= 0:
		profile := c.Commands.Diagnostics[index]
		f.editor = commandEditor(kind, index, profile.NativeCommand, profile.ID, profile.Description)
	default:
		f.editor = commandEditor(kind, -1, hub.NativeCommand{}, "", "")
	}
}

func (p *NativePage) saveCommand() {
	f := &p.settingsForm
	e := f.editor
	var problems product.FieldErrors
	desired := f.view.Capabilities.Clone()
	id := strings.TrimSpace(*e.str("id"))
	command := hub.NativeCommand{Command: strings.TrimSpace(*e.str("command")), Args: e.values("args")}
	field := e.kind + "." + id
	command.Env = e.environment("env", field+".env", &problems)
	command.TimeoutMS = e.number("timeout", field+".timeout", &problems)
	if len(problems) > 0 {
		f.errors = problems
		return
	}
	if e.kind == hub.CapabilityHook {
		hook := hub.HookSpec{ID: id, Event: *e.str("event"), Matcher: strings.TrimSpace(*e.str("extra")), NativeCommand: command}
		if e.index >= 0 {
			desired.Commands.Hooks[e.index] = hook
		} else {
			desired.Commands.Hooks = append(desired.Commands.Hooks, hook)
		}
	} else {
		profile := hub.DiagnosticSpec{ID: id, Description: strings.TrimSpace(*e.str("extra")), NativeCommand: command}
		if e.index >= 0 {
			desired.Commands.Diagnostics[e.index] = profile
		} else {
			desired.Commands.Diagnostics = append(desired.Commands.Diagnostics, profile)
		}
	}
	p.saveSettingsSection(product.SectionCommands, desired, false)
}

func (p *NativePage) deleteCommand(kind string, index int, label string) {
	f := &p.settingsForm
	f.confirm = &settingsConfirm{key: "delete-" + kind + "-" + strconv.Itoa(index), message: "删除“" + label + "”？", action: "删除", danger: true, run: func() {
		desired := f.view.Capabilities.Clone()
		if kind == hub.CapabilityHook {
			desired.Commands.Hooks = append(desired.Commands.Hooks[:index:index], desired.Commands.Hooks[index+1:]...)
		} else {
			desired.Commands.Diagnostics = append(desired.Commands.Diagnostics[:index:index], desired.Commands.Diagnostics[index+1:]...)
		}
		p.saveSettingsSection(product.SectionCommands, desired, false)
	}}
}

func (p *NativePage) buildCommands(c *ui.Context) {
	p.ensureSettingsForm(initCommandsEditor)
	f := &p.settingsForm
	settingsPageTitle(c, "Hooks 与诊断", "Hooks 在选定事件自动运行；诊断由 Alva 按需调用。命令以你的完整权限运行，保存不会启动它们。")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded {
		return
	}
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(24).Children(func() {
			settingsGroup(c, "commands-hooks", "Hooks", "最多 32 个；超时默认 10 秒，最长 60 秒。", func() {
				for i, hook := range f.view.Capabilities.Commands.Hooks {
					index, hook := i, hook
					p.settingsSwitchRow(c, hub.CapabilityHook, hook.ID, hook.ID+" · "+hook.Event, hook.Command)
					ui.Row(c).Gap(8).Children(func() {
						workspaceControlIconButton(c, "hook-edit-"+hook.ID, "编辑", "pencil", "ghost", f.saving).Label("Edit hook " + hook.ID).OnClick(func() { p.editCommand(hub.CapabilityHook, index) })
						workspaceControlIconButton(c, "hook-delete-"+hook.ID, "删除", "trash-2", "danger", f.saving).Label("Delete hook " + hook.ID).OnClick(func() { p.deleteCommand(hub.CapabilityHook, index, hook.ID) })
					})
				}
				workspaceControlIconButton(c, "hook-add", "添加 Hook", "plus", "secondary", f.saving || len(f.view.Capabilities.Commands.Hooks) >= 32).Label("Add hook").OnClick(func() { p.editCommand(hub.CapabilityHook, -1) })
			})
			settingsGroup(c, "commands-diagnostics", "诊断", "最多 16 个；超时默认 30 秒，最长 300 秒。", func() {
				for i, profile := range f.view.Capabilities.Commands.Diagnostics {
					index, profile := i, profile
					p.settingsSwitchRow(c, hub.CapabilityDiagnostic, profile.ID, profile.ID, profile.Description)
					ui.Row(c).Gap(8).Children(func() {
						workspaceControlIconButton(c, "diagnostic-edit-"+profile.ID, "编辑", "pencil", "ghost", f.saving).Label("Edit diagnostic " + profile.ID).OnClick(func() { p.editCommand(hub.CapabilityDiagnostic, index) })
						workspaceControlIconButton(c, "diagnostic-delete-"+profile.ID, "删除", "trash-2", "danger", f.saving).Label("Delete diagnostic " + profile.ID).OnClick(func() { p.deleteCommand(hub.CapabilityDiagnostic, index, profile.ID) })
					})
				}
				workspaceControlIconButton(c, "diagnostic-add", "添加诊断", "plus", "secondary", f.saving || len(f.view.Capabilities.Commands.Diagnostics) >= 16).Label("Add diagnostic").OnClick(func() { p.editCommand(hub.CapabilityDiagnostic, -1) })
			})
			if e := f.editor; e != nil {
				title := "编辑诊断"
				if e.kind == hub.CapabilityHook {
					title = "编辑 Hook"
				}
				settingsGroup(c, "commands-editor", title, "改动后需要重新启用并批准。", func() {
					field := e.kind + "." + *e.str("id")
					if e.index < 0 {
						p.settingsTextField(c, "command-id", "标识", field+".id", "例如 format-check", e.str("id"))
					}
					if e.kind == hub.CapabilityHook {
						ui.Text(c, "触发事件").FontSize(workspaceFontBody)
						workspaceControlSelect(c, "command-event", e.str("event"), hookEvents).Label("Hook event").FillWidth().Disabled(f.saving)
						p.settingsFieldError(c, field+".event")
						p.settingsTextField(c, "command-matcher", "匹配工具（可选）", field+".matcher", "例如 *_file，只允许首尾 *", e.str("extra"))
					} else {
						p.settingsTextField(c, "command-description", "说明", field+".description", "告诉 Alva 何时使用", e.str("extra"))
					}
					p.settingsPathField(c, "command-command", "命令", field+".command", "可执行文件的绝对路径", e.str("command"), false)
					p.settingsListField(c, "command-args", "参数", field+".args", "", e.list("args"))
					p.settingsEnvField(c, "command-env", "环境变量", field+".env", "名称", e.env("env"))
					p.settingsTextField(c, "command-timeout", "超时（毫秒）", field+".timeout", "留空使用默认值", e.str("timeout"))
					ui.Row(c).Gap(8).Justify(ui.End).FillWidth().Children(func() {
						workspaceControlButton(c, "command-cancel", "取消", "secondary", f.saving).Label("Cancel command edit").OnClick(func() { f.editor, f.errors = nil, nil })
						p.workspaceControlAction(c, "command-save", "保存", "", "primary", f.saving, p.saveCommand).Label("Save command")
					})
				})
			}
			ui.Row(c).FillWidth().Justify(ui.End).Children(func() {
				workspaceControlButton(c, "commands-reset", "恢复默认", "ghost", f.saving).Label("Reset commands to defaults").OnClick(func() {
					p.resetSettingsSection(product.SectionCommands, "停用全部 Hooks 和诊断？配置会保留。")
				})
			})
		})
	})
}

func initBrowserEditor(p *NativePage) {
	b := p.settingsForm.view.Capabilities.Browser
	e := newSettingsEditor("browser", 0)
	*e.str("executable") = b.Executable
	e.setEnv("env", b.Env)
	*e.flag("headless") = b.Headless
	*e.str("proxy") = b.Proxy
	e.setNumber("startup", b.StartupTimeoutMS)
	e.setNumber("operation", b.OperationTimeoutMS)
	p.settingsForm.editor = e
}

func (p *NativePage) saveBrowser() {
	f := &p.settingsForm
	e := f.editor
	var problems product.FieldErrors
	desired := f.view.Capabilities.Clone()
	desired.Browser.Executable = strings.TrimSpace(*e.str("executable"))
	desired.Browser.Env = e.environment("env", "browser.env", &problems)
	desired.Browser.Headless = *e.flag("headless")
	desired.Browser.Proxy = strings.TrimSpace(*e.str("proxy"))
	desired.Browser.StartupTimeoutMS = e.number("startup", "browser.startup_timeout", &problems)
	desired.Browser.OperationTimeoutMS = e.number("operation", "browser.operation_timeout", &problems)
	if len(problems) > 0 {
		f.errors, f.notice = problems, ""
		return
	}
	p.saveSettingsSection(product.SectionBrowser, desired, true)
}

func (p *NativePage) buildBrowserSettings(c *ui.Context) {
	p.ensureSettingsForm(initBrowserEditor)
	f := &p.settingsForm
	settingsPageTitle(c, "浏览器工具", "每轮使用全新的私有配置文件，不读取你浏览器里的登录状态，结束后清理。")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded || f.editor == nil {
		return
	}
	e := f.editor
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(16).Children(func() {
			p.settingsSwitchRow(c, hub.CapabilityBrowser, "", "启用浏览器工具", "导航、点击和输入可能产生外部影响，仍需逐次审批。")
			p.settingsPathField(c, "browser-executable", "浏览器可执行文件", "browser.executable", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", e.str("executable"), false)
			p.settingsCheckRow(c, "browser-headless", "无头模式", e.flag("headless"))
			p.settingsTextField(c, "browser-proxy", "代理（可选）", "browser.proxy", "http://127.0.0.1:7890", e.str("proxy"))
			p.settingsTextField(c, "browser-startup", "启动超时（毫秒）", "browser.startup_timeout", "留空使用默认值，最多 60000", e.str("startup"))
			p.settingsTextField(c, "browser-operation", "操作超时（毫秒）", "browser.operation_timeout", "留空使用默认值，最多 60000", e.str("operation"))
			p.settingsEnvField(c, "browser-env", "环境变量", "browser.env", "名称", e.env("env"))
			p.settingsSaveBar(c, "browser", p.saveBrowser, func() {
				p.resetSettingsSection(product.SectionBrowser, "关闭浏览器工具并清空它的配置？")
			})
		})
	})
}

func initMediaEditor(p *NativePage) {
	d := p.settingsForm.view.Capabilities.MediaDecoder
	e := newSettingsEditor("media", 0)
	*e.str("ffmpeg"), *e.str("ffprobe") = d.FFmpeg, d.FFprobe
	p.settingsForm.editor = e
}

func (p *NativePage) saveMedia() {
	f := &p.settingsForm
	desired := f.view.Capabilities.Clone()
	desired.MediaDecoder.FFmpeg = strings.TrimSpace(*f.editor.str("ffmpeg"))
	desired.MediaDecoder.FFprobe = strings.TrimSpace(*f.editor.str("ffprobe"))
	p.saveSettingsSection(product.SectionMedia, desired, true)
}

func (p *NativePage) buildMediaSettings(c *ui.Context) {
	p.ensureSettingsForm(initMediaEditor)
	f := &p.settingsForm
	settingsPageTitle(c, "媒体工具", "")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded || f.editor == nil {
		return
	}
	e := f.editor
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(24).Children(func() {
			p.settingsSwitchRow(c, hub.CapabilityMedia, "", "图片工具", "读取 PNG、JPEG、GIF 并缩放到 512px 以内，不调用外部程序。只有声明支持图片的模型连接才会收到图片。")
			settingsGroup(c, "media-decoder", "视频与 WebP 解码", "使用你指定的 ffmpeg/ffprobe，以完整权限运行。", func() {
				p.settingsSwitchRow(c, hub.CapabilityDecoder, "", "启用外部解码器", "")
				p.settingsPathField(c, "media-ffmpeg", "ffmpeg 路径", "media.ffmpeg", "/opt/homebrew/bin/ffmpeg", e.str("ffmpeg"), false)
				p.settingsPathField(c, "media-ffprobe", "ffprobe 路径", "media.ffprobe", "/opt/homebrew/bin/ffprobe", e.str("ffprobe"), false)
			})
			p.settingsSaveBar(c, "media", p.saveMedia, func() { p.resetSettingsSection(product.SectionMedia, "关闭媒体工具并清空解码器路径？") })
		})
	})
}

func initAEPEditor(p *NativePage) { p.settingsForm.editor = nil }

func (p *NativePage) editAEP(index int) {
	f := &p.settingsForm
	f.errors, f.notice = nil, ""
	e := newSettingsEditor(hub.CapabilityAEP, index)
	if index >= 0 {
		entry := f.view.Capabilities.AEP[index]
		*e.str("id"), *e.str("source"), *e.str("interpreter") = entry.ID, entry.SourceDirectory, entry.Interpreter
		e.setList("args", entry.InterpreterArgs)
		e.setEnv("env", entry.Env)
	} else if id, err := hub.NewID("aep-"); err == nil {
		*e.str("id") = id
	}
	f.editor = e
}

func (p *NativePage) saveAEPEntry() {
	f := &p.settingsForm
	e := f.editor
	var problems product.FieldErrors
	id := *e.str("id")
	entry := hub.AEPBundle{ID: id, SourceDirectory: strings.TrimSpace(*e.str("source")), Interpreter: strings.TrimSpace(*e.str("interpreter")), InterpreterArgs: e.values("args"), Env: e.environment("env", "aep."+id+".env", &problems)}
	if len(problems) > 0 {
		f.errors = problems
		return
	}
	desired := f.view.Capabilities.Clone()
	if e.index >= 0 {
		desired.AEP[e.index] = entry
	} else {
		desired.AEP = append(desired.AEP, entry)
	}
	p.saveSettingsSection(product.SectionAEP, desired, false)
}

func (p *NativePage) deleteAEP(index int, label string) {
	f := &p.settingsForm
	f.confirm = &settingsConfirm{key: "delete-aep-" + strconv.Itoa(index), message: "删除扩展插件“" + label + "”？源目录不会被删除。", action: "删除", danger: true, run: func() {
		desired := f.view.Capabilities.Clone()
		desired.AEP = append(desired.AEP[:index:index], desired.AEP[index+1:]...)
		p.saveSettingsSection(product.SectionAEP, desired, false)
	}}
}

func (p *NativePage) buildAEP(c *ui.Context) {
	p.ensureSettingsForm(initAEPEditor)
	f := &p.settingsForm
	settingsPageTitle(c, "扩展插件", "JS/Python 插件以你的权限运行；WASM 插件在隔离实例中运行，不能访问文件、环境变量和网络。保存时只检查源目录并记录指纹，不会运行插件。")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded {
		return
	}
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(16).Children(func() {
			for i, entry := range f.view.Capabilities.AEP {
				index, entry := i, entry
				title := entry.Name
				if title == "" {
					title = entry.ID
				}
				detail := entry.SourceDirectory
				if entry.Version != "" {
					detail = "版本 " + entry.Version + " · " + detail
				}
				p.settingsSwitchRow(c, hub.CapabilityAEP, entry.ID, title, detail)
				ui.Row(c).Gap(8).Children(func() {
					workspaceControlIconButton(c, "aep-edit-"+entry.ID, "编辑", "pencil", "ghost", f.saving).Label("Edit AEP " + entry.ID).OnClick(func() { p.editAEP(index) })
					workspaceControlIconButton(c, "aep-delete-"+entry.ID, "删除", "trash-2", "danger", f.saving).Label("Delete AEP " + entry.ID).OnClick(func() { p.deleteAEP(index, title) })
				})
			}
			workspaceControlIconButton(c, "aep-add", "添加插件", "plus", "secondary", f.saving || len(f.view.Capabilities.AEP) >= 8).Label("Add AEP plugin").OnClick(func() { p.editAEP(-1) })
			if e := f.editor; e != nil {
				id := *e.str("id")
				settingsGroup(c, "aep-editor", "插件配置", "WASM 插件的解释器、参数和环境变量留空。", func() {
					p.settingsPathField(c, "aep-source", "源目录", "aep."+id+".source", "包含 manifest 的插件目录", e.str("source"), true)
					p.settingsPathField(c, "aep-interpreter", "解释器（JS/Python）", "aep."+id+".interpreter", "例如 /opt/homebrew/bin/node", e.str("interpreter"), false)
					p.settingsListField(c, "aep-args", "解释器参数", "aep."+id+".args", "", e.list("args"))
					p.settingsEnvField(c, "aep-env", "环境变量", "aep."+id+".env", "名称", e.env("env"))
					p.settingsFieldError(c, "aep")
					ui.Row(c).Gap(8).Justify(ui.End).FillWidth().Children(func() {
						workspaceControlButton(c, "aep-cancel", "取消", "secondary", f.saving).Label("Cancel AEP edit").OnClick(func() { f.editor, f.errors = nil, nil })
						p.workspaceControlAction(c, "aep-save", "检查并保存", "", "primary", f.saving, p.saveAEPEntry).Label("Inspect and save AEP")
					})
				})
			}
			ui.Row(c).FillWidth().Justify(ui.End).Children(func() {
				workspaceControlButton(c, "aep-reset", "恢复默认", "ghost", f.saving).Label("Reset AEP to defaults").OnClick(func() {
					p.resetSettingsSection(product.SectionAEP, "停用全部扩展插件？配置会保留。")
				})
			})
		})
	})
}
