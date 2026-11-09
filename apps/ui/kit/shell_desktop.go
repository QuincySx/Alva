// INPUT: Alva PageHeader (Navigation), PanelHeader, Sidebar (Area) and SettingsNav (Category) definitions.
// OUTPUT: Shell chrome: the page and panel header rows, the sidebar and the settings navigation, as appearance and interaction only.
// POS: [L7 Kit] No business: callers pass labels, selection and callbacks; the kit never knows what a Work area or a Models category does.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// PageHeaderProps configure PageHeader.
type PageHeaderProps struct {
	Title, Description string
	// Collapsed shows the sidebar toggle at the start of the header (the sidebar is hidden); expanded has it in the sidebar.
	Collapsed       bool
	OnToggleSidebar func()
	// Actions builds trailing controls.
	Actions func()
	Key     string
}

// PageHeader is the 52px page title row.
func PageHeader(c *ui.Context, p PageHeaderProps) ui.Element {
	return ui.Row(keyed(c, p.Key)).FillWidth().Height(theme.SizeHeader).Shrink(0).Padding(0, theme.Space4).Gap(theme.Space2).AlignItems(ui.Center).Children(func() {
		if p.Collapsed {
			IconButton(c, icon.PanelLeft, "展开侧栏", IconButtonProps{OnClick: p.OnToggleSidebar, Key: "toggle"})
		}
		Text(c, p.Title, TextProps{Role: RoleSubtitle, Strong: true, SingleLine: true, Key: "title"})
		if p.Description != "" {
			Text(c, p.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary, SingleLine: true, Key: "desc"})
		}
		ui.Spacer(c).Grow(1)
		if p.Actions != nil {
			p.Actions()
		}
	})
}

// PanelHeaderProps configure PanelHeader.
type PanelHeaderProps struct {
	Title   string
	Actions func()
	// OnClose adds a close action at the end.
	OnClose func()
	Key     string
}

// PanelHeader is the 52px title row of a side panel.
func PanelHeader(c *ui.Context, p PanelHeaderProps) ui.Element {
	return ui.Row(keyed(c, p.Key)).FillWidth().Height(theme.SizeHeader).Shrink(0).Padding(0, theme.Space3).Gap(theme.Space1).AlignItems(ui.Center).Children(func() {
		Text(c, p.Title, TextProps{Strong: true, SingleLine: true, Key: "title"}).Grow(1).MinWidth(0)
		if p.Actions != nil {
			p.Actions()
		}
		if p.OnClose != nil {
			IconButton(c, icon.Close, "关闭面板", IconButtonProps{OnClick: p.OnClose, Key: "close"})
		}
	})
}

// SidebarArea is the design Area axis of Sidebar; Work is the zero value.
type SidebarArea uint8

const (
	AreaWork SidebarArea = iota
	AreaPartners
	AreaSettings
	AreaNew
)

// SidebarProps configure Sidebar.
type SidebarProps struct {
	Area SidebarArea
	// Narrow shows icons only (theme.ViewSidebarLabels(true) is false) at the narrow width.
	Narrow bool
	// OnSelect is called with the chosen area; OnCollapse hides the sidebar.
	OnSelect   func(SidebarArea)
	OnCollapse func()
	// Label names the navigation for assistive technology; it defaults to 主导航.
	Label string
	Key   string
}

// Sidebar is the left navigation: collapse toggle, new action and the three areas.
func Sidebar(c *ui.Context, p SidebarProps) ui.Element {
	pl := pal(c)
	labels := theme.ViewSidebarLabels(p.Narrow)
	pick := func(a SidebarArea) func() {
		return func() {
			if p.OnSelect != nil {
				p.OnSelect(a)
			}
		}
	}
	label := p.Label
	if label == "" {
		label = "主导航"
	}
	return ui.Column(keyed(c, p.Key)).Width(theme.SizeSidebar(p.Narrow)).Shrink(0).FillHeight().Padding(theme.Space3).Gap(theme.Space1).Background(col(pl.Sidebar)).
		Role(ui.RoleList).Label(label).FocusGroup(ui.Vertical).Children(func() {
		ui.Row(c.Key("top")).FillWidth().Height(theme.ControlHeight).AlignItems(ui.Center).Justify(ui.End).Children(func() {
			IconButton(c, icon.PanelLeft, "收起侧栏", IconButtonProps{OnClick: p.OnCollapse, Key: "collapse"})
		})
		kind := Secondary
		if p.Area == AreaNew {
			kind = Primary
		}
		if labels {
			Button(c, "新建", ButtonProps{Kind: kind, Icon: icon.Plus, OnClick: pick(AreaNew), Key: "new"}).FillWidth()
		} else {
			Button(c, "新建", ButtonProps{Kind: kind, Icon: icon.Plus, IconOnly: true, OnClick: pick(AreaNew), Key: "new"})
		}
		for _, item := range []struct {
			area  SidebarArea
			label string
			glyph icon.Name
		}{{AreaWork, "工作区", icon.Chat}, {AreaPartners, "伙伴", icon.Users}, {AreaSettings, "设置", icon.Settings}} {
			if labels {
				NavItem(c, item.label, NavItemProps{Icon: item.glyph, Selected: p.Area == item.area, OnClick: pick(item.area), Key: "nav-" + item.label})
			} else {
				IconButton(c, item.glyph, item.label, IconButtonProps{Selected: p.Area == item.area, OnClick: pick(item.area), Key: "nav-" + item.label})
			}
		}
	})
}

// SettingsCategory is one entry of SettingsNav.
type SettingsCategory struct {
	Label string
	Icon  icon.Name
}

// SettingsNavProps configure SettingsNav.
type SettingsNavProps struct {
	// Label names the navigation for assistive technology; it defaults to 设置导航.
	Label string
	// Selected indexes Categories.
	Selected int
	OnSelect func(int)
	Key      string
}

// SettingsNav is the 208px settings category list.
func SettingsNav(c *ui.Context, categories []SettingsCategory, p SettingsNavProps) ui.Element {
	label := p.Label
	if label == "" {
		label = "设置导航"
	}
	return ui.Column(keyed(c, p.Key)).Width(theme.SizeNavigation(false)).Shrink(0).Padding(theme.Space3).Gap(theme.Space1).
		Role(ui.RoleList).Label(label).FocusGroup(ui.Vertical).Children(func() {
		for i, cat := range categories {
			index := i
			NavItem(c, cat.Label, NavItemProps{Icon: cat.Icon, Selected: i == p.Selected, Key: "cat-" + cat.Label, OnClick: func() {
				if p.OnSelect != nil {
					p.OnSelect(index)
				}
			}})
		}
	})
}
