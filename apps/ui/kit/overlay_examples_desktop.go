// INPUT: Alva Icon (Name 69), Text scale, Tooltip (single) and MenuItem (State 8).
// OUTPUT: Gallery pages; Text has no design component and is listed with Design "".
// POS: [L7 Kit] Tooltip and menu items render through the static helpers so the matrix needs no hover or open popover.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/egoist/mygo/ui"
)

func textPage() Page {
	return Page{
		Name: "Text", Category: "基础",
		Axes: []Axis{axis("Role", "Caption", "Body", "Subtitle", "Heading", "Display", "Mono")},
		Cell: func(c *ui.Context, sel []string) {
			role := map[string]TextRole{"Caption": RoleCaption, "Body": RoleBody, "Subtitle": RoleSubtitle, "Heading": RoleHeading, "Display": RoleDisplay, "Mono": RoleMono}[sel[0]]
			Text(c, "Alva 字阶 Aa 123", TextProps{Role: role, SingleLine: true})
		},
	}
}

func iconPage() Page {
	names := icon.Names()
	values := make([]string, len(names))
	for i, n := range names {
		values[i] = string(n)
	}
	return Page{
		Name: "Icon", Design: "Icon", Category: "基础",
		Axes: []Axis{axis("Name", values...)},
		Cell: func(c *ui.Context, sel []string) {
			Icon(c, icon.Name(sel[0]), IconProps{Label: sel[0], Tone: ToneText})
		},
	}
}

func tooltipPage() Page {
	return Page{
		Name: "Tooltip", Design: "Tooltip", Category: "反馈",
		Cell: func(c *ui.Context, _ []string) { tipStatic(c, "显示运行详情") },
	}
}

func menuPage() Page {
	return Page{
		Name: "Menu", Design: "MenuItem", Category: "导航",
		Axes: []Axis{axis("State", "Default", "Hover", "Selected", "Disabled", "Danger", "Focus", "SelectedFocus", "SelectedDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			state := sel[0]
			it := MenuItem{Label: "添加目录", Description: "选择当前任务可用的资料", Shortcut: "⌘ O", Icon: icon.Folder,
				Selected: len(state) >= 8 && state[:8] == "Selected", Danger: state == "Danger", Disabled: state == "Disabled" || state == "SelectedDisabled"}
			forced := force.None
			switch state {
			case "Hover":
				forced = force.Hover
			case "Focus", "SelectedFocus":
				forced = force.Focus
			}
			menuItemStatic(c, it, forced)
		},
		Live: func(c *ui.Context) {
			open, chosen := cellSlot("Menu", nil, "open", false), cellSlot("Menu", nil, "chosen", "尚未选择")
			trigger := Button(c, "打开菜单", ButtonProps{Icon: icon.More, Key: "menu-trigger", OnClick: func() { *open = !*open }})
			if key := Menu(c, trigger, open, []MenuItem{
				{Key: "copy", Label: "复制", Icon: icon.Copy, Shortcut: "⌘ C"},
				{Key: "rename", Label: "重命名", Icon: icon.Edit, Disabled: true},
				{Key: "export", Label: "导出", Description: "保存为文件", Icon: icon.Download},
				{Separator: true},
				{Key: "delete", Label: "删除", Icon: icon.Trash, Danger: true},
			}, MenuProps{Label: "示例菜单"}); key != "" {
				*chosen = "已选择：" + key
			}
			Text(c, *chosen, TextProps{Role: RoleCaption, Tone: ToneSecondary, Key: "menu-chosen"})
		},
	}
}
