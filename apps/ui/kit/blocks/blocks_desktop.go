// INPUT: Alva RunStatus, TaskRow, ConnectionRow, ModelRow, MessageHeader, Citation, Attachment and CodeBlock definitions plus kit components.
// OUTPUT: Presentation blocks driven by view models defined here: enums for kinds and states, plain strings for content, callbacks for actions.
// POS: [L7 Kit blocks] Imports only kit and MyGo; never a product, engine or tool type. Mapping product facts to these enums is the product layer's job (K4).
//go:build desktop

package blocks

import (
	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// Geometry without a design token of its own.
const (
	chipHeight     = theme.ControlHeight
	chipMaxWidth   = 240
	roundAction    = theme.ControlHeight
	cardMaxWidth   = 560
	citationNumber = theme.Space3
	dimmed         = .6
)

func pal(c *ui.Context) theme.Palette { return theme.Of(c) }

func col(c theme.Color) ui.Color { return theme.UI(c) }

func keyed(c *ui.Context, key string) *ui.Context {
	if key == "" {
		return c
	}
	return c.Key(key)
}

// RunState is the state of a run as RunStatus shows it; Working is the zero value.
type RunState uint8

const (
	RunWorking RunState = iota
	RunWaiting
	RunStopping
	RunStopped
	RunDone
	RunError
)

// RunStatusView is the data of RunStatus.
type RunStatusView struct {
	State RunState
	// Detail is optional secondary text (a step name, an elapsed time).
	Detail string
}

func runLabel(s RunState) string {
	switch s {
	case RunWaiting:
		return "等待确认"
	case RunStopping:
		return "正在停止"
	case RunStopped:
		return "已停止"
	case RunDone:
		return "已完成"
	case RunError:
		return "运行失败"
	}
	return "正在工作"
}

// RunStatus is the one-line status of a run: indicator, label and optional detail.
func RunStatus(c *ui.Context, v RunStatusView, key string) ui.Element {
	label := runLabel(v.State)
	audit.Named("RunStatus", label)
	return ui.Row(keyed(c, key)).AlignItems(ui.Center).Gap(theme.Space2).Role(ui.RoleStatus).Label(label).Children(func() {
		switch v.State {
		case RunWorking, RunStopping:
			kit.Loader(c.Key("busy"), theme.IconSmall, kit.ToneAccent)
		case RunWaiting:
			kit.Icon(c, icon.Clock, kit.IconProps{Small: true, Tone: kit.ToneWarning, Key: "g"})
		case RunStopped:
			kit.Icon(c, icon.Stop, kit.IconProps{Small: true, Tone: kit.ToneSecondary, Key: "g"})
		case RunDone:
			kit.Icon(c, icon.Check, kit.IconProps{Small: true, Tone: kit.ToneSuccess, Key: "g"})
		case RunError:
			kit.Icon(c, icon.Warning, kit.IconProps{Small: true, Tone: kit.ToneDanger, Key: "g"})
		}
		tone := kit.ToneText
		if v.State == RunError {
			tone = kit.ToneDanger
		}
		kit.Text(c, label, kit.TextProps{Tone: tone, SingleLine: true, Key: "label"})
		if v.Detail != "" {
			kit.Text(c, v.Detail, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, SingleLine: true, Key: "detail"})
		}
	})
}

// TaskState is the state of a task row; Running is the zero value.
type TaskState uint8

const (
	TaskRunning TaskState = iota
	TaskWaiting
	TaskDone
	TaskFailed
	TaskCancelled
	TaskQueued
)

// TaskRowView is the data of TaskRow.
type TaskRowView struct {
	Title, Detail string
	State         TaskState
}

func taskLabel(s TaskState) string {
	switch s {
	case TaskWaiting:
		return "等待中"
	case TaskDone:
		return "已完成"
	case TaskFailed:
		return "失败"
	case TaskCancelled:
		return "已取消"
	case TaskQueued:
		return "排队中"
	}
	return "进行中"
}

func taskTone(s TaskState) kit.BadgeTone {
	switch s {
	case TaskRunning:
		return kit.BadgeAccent
	case TaskWaiting:
		return kit.BadgeWarning
	case TaskDone:
		return kit.BadgeSuccess
	case TaskFailed:
		return kit.BadgeDanger
	}
	return kit.BadgeNeutral
}

// TaskRow lists one task with its state; onClick opens it (nil makes the row informational).
func TaskRow(c *ui.Context, v TaskRowView, onClick func(), key string) ui.Element {
	tone := taskTone(v.State)
	return kit.ListRow(c, v.Title, kit.ListRowProps{Description: v.Detail, OnClick: onClick, Key: key,
		Leading: func() {
			switch v.State {
			case TaskRunning:
				kit.Loader(c.Key("busy"), theme.IconSmall, kit.ToneAccent)
			case TaskWaiting:
				kit.Icon(c, icon.Clock, kit.IconProps{Small: true, Tone: kit.ToneWarning, Key: "g"})
			case TaskDone:
				kit.Icon(c, icon.Check, kit.IconProps{Small: true, Tone: kit.ToneSuccess, Key: "g"})
			case TaskFailed:
				kit.Icon(c, icon.Warning, kit.IconProps{Small: true, Tone: kit.ToneDanger, Key: "g"})
			case TaskCancelled:
				kit.Icon(c, icon.Close, kit.IconProps{Small: true, Tone: kit.ToneSecondary, Key: "g"})
			default:
				kit.Icon(c, icon.List, kit.IconProps{Small: true, Tone: kit.ToneSecondary, Key: "g"})
			}
		},
		Trailing: func() { kit.Badge(c, taskLabel(v.State), kit.BadgeProps{Tone: tone, Key: "badge"}) }})
}

// ConnectionState is the state of a provider connection; Default is connected and unselected.
type ConnectionState uint8

const (
	ConnectionDefault ConnectionState = iota
	ConnectionSelected
	ConnectionDisconnected
	ConnectionError
)

// ConnectionRowView is the data of ConnectionRow.
type ConnectionRowView struct {
	Name, Detail string
	State        ConnectionState
}

// ConnectionRow lists a model provider connection with its health.
func ConnectionRow(c *ui.Context, v ConnectionRowView, onClick func(), key string) ui.Element {
	return kit.ListRow(c, v.Name, kit.ListRowProps{Description: v.Detail, Selected: v.State == ConnectionSelected, OnClick: onClick, Key: key,
		Leading: func() { kit.Avatar(c, kit.AvatarProps{Kind: kit.AvatarProvider, Name: v.Name, Key: "avatar"}) },
		Trailing: func() {
			switch v.State {
			case ConnectionDisconnected:
				kit.Badge(c, "未连接", kit.BadgeProps{Tone: kit.BadgeNeutral, Key: "badge"})
			case ConnectionError:
				kit.Badge(c, "出错", kit.BadgeProps{Tone: kit.BadgeDanger, Key: "badge"})
			default:
				kit.Badge(c, "已连接", kit.BadgeProps{Tone: kit.BadgeSuccess, Key: "badge"})
			}
		}})
}

// ModelRowMode selects the picker or the management form of a model row; Picker is the zero value.
type ModelRowMode uint8

const (
	ModelPicker ModelRowMode = iota
	ModelManage
)

// ModelRowView is the data of ModelRow.
type ModelRowView struct {
	Name, Description string
	Selected          bool
	// Unavailable shows the model dimmed and inert (picker) with a badge.
	Unavailable bool
	Favorite    bool
}

// ModelRowActions are the callbacks of ModelRow; nil ones hide their control.
type ModelRowActions struct {
	OnSelect, OnToggleFavorite, OnEdit func()
}

// ModelRow lists a model. The picker selects on click and checks the chosen row; the manage form adds a
// favorite toggle and an edit action instead.
func ModelRow(c *ui.Context, v ModelRowView, mode ModelRowMode, a ModelRowActions, key string) ui.Element {
	pick := a.OnSelect
	if mode == ModelManage {
		pick = nil
	}
	return kit.ListRow(c, v.Name, kit.ListRowProps{Description: v.Description, Selected: v.Selected && mode == ModelPicker, Disabled: v.Unavailable && mode == ModelPicker, OnClick: pick, Key: key,
		Leading: func() { kit.Icon(c, icon.Chip, kit.IconProps{Small: true, Tone: kit.ToneSecondary, Key: "g"}) },
		Trailing: func() {
			if v.Unavailable {
				kit.Badge(c, "不可用", kit.BadgeProps{Tone: kit.BadgeNeutral, Key: "badge"})
			}
			if mode == ModelManage {
				if a.OnToggleFavorite != nil {
					kit.InlineAction(c, icon.Star, "收藏 "+v.Name, kit.InlineActionProps{Selected: v.Favorite, OnClick: a.OnToggleFavorite, Key: "fav"})
				}
				if a.OnEdit != nil {
					kit.InlineAction(c, icon.Edit, "编辑 "+v.Name, kit.InlineActionProps{OnClick: a.OnEdit, Key: "modify"})
				}
				return
			}
			if v.Selected {
				kit.Icon(c, icon.Check, kit.IconProps{Small: true, Tone: kit.ToneAccent, Key: "check"})
			}
		}})
}

// AuthorKind is who wrote a message; Assistant is the zero value.
type AuthorKind uint8

const (
	AuthorAssistant AuthorKind = iota
	AuthorUser
	AuthorPartner
)

// MessageHeaderView is the data of MessageHeader.
type MessageHeaderView struct {
	Author AuthorKind
	Name   string
	// Meta is secondary text such as a time.
	Meta string
}

// MessageHeader is the avatar, name and meta line above a message.
func MessageHeader(c *ui.Context, v MessageHeaderView, key string) ui.Element {
	kind := kit.AvatarAgent
	if v.Author == AuthorUser {
		kind = kit.AvatarPerson
	}
	return ui.Row(keyed(c, key)).AlignItems(ui.Center).Gap(theme.Space2).Children(func() {
		kit.Avatar(c, kit.AvatarProps{Kind: kind, Name: v.Name, Key: "avatar"})
		kit.Text(c, v.Name, kit.TextProps{Strong: true, SingleLine: true, Key: "name"})
		if v.Author == AuthorPartner {
			kit.Badge(c, "伙伴", kit.BadgeProps{Tone: kit.BadgeAccent, Key: "badge"})
		}
		if v.Meta != "" {
			kit.Text(c, v.Meta, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, SingleLine: true, Key: "meta"})
		}
	})
}

// CitationView is the data of Citation.
type CitationView struct {
	// Index is the 1-based footnote number.
	Index  int
	Title  string
	Source string
	// Unavailable shows a source that can no longer be opened.
	Unavailable bool
}

// Citation is a compact source chip: a footnote number and a title; onOpen opens the source.
func Citation(c *ui.Context, v CitationView, onOpen func(), key string) ui.Element {
	audit.Named("Citation", v.Title)
	pl, fs := pal(c), force.Take()
	b := ui.ButtonBase(keyed(c, key)).Label(v.Title).Height(theme.Space6).Padding(0, theme.Space2).Gap(theme.Space1).Radius(theme.RadiusPill).AlignItems(ui.Center).
		AlignSelf(ui.Start).MaxWidth(chipMaxWidth).FocusRing(false).Disabled(v.Unavailable)
	if v.Source != "" {
		b.Description(v.Source)
	}
	fill, text := col(pl.Subtle), col(pl.Text)
	switch {
	case v.Unavailable:
		text = col(pl.Secondary)
	case fs.Hovered(b.Hovered()):
		fill = col(pl.Hover)
	}
	b.Background(fill).TextColor(text)
	focused, accent := !v.Unavailable && fs.Focused(b.FocusVisible()), col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			kit.InlineFocus(pt, r, theme.RadiusPill, accent)
		}
	})
	if onOpen != nil {
		off := v.Unavailable
		b.OnClick(func() {
			if !off {
				onOpen()
			}
		})
	}
	return b.Children(func() {
		kit.Text(c, itoa(v.Index), kit.TextProps{Role: kit.RoleCaption, Strong: true, Tone: kit.ToneAccent, Key: "n"}).Width(citationNumber)
		kit.Text(c, v.Title, kit.TextProps{Role: kit.RoleCaption, SingleLine: true, Key: "title"}).MinWidth(0)
	})
}

// AttachmentKind is what an attachment is; File is the zero value.
type AttachmentKind uint8

const (
	AttachmentFile AttachmentKind = iota
	AttachmentImage
	AttachmentFolder
)

// AttachmentState is the lifecycle of an attachment; Ready is the zero value.
type AttachmentState uint8

const (
	AttachmentReady AttachmentState = iota
	AttachmentUploading
	AttachmentError
	AttachmentUnavailable
)

// AttachmentView is the data of Attachment.
type AttachmentView struct {
	Name, Detail string
	Kind         AttachmentKind
	State        AttachmentState
}

// Attachment is a file chip with its state and an optional remove action (onRemove nil hides it).
func Attachment(c *ui.Context, v AttachmentView, onRemove func(), key string) ui.Element {
	audit.Named("Attachment", v.Name)
	pl := pal(c)
	glyph := icon.File
	switch v.Kind {
	case AttachmentImage:
		glyph = icon.Image
	case AttachmentFolder:
		glyph = icon.Folder
	}
	border := col(pl.Border)
	if v.State == AttachmentError {
		border = col(pl.Danger)
	}
	row := ui.Row(keyed(c, key)).MinHeight(chipHeight).Padding(theme.Space1, theme.Space2).Gap(theme.Space2).AlignItems(ui.Center).AlignSelf(ui.Start).MaxWidth(chipMaxWidth).
		Radius(theme.RadiusCtrl).Background(col(pl.Subtle)).Border(1, border).Label(v.Name)
	if v.State == AttachmentUnavailable {
		row.Opacity(dimmed)
	}
	return row.Children(func() {
		switch v.State {
		case AttachmentUploading:
			kit.Loader(c.Key("busy"), theme.IconSmall, kit.ToneAccent)
		case AttachmentError:
			kit.Icon(c, icon.Warning, kit.IconProps{Small: true, Tone: kit.ToneDanger, Key: "g"})
		case AttachmentUnavailable:
			kit.Icon(c, icon.Lock, kit.IconProps{Small: true, Tone: kit.ToneSecondary, Key: "g"})
		default:
			kit.Icon(c, glyph, kit.IconProps{Small: true, Key: "g"})
		}
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Children(func() {
			kit.Text(c, v.Name, kit.TextProps{SingleLine: true, Key: "name"})
			if v.Detail != "" {
				tone := kit.ToneSecondary
				if v.State == AttachmentError {
					tone = kit.ToneDanger
				}
				kit.Text(c, v.Detail, kit.TextProps{Role: kit.RoleCaption, Tone: tone, SingleLine: true, Key: "detail"})
			}
		})
		if onRemove != nil {
			kit.InlineAction(c, icon.Close, "移除 "+v.Name, kit.InlineActionProps{OnClick: onRemove, Key: "remove"})
		}
	})
}

// CopyState is the state of a code block's copy action; Default is ready to copy.
type CopyState uint8

const (
	CopyDefault CopyState = iota
	CopyCopied
	CopyFailed
)

// CodeBlockView is the data of CodeBlock.
type CodeBlockView struct {
	Language, Code string
	Copy           CopyState
}

// CodeBlock shows code in the mono font with a language label and a copy action; onCopy performs the copy and the
// caller reports the result back through Copy (so the block never claims success by itself).
func CodeBlock(c *ui.Context, v CodeBlockView, onCopy func(), key string) ui.Element {
	pl := pal(c)
	label, glyph, kind := "复制", icon.Copy, kit.Ghost
	switch v.Copy {
	case CopyCopied:
		label, glyph = "已复制", icon.Check
	case CopyFailed:
		label, glyph, kind = "复制失败", icon.Warning, kit.Danger
	}
	return ui.Column(keyed(c, key)).FillWidth().Radius(theme.RadiusCtrl).Background(col(pl.Subtle)).Border(1, col(pl.Border)).Clip().Children(func() {
		ui.Row(c.Key("head")).FillWidth().Padding(0, theme.Space1, 0, theme.Space3).AlignItems(ui.Center).Children(func() {
			kit.Text(c, v.Language, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "lang"}).Grow(1)
			kit.Button(c, label, kit.ButtonProps{Kind: kind, Icon: glyph, OnClick: onCopy, Key: "copy"})
		})
		ui.ScrollHorizontal(c.Key("body")).FillWidth().Children(func() {
			kit.Text(c, v.Code, kit.TextProps{Role: kit.RoleMono, Key: "code"}).Padding(theme.Space1, theme.Space3, theme.Space3).Selectable()
		})
	})
}
