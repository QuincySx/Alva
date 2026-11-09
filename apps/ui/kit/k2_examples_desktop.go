// INPUT: The design axes of the K2 components (display, choice, rows, overlays, shell).
// OUTPUT: Gallery pages whose cells are the design variants, with live demos for the interactive overlays and choices.
// POS: [L7 Kit] Same rules as the K1 examples: forced states via internal/force, per-cell state via cellSlot, no hand-written variant counts.
//go:build desktop

package kit

import (
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// Sample sizes that are gallery layout, not design tokens.
const (
	sampleWide         = 360
	sampleSidebarH     = 260
	sampleSkeletonW    = 200
	sampleHalf         = sampleWide / 2
	sampleWider        = sampleWide + sampleWide/2
	sampleSkeletonHalf = sampleSkeletonW / 2
)

// stateForce maps a design State value to the forced runtime state.
func stateForce(state string) force.State {
	switch {
	case strings.HasSuffix(state, "Focus"):
		return force.Focus
	case strings.HasSuffix(state, "Hover"):
		return force.Hover
	case strings.HasSuffix(state, "Pressed"):
		return force.Pressed
	}
	return force.None
}

func isSelected(state string) bool { return strings.HasPrefix(state, "Selected") }

func k2Pages() []Page {
	return []Page{
		badgePage(), avatarPage(), noticePage(), emptyStatePage(), skeletonPage(), progressPage(), stepPage(), paginationPage(), detailRowPage(), toastPage(),
		dividerPage(), cardPage(),
		radioPage(), segmentPage(), tabPage(), toggleButtonPage(),
		navItemPage(), treeRowPage(), skillRowPage(), resourceRowPage(), companionRowPage(), settingRowPage(),
		dialogHeaderPage(), dialogFooterPage(), dialogPage(), popoverPage(),
		pageHeaderPage(), panelHeaderPage(), sidebarPage(), settingsNavPage(),
	}
}

func badgePage() Page {
	return Page{Name: "Badge", Design: "Badge", Category: "反馈", Axes: []Axis{axis("Tone", "Neutral", "Accent", "Success", "Warning", "Danger", "Info")},
		Cell: func(c *ui.Context, sel []string) {
			tone := map[string]BadgeTone{"Neutral": BadgeNeutral, "Accent": BadgeAccent, "Success": BadgeSuccess, "Warning": BadgeWarning, "Danger": BadgeDanger, "Info": BadgeInfo}[sel[0]]
			Badge(c, "标签", BadgeProps{Tone: tone, Key: "b"})
		}}
}

func avatarPage() Page {
	return Page{Name: "Avatar", Design: "Avatar", Category: "反馈", Axes: []Axis{axis("Kind", AvatarPerson.String(), AvatarAgent.String(), AvatarProvider.String())},
		Cell: func(c *ui.Context, sel []string) {
			Avatar(c, AvatarProps{Kind: map[string]AvatarKind{AvatarPerson.String(): AvatarPerson, AvatarAgent.String(): AvatarAgent, AvatarProvider.String(): AvatarProvider}[sel[0]], Name: "林", Key: "a"})
		}}
}

func noticePage() Page {
	return Page{Name: "Notice", Design: "Notice", Category: "反馈", Axes: []Axis{axis("Tone", "Info", "Success", "Warning", "Danger")},
		Cell: func(c *ui.Context, sel []string) {
			tone := map[string]BadgeTone{"Info": BadgeInfo, "Success": BadgeSuccess, "Warning": BadgeWarning, "Danger": BadgeDanger}[sel[0]]
			Notice(c, NoticeProps{Tone: tone, Title: "模型目录已更新", Description: "共 12 个模型可用", OnClose: func() {}, Key: "n"}).Width(sampleWide)
		}}
}

func emptyStatePage() Page {
	return Page{Name: "EmptyState", Design: "EmptyState", Category: "反馈", Axes: []Axis{axis("State", "Empty", "Loading", "Error", "Unavailable")},
		Cell: func(c *ui.Context, sel []string) {
			kind := map[string]EmptyKind{"Empty": EmptyNone, "Loading": EmptyLoading, "Error": EmptyError, "Unavailable": EmptyUnavailable}[sel[0]]
			title := map[string]string{"Empty": "还没有内容", "Loading": "正在加载", "Error": "加载失败", "Unavailable": "暂不可用"}[sel[0]]
			EmptyState(c, EmptyStateProps{Kind: kind, Title: title, Description: "稍后再试，或检查连接", Key: "e", Action: func() {
				if kind == EmptyError {
					Button(c, "重试", ButtonProps{Key: "retry"})
				}
			}}).Width(sampleHalf)
		}}
}

func skeletonPage() Page {
	return Page{Name: "Skeleton", Design: "Skeleton", Category: "反馈", Cell: func(c *ui.Context, _ []string) {
		SkeletonGroup(c, "sk", func() {
			Skeleton(c.Key(0), sampleSkeletonW, theme.Space4)
			Skeleton(c.Key(1), sampleSkeletonHalf, theme.Space4)
		})
	}}
}

func progressPage() Page {
	return Page{Name: "Progress", Design: "Progress", Category: "反馈", Axes: []Axis{axis("State", "Running", "Done", "Error")},
		Cell: func(c *ui.Context, sel []string) {
			st := map[string]ProgressState{"Running": ProgressRunning, "Done": ProgressDone, "Error": ProgressError}[sel[0]]
			pct := float32(60)
			if st == ProgressDone {
				pct = percentMax
			}
			Progress(c, ProgressProps{Title: "同步模型目录", Percent: pct, State: st, Key: "p"}).Width(sampleWide)
		}}
}

func stepPage() Page {
	return Page{Name: "Step", Design: "Step", Category: "导航", Axes: []Axis{axis("State", "Current", "Done", "Next")},
		Cell: func(c *ui.Context, sel []string) {
			Step(c, "配置连接", StepProps{Index: 2, State: map[string]StepState{"Current": StepCurrent, "Done": StepDone, "Next": StepNext}[sel[0]], Key: "s"})
		}}
}

func paginationPage() Page {
	return Page{Name: "Pagination", Design: "Pagination", Category: "导航", Axes: []Axis{axis("State", "Ready", "Loading", "End")},
		Cell: func(c *ui.Context, sel []string) {
			Pagination(c, PaginationProps{State: map[string]PaginationState{"Ready": PaginationReady, "Loading": PaginationLoading, "End": PaginationEnd}[sel[0]], OnMore: func() {}, Key: "p"}).Width(sampleHalf)
		}}
}

func detailRowPage() Page {
	return Page{Name: "DetailRow", Design: "DetailRow", Category: "数据", Cell: func(c *ui.Context, _ []string) {
		DetailRow(c, "创建时间", "2026-11-09 10:30").Width(sampleWide)
	}}
}

func toastPage() Page {
	return Page{Name: "Toast", Design: "Toast", Category: "反馈", Axes: []Axis{axis("Tone", "Success", "Error", "Info")},
		Cell: func(c *ui.Context, sel []string) {
			ToastView(c, map[string]ToastTone{"Success": ToastSuccess, "Error": ToastError, "Info": ToastInfo}[sel[0]], "已保存设置")
		},
		Live: func(c *ui.Context) {
			Button(c, "显示提示", ButtonProps{Key: "toast", OnClick: func() { ShowToast(c, ToastSuccess, "已保存设置") }})
		}}
}

func dividerPage() Page {
	return Page{Name: "Divider", Category: "基础", Cell: func(c *ui.Context, _ []string) {
		ui.Row(c).Height(theme.Space6).Gap(theme.Space4).AlignItems(ui.Center).Children(func() {
			Text(c, "左", TextProps{Key: "a"})
			Divider(c.Key("v"), true)
			Text(c, "右", TextProps{Key: "b"})
		})
	}}
}

func cardPage() Page {
	return Page{Name: "Card", Category: "基础", Cell: func(c *ui.Context, _ []string) {
		Card(c, "card", func() {
			Text(c, "卡片标题", TextProps{Strong: true, Key: "t"})
			Divider(c.Key("d"), false)
			Text(c, "内容组间隔 16，圆角 16。", TextProps{Tone: ToneSecondary, Key: "b"})
		}).Width(sampleWide)
	}}
}

func radioPage() Page {
	return Page{Name: "Radio", Design: "Radio", Category: "选择", Axes: []Axis{axis("State", "Off", "On", "Focus", "Disabled", "OnFocus", "OnDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			on := strings.HasPrefix(sel[0], "On")
			initial := 0
			if on {
				initial = 1
			}
			value := cellSlot("Radio", sel, "value", initial)
			force.With(stateForce(sel[0]), func() {
				RadioGroup(c, "示例选项", false, func() {
					Radio(c, value, 1, "选择此项", RadioProps{Disabled: strings.HasSuffix(sel[0], "Disabled"), Key: "r"})
				})
			})
		}}
}

func segmentPage() Page {
	return Page{Name: "Segmented", Design: "Segment", Category: "选择", Axes: []Axis{axis("State", "Default", "Selected", "Disabled", "SelectedFocus", "SelectedDisabled", "Focus")},
		Cell: func(c *ui.Context, sel []string) {
			segmentStatic(c, "按周", isSelected(sel[0]), strings.HasSuffix(sel[0], "Disabled"), stateForce(sel[0]))
		},
		Live: func(c *ui.Context) {
			value := cellSlot("Segmented", nil, "live", 1)
			Segmented(c, value, []string{"按日", "按周", "按月", "按年"}, SegmentedProps{Label: "统计周期", Disabled: []bool{3: true}, Key: "live"})
		}}
}

func tabPage() Page {
	return Page{Name: "Tabs", Design: "Tab", Category: "导航", Axes: []Axis{axis("State", "Default", "Selected", "Focus", "Busy", "SelectedFocus", "SelectedDisabled", "Disabled")},
		Cell: func(c *ui.Context, sel []string) {
			state := sel[0]
			tabStatic(c, TabItem{Label: "概览", Busy: state == "Busy", Disabled: strings.HasSuffix(state, "Disabled")}, isSelected(state), stateForce(state))
		},
		Live: func(c *ui.Context) {
			value := cellSlot("Tabs", nil, "live", 0)
			Tabs(c, value, []TabItem{{Label: "概览"}, {Label: "活动", Icon: icon.Activity}, {Label: "同步中", Busy: true}, {Label: "归档", Disabled: true}}, TabsProps{Label: "示例标签", Key: "live"})
		}}
}

func toggleButtonPage() Page {
	return Page{Name: "ToggleButton", Design: "ToggleButton", Category: "动作", Axes: []Axis{
		axis("State", "Default", "Hover", "Pressed", "Focus", "Disabled", "Selected", "SelectedHover", "SelectedPressed", "SelectedFocus", "SelectedDisabled"), axis("Content", "Text", "Leading")},
		Cell: func(c *ui.Context, sel []string) {
			on := cellSlot("ToggleButton", sel, "on", isSelected(sel[0]))
			p := ToggleButtonProps{Disabled: strings.HasSuffix(sel[0], "Disabled"), Key: "t"}
			if sel[1] == "Leading" {
				p.Icon = icon.Star
			}
			force.With(stateForce(sel[0]), func() { ToggleButton(c, on, "收藏", p) })
		}}
}

func rowStates(state string) (selected, disabled bool, forced force.State) {
	return isSelected(state), strings.HasSuffix(state, "Disabled"), stateForce(state)
}

func navItemPage() Page {
	return Page{Name: "NavItem", Design: "NavItem", Category: "导航", Axes: []Axis{axis("State", "Default", "Hover", "Selected", "Focus", "Disabled", "SelectedFocus", "SelectedDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			selected, disabled, forced := rowStates(sel[0])
			force.With(forced, func() {
				NavItem(c, "工作区", NavItemProps{Icon: icon.Chat, Selected: selected, Disabled: disabled, Key: "n"}).Width(theme.SizeNavigation(false))
			})
		}}
}

func treeRowPage() Page {
	return Page{Name: "TreeRow", Design: "TreeRow", Category: "数据", Axes: []Axis{axis("Kind", "Folder", "File"), axis("State", "Default", "Selected", "Focus", "SelectedFocus", "Unavailable")},
		Cell: func(c *ui.Context, sel []string) {
			selected, _, forced := rowStates(sel[1])
			kind, label := TreeFolder, "src"
			if sel[0] == "File" {
				kind, label = TreeFile, "main.go"
			}
			force.With(forced, func() {
				TreeRow(c, label, TreeRowProps{Kind: kind, Level: 1, Selected: selected, Unavailable: sel[1] == "Unavailable", Key: "t"}).Width(theme.SizeNavigation(false))
			})
		}}
}

func skillRowPage() Page {
	return Page{Name: "SkillRow", Design: "SkillRow", Category: "数据", Axes: []Axis{axis("State", "Shared", "Owned", "Unavailable")},
		Cell: func(c *ui.Context, sel []string) {
			SkillRow(c, "代码审查", SkillRowProps{State: map[string]SkillState{"Shared": SkillShared, "Owned": SkillOwned, "Unavailable": SkillUnavailable}[sel[0]], Description: "检查改动中的缺陷", Key: "s"}).Width(sampleWide)
		}}
}

func resourceRowPage() Page {
	return Page{Name: "ResourceRow", Design: "ResourceRow", Category: "数据", Axes: []Axis{axis("Kind", "File", "Directory", "Link", "Media")},
		Cell: func(c *ui.Context, sel []string) {
			ResourceRow(c, "设计说明", ResourceRowProps{Kind: map[string]ResourceKind{"File": ResourceFile, "Directory": ResourceDirectory, "Link": ResourceLink, "Media": ResourceMedia}[sel[0]], Detail: "docs/design/alva", Key: "r"}).Width(sampleWide)
		}}
}

func companionRowPage() Page {
	return Page{Name: "CompanionRow", Design: "CompanionRow", Category: "数据", Axes: []Axis{axis("State", "Default", "Selected", "Paused", "Error")},
		Cell: func(c *ui.Context, sel []string) {
			CompanionRow(c, "研究伙伴", CompanionRowProps{State: map[string]CompanionState{"Default": CompanionDefault, "Selected": CompanionSelected, "Paused": CompanionPaused, "Error": CompanionError}[sel[0]], Description: "整理资料并跟进", Key: "c"}).Width(sampleWide)
		}}
}

func settingRowPage() Page {
	return Page{Name: "SettingRow", Design: "SettingRow", Category: "输入", Axes: []Axis{axis("Control", "Switch", "Action", "Select")},
		Cell: func(c *ui.Context, sel []string) {
			SettingRow(c, SettingRowProps{Title: "自动同步", Description: "启动时刷新模型目录", Key: "s"}, func() {
				switch sel[0] {
				case "Switch":
					on := cellSlot("SettingRow", sel, "on", true)
					Switch(c, on, "自动同步", SwitchProps{Key: "sw"})
				case "Action":
					Button(c, "管理", ButtonProps{Kind: Ghost, TrailingIcon: icon.ChevronRight, Key: "act"})
				default:
					value := cellSlot("SettingRow", sel, "v", "weekly")
					Select(c, value, []Option[string]{{Value: "weekly", Label: "每周"}, {Value: "daily", Label: "每天"}}, SelectProps{Label: "频率", Plain: true, Key: "sel"})
				}
			}).Width(sampleWide)
		}}
}

func dialogHeaderPage() Page {
	return Page{Name: "DialogHeader", Design: "DialogHeader", Category: "反馈", Cell: func(c *ui.Context, _ []string) {
		DialogHeader(c, "删除会话", "此操作无法撤销。", func() {}).Width(sampleWide)
	}}
}

func dialogFooterPage() Page {
	return Page{Name: "DialogFooter", Design: "DialogFooter", Category: "反馈", Cell: func(c *ui.Context, _ []string) {
		DialogFooter(c, func() {
			Button(c, "取消", ButtonProps{Key: "cancel"})
			Button(c, "删除", ButtonProps{Kind: Danger, Key: "ok"})
		}).Width(sampleWide)
	}}
}

func dialogPage() Page {
	return Page{Name: "Dialog", Category: "反馈", Cell: func(c *ui.Context, _ []string) {
		Text(c, "对话框与侧边栏在下方演示中打开。", TextProps{Tone: ToneSecondary, Key: "hint"})
	}, Live: func(c *ui.Context) {
		dialog, sheet := cellSlot("Dialog", nil, "dialog", false), cellSlot("Dialog", nil, "sheet", false)
		ui.Row(c.Key("row")).Gap(theme.Space2).Children(func() {
			Button(c, "打开对话框", ButtonProps{Key: "open-dialog", OnClick: func() { *dialog = true }})
			Button(c, "打开侧边栏", ButtonProps{Key: "open-sheet", OnClick: func() { *sheet = true }})
		})
		Dialog(c, dialog, DialogProps{Label: "删除会话"}, func() {
			DialogHeader(c, "删除会话", "此操作无法撤销。", func() { *dialog = false })
			DialogFooter(c, func() {
				Button(c, "取消", ButtonProps{Key: "cancel", OnClick: func() { *dialog = false }})
				Button(c, "删除", ButtonProps{Kind: Danger, Key: "ok", OnClick: func() { *dialog = false }})
			})
		})
		Dialog(c, sheet, DialogProps{Label: "详情", Sheet: true}, func() {
			DialogHeader(c, "详情", "", func() { *sheet = false })
			DetailRow(c, "状态", "已完成")
		})
	}}
}

func popoverPage() Page {
	return Page{Name: "Popover", Category: "反馈", Cell: func(c *ui.Context, _ []string) {
		Text(c, "弹层在下方演示中打开。", TextProps{Tone: ToneSecondary, Key: "hint"})
	}, Live: func(c *ui.Context) {
		open := cellSlot("Popover", nil, "open", false)
		trigger := Button(c, "打开弹层", ButtonProps{Key: "trigger", OnClick: func() { *open = !*open }})
		Popover(c, trigger, open, PopoverProps{Label: "筛选", Width: sampleHalf}, func() {
			Text(c, "筛选条件", TextProps{Strong: true, Key: "t"})
			on := cellSlot("Popover", nil, "check", true)
			Checkbox(c, on, "只看进行中", CheckboxProps{Key: "c"})
		})
	}}
}

func pageHeaderPage() Page {
	return Page{Name: "PageHeader", Design: "PageHeader", Category: "壳", Axes: []Axis{axis("Navigation", "Expanded", "Collapsed")},
		Cell: func(c *ui.Context, sel []string) {
			PageHeader(c, PageHeaderProps{Title: "设置", Description: "常规", Collapsed: sel[0] == "Collapsed", OnToggleSidebar: func() {}, Key: "h",
				Actions: func() { Button(c, "保存", ButtonProps{Kind: Primary, Key: "save"}) }}).Width(sampleWider)
		}}
}

func panelHeaderPage() Page {
	return Page{Name: "PanelHeader", Design: "PanelHeader", Category: "壳", Cell: func(c *ui.Context, _ []string) {
		PanelHeader(c, PanelHeaderProps{Title: "文件", OnClose: func() {}, Key: "p", Actions: func() { IconButton(c, icon.Plus, "添加文件", IconButtonProps{Key: "add"}) }}).Width(theme.SizePanel(true))
	}}
}

func sidebarPage() Page {
	return Page{Name: "Sidebar", Design: "Sidebar", Category: "壳", Axes: []Axis{axis("Area", "Work", "Partners", "Settings", "New")},
		Cell: func(c *ui.Context, sel []string) {
			area := cellSlot("Sidebar", sel, "area", map[string]SidebarArea{"Work": AreaWork, "Partners": AreaPartners, "Settings": AreaSettings, "New": AreaNew}[sel[0]])
			ui.Box(c).Height(sampleSidebarH).Children(func() {
				Sidebar(c, SidebarProps{Area: *area, OnSelect: func(a SidebarArea) { *area = a }, OnCollapse: func() {}, Key: "s"})
			})
		}}
}

func settingsNavPage() Page {
	cats := []string{"General", "Models", "Plugins", "Skills", "Storage", "Security", "Gateway", "Developer"}
	glyphs := []icon.Name{icon.Settings, icon.Chip, icon.Plugin, icon.Sparkles, icon.Database, icon.Shield, icon.Globe, icon.Terminal}
	labels := []string{"常规", "模型", "插件", "技能", "存储", "安全", "网关", "开发者"}
	return Page{Name: "SettingsNav", Design: "SettingsNav", Category: "壳", Axes: []Axis{axis("Category", cats...)},
		Cell: func(c *ui.Context, sel []string) {
			index := 0
			for i, name := range cats {
				if name == sel[0] {
					index = i
				}
			}
			selected := cellSlot("SettingsNav", sel, "i", index)
			list := make([]SettingsCategory, len(cats))
			for i := range cats {
				list[i] = SettingsCategory{Label: labels[i], Icon: glyphs[i]}
			}
			SettingsNav(c, list, SettingsNavProps{Selected: *selected, OnSelect: func(i int) { *selected = i }, Key: "n"})
		}}
}
