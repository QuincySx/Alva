// INPUT: Alva NavItem, TreeRow, SkillRow, ResourceRow, CompanionRow and SettingRow definitions plus the list recipe.
// OUTPUT: ListRow, the shared interactive row (hover, selected, focus, disabled), and the row components built on it.
// POS: [L7 Kit] Rows are presentation: selection and expansion state belong to the caller, activation goes through gated OnClick.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// ListRowProps configure ListRow.
type ListRowProps struct {
	// Leading and Trailing build the slots; either may be nil.
	Leading, Trailing  func()
	Description        string
	Selected, Disabled bool
	// Dense uses the 28px navigation row height instead of the 32px list row.
	Dense bool
	// Indent shifts the content in steps of space.4 (tree levels).
	Indent  int
	OnClick func()
	Key     string
}

// ListRow is a full-width activatable row: label (and description) with optional leading and trailing slots.
func ListRow(c *ui.Context, title string, p ListRowProps) ui.Element {
	audit.Named("ListRow", title)
	pl, fs := pal(c), force.Take()
	height := float32(theme.ControlHeight)
	if p.Dense {
		height = theme.ControlRow
	}
	// Without OnClick the row is information, not a control: no button role, no tab stop, no hover.
	interactive := p.OnClick != nil
	var b ui.Element
	if interactive {
		b = ui.ButtonBase(keyed(c, p.Key)).FocusRing(false)
	} else {
		b = ui.Row(keyed(c, p.Key))
	}
	b.Label(title).Role(ui.RoleListItem).FillWidth().MinHeight(height).Padding(theme.Space1, theme.Space2).
		PaddingStart(theme.Space2 + float32(p.Indent)*theme.Space4).Gap(theme.Space2).Radius(theme.RadiusCtrl).AlignItems(ui.Center).Selected(p.Selected).Disabled(p.Disabled)
	if p.Description != "" {
		b.Description(p.Description)
	}
	text := col(pl.Text)
	switch {
	case p.Disabled:
		text = col(pl.Secondary)
		if p.Selected {
			b.Background(col(pl.Hover))
		}
	case p.Selected, interactive && fs.Pressed(b.Pressed()):
		b.Background(col(pl.Hover))
	case interactive && fs.Hovered(b.Hovered()):
		b.Background(col(pl.Subtle))
	}
	b.TextColor(text)
	focused := interactive && !p.Disabled && fs.Focused(b.FocusVisible())
	accent := col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			inlineFocus(pt, r, theme.RadiusCtrl, accent)
		}
	})
	if p.OnClick != nil {
		off, action := p.Disabled, p.OnClick
		b.OnClick(func() {
			if !off {
				action()
			}
		})
	}
	return b.Children(func() {
		if p.Leading != nil {
			p.Leading()
		}
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Children(func() {
			Text(c, title, TextProps{SingleLine: true, Key: "title"})
			if p.Description != "" {
				Text(c, p.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary, SingleLine: true, Key: "desc"})
			}
		})
		if p.Trailing != nil {
			p.Trailing()
		}
	})
}

// NavItemProps configure NavItem.
type NavItemProps struct {
	Icon               icon.Name
	Selected, Disabled bool
	OnClick            func()
	Key                string
}

// NavItem is a 28px navigation row with an icon and a label.
func NavItem(c *ui.Context, label string, p NavItemProps) ui.Element {
	return ListRow(c, label, ListRowProps{Dense: true, Selected: p.Selected, Disabled: p.Disabled, OnClick: p.OnClick, Key: p.Key, Leading: func() {
		Icon(c, p.Icon, IconProps{Small: true, Key: "g"})
	}})
}

// TreeKind is the design Kind axis of TreeRow; Folder is the zero value.
type TreeKind uint8

const (
	TreeFolder TreeKind = iota
	TreeFile
)

// TreeRowProps configure TreeRow.
type TreeRowProps struct {
	Kind TreeKind
	// Level is the nesting depth; the caller manages expansion and loading.
	Level    int
	Expanded bool
	Selected bool
	// Unavailable shows the entry dimmed and inert (a missing or unreadable path).
	Unavailable bool
	OnClick     func()
	Key         string
}

// TreeRow is a file or folder row; a folder shows an expansion chevron.
func TreeRow(c *ui.Context, label string, p TreeRowProps) ui.Element {
	return ListRow(c, label, ListRowProps{Dense: true, Indent: p.Level, Selected: p.Selected, Disabled: p.Unavailable, OnClick: p.OnClick, Key: p.Key, Leading: func() {
		chevron := icon.ChevronRight
		if p.Expanded {
			chevron = icon.ChevronDown
		}
		if p.Kind == TreeFolder {
			Icon(c, chevron, IconProps{Small: true, Tone: ToneSecondary, Key: "chevron"})
			Icon(c, icon.Folder, IconProps{Small: true, Key: "g"})
		} else {
			ui.Box(c.Key("indent")).Size(theme.IconSmall, theme.IconSmall).Shrink(0)
			Icon(c, icon.File, IconProps{Small: true, Key: "g"})
		}
	}})
}

// SkillState is the design State axis of SkillRow; Shared is the zero value.
type SkillState uint8

const (
	SkillShared SkillState = iota
	SkillOwned
	SkillUnavailable
)

// SkillRowProps configure SkillRow.
type SkillRowProps struct {
	State       SkillState
	Description string
	OnClick     func()
	Key         string
}

// SkillRow lists a skill with its ownership as a badge.
func SkillRow(c *ui.Context, title string, p SkillRowProps) ui.Element {
	label, tone := "共享", BadgeAccent
	switch p.State {
	case SkillOwned:
		label, tone = "自有", BadgeSuccess
	case SkillUnavailable:
		label, tone = "不可用", BadgeNeutral
	}
	return ListRow(c, title, ListRowProps{Description: p.Description, Disabled: p.State == SkillUnavailable, OnClick: p.OnClick, Key: p.Key,
		Leading:  func() { Icon(c, icon.Sparkles, IconProps{Small: true, Key: "g"}) },
		Trailing: func() { Badge(c, label, BadgeProps{Tone: tone, Key: "badge"}) }})
}

// ResourceKind is the design Kind axis of ResourceRow; File is the zero value.
type ResourceKind uint8

const (
	ResourceFile ResourceKind = iota
	ResourceDirectory
	ResourceLink
	ResourceMedia
)

// ResourceRowProps configure ResourceRow.
type ResourceRowProps struct {
	Kind ResourceKind
	// Detail is the path, address or size shown as the description.
	Detail  string
	OnClick func()
	Key     string
}

// ResourceRow lists a file, directory, link or media resource.
func ResourceRow(c *ui.Context, title string, p ResourceRowProps) ui.Element {
	glyph := map[ResourceKind]icon.Name{ResourceFile: icon.File, ResourceDirectory: icon.Folder, ResourceLink: icon.Link, ResourceMedia: icon.Image}[p.Kind]
	return ListRow(c, title, ListRowProps{Description: p.Detail, OnClick: p.OnClick, Key: p.Key, Leading: func() { Icon(c, glyph, IconProps{Small: true, Key: "g"}) }})
}

// CompanionState is the design State axis of CompanionRow; Default is the zero value.
type CompanionState uint8

const (
	CompanionDefault CompanionState = iota
	CompanionSelected
	CompanionPaused
	CompanionError
)

// CompanionRowProps configure CompanionRow.
type CompanionRowProps struct {
	State       CompanionState
	Description string
	OnClick     func()
	Key         string
}

// CompanionRow lists a long-lived companion with an avatar and a status badge.
func CompanionRow(c *ui.Context, name string, p CompanionRowProps) ui.Element {
	return ListRow(c, name, ListRowProps{Description: p.Description, Selected: p.State == CompanionSelected, OnClick: p.OnClick, Key: p.Key,
		Leading: func() { Avatar(c, AvatarProps{Kind: AvatarAgent, Name: name, Key: "avatar"}) },
		Trailing: func() {
			switch p.State {
			case CompanionPaused:
				Badge(c, "已暂停", BadgeProps{Tone: BadgeWarning, Key: "badge"})
			case CompanionError:
				Badge(c, "出错", BadgeProps{Tone: BadgeDanger, Key: "badge"})
			}
		}})
}

// SettingRowProps configure SettingRow.
type SettingRowProps struct {
	Title, Description string
	Key                string
}

// SettingRow lays out a setting's title and description with its control on the right (a Switch, a Ghost Button or a Select).
func SettingRow(c *ui.Context, p SettingRowProps, control func()) ui.Element {
	return ui.Row(keyed(c, p.Key)).FillWidth().Padding(theme.Space2, 0).Gap(theme.Space4).AlignItems(ui.Center).Children(func() {
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Gap(theme.Space1).Children(func() {
			Text(c, p.Title, TextProps{Key: "title"})
			if p.Description != "" {
				Text(c, p.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary, Key: "desc"})
			}
		})
		control()
	})
}
