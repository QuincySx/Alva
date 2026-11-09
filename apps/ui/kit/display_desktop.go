// INPUT: Alva Badge, Avatar, Notice, EmptyState, Skeleton, Progress, Step, Pagination, DetailRow and Toast definitions plus Divider/Card recipes.
// OUTPUT: Presentation components with no interaction of their own: tones from the palette, one density, no color or size literals.
// POS: [L7 Kit] K2 feedback and data display; actions are caller-supplied (Button, InlineAction), never implied by the component.
//go:build desktop

package kit

import (
	"strconv"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// Geometry without a design token of its own.
const (
	avatarSize       = theme.ControlHeight
	stepSize         = theme.Space6
	progressBar      = theme.Space2
	detailLabelWidth = 96
	emptyIconSize    = theme.Space8
	noticeIconSlot   = theme.IconSize
	percentMax       = 100
	avatarUnknown    = "?"
)

// Tone-style pair for tinted surfaces.
func toneSurface(pl theme.Palette, t BadgeTone) (fill, text ui.Color) {
	switch t {
	case BadgeAccent:
		return col(pl.AccentSoft), col(pl.Accent)
	case BadgeSuccess:
		return col(pl.SuccessBg), col(pl.Success)
	case BadgeWarning:
		return col(pl.WarningBg), col(pl.Warning)
	case BadgeDanger:
		return col(pl.DangerBg), col(pl.Danger)
	case BadgeInfo:
		return col(pl.Hover), col(pl.Text)
	}
	return col(pl.Subtle), col(pl.Secondary)
}

// BadgeTone is the design Tone axis; Neutral is the zero value.
type BadgeTone uint8

const (
	BadgeNeutral BadgeTone = iota
	BadgeAccent
	BadgeSuccess
	BadgeWarning
	BadgeDanger
	BadgeInfo
)

// BadgeProps configure Badge.
type BadgeProps struct {
	Tone BadgeTone
	Key  string
}

// Badge draws a pill with caption text.
func Badge(c *ui.Context, text string, p BadgeProps) ui.Element {
	fill, color := toneSurface(pal(c), p.Tone)
	return ui.Row(keyed(c, p.Key)).AlignItems(ui.Center).Padding(0, theme.Space2).Height(theme.Space6).Radius(theme.RadiusPill).Background(fill).TextColor(color).Shrink(0).
		AlignSelf(ui.Start).Children(func() { Text(c, text, TextProps{Role: RoleCaption, Strong: true, SingleLine: true, Key: "t"}) })
}

// AvatarKind is the design Kind axis; Person is the zero value.
type AvatarKind uint8

const (
	AvatarPerson AvatarKind = iota
	AvatarAgent
	AvatarProvider
)

// String is the design Kind axis value of the kind.
func (k AvatarKind) String() string {
	switch k {
	case AvatarAgent:
		return "Agent"
	case AvatarProvider:
		return "Provider"
	}
	return "Person"
}

// AvatarProps configure Avatar.
type AvatarProps struct {
	Kind AvatarKind
	// Name provides the initial of a person and the accessible name.
	Name string
	Key  string
}

// Avatar draws a 32px identity mark: a person's initial, an agent's bot glyph or a provider's rounded square.
func Avatar(c *ui.Context, p AvatarProps) ui.Element {
	audit.Named("Avatar", p.Name)
	pl := pal(c)
	e := ui.Box(keyed(c, p.Key)).Size(avatarSize, avatarSize).Shrink(0).Center().Radius(theme.RadiusPill).Background(col(pl.Subtle)).TextColor(col(pl.Secondary))
	if p.Name != "" {
		e.Label(p.Name)
	}
	return e.Children(func() {
		switch p.Kind {
		case AvatarAgent:
			e.Background(col(pl.AccentSoft)).TextColor(col(pl.Accent))
			Icon(c, icon.Agent, IconProps{Key: "g"})
		case AvatarProvider:
			e.Radius(theme.RadiusCtrl).Background(col(pl.Hover)).TextColor(col(pl.Text))
			Icon(c, icon.Cloud, IconProps{Key: "g"})
		default:
			initial := avatarUnknown
			if r := []rune(p.Name); len(r) > 0 {
				initial = string(r[0])
			}
			Text(c, initial, TextProps{Strong: true, Key: "i"})
		}
	})
}

// NoticeProps configure Notice. Tone reuses the badge tones Info, Success, Warning and Danger.
type NoticeProps struct {
	Tone               BadgeTone
	Title, Description string
	// Action is an optional trailing control (a Button); OnClose adds a close InlineAction.
	Action  func()
	OnClose func()
	Key     string
}

// Notice is an inline message surface with a tone icon, title and description.
func Notice(c *ui.Context, p NoticeProps) ui.Element {
	pl := pal(c)
	fill, color := toneSurface(pl, p.Tone)
	glyph := icon.Info
	switch p.Tone {
	case BadgeSuccess:
		glyph = icon.Check
	case BadgeWarning, BadgeDanger:
		glyph = icon.Warning
	}
	return ui.Row(keyed(c, p.Key)).FillWidth().Padding(theme.Space3).Gap(theme.Space3).Radius(theme.RadiusCtrl).Background(fill).Role(ui.RoleStatus).Label(p.Title).Children(func() {
		Icon(c, glyph, IconProps{Key: "g"}).TextColor(color).Shrink(0)
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Gap(theme.Space1).Children(func() {
			Text(c, p.Title, TextProps{Strong: true, Key: "title"}).TextColor(color)
			if p.Description != "" {
				Text(c, p.Description, TextProps{Role: RoleCaption, Tone: ToneText, Key: "desc"})
			}
		})
		if p.Action != nil {
			p.Action()
		}
		if p.OnClose != nil {
			InlineAction(c, icon.Close, "关闭提示", InlineActionProps{OnClick: p.OnClose, Key: "close"})
		}
	})
}

// EmptyKind is the design State axis of EmptyState; Empty is the zero value.
type EmptyKind uint8

const (
	EmptyNone EmptyKind = iota
	EmptyLoading
	EmptyError
	EmptyUnavailable
)

// EmptyStateProps configure EmptyState.
type EmptyStateProps struct {
	Kind               EmptyKind
	Title, Description string
	// Action is an optional control under the text (a Button).
	Action func()
	Key    string
}

// EmptyState centers an icon, a title, a description and an optional action in the space it is given.
func EmptyState(c *ui.Context, p EmptyStateProps) ui.Element {
	pl := pal(c)
	glyph, tone := icon.Folder, ToneSecondary
	switch p.Kind {
	case EmptyError:
		glyph, tone = icon.Warning, ToneDanger
	case EmptyUnavailable:
		glyph, tone = icon.Lock, ToneSecondary
	}
	return ui.Column(keyed(c, p.Key)).FillWidth().Center().Gap(theme.Space2).Padding(theme.Space6).Children(func() {
		if p.Kind == EmptyLoading {
			loader(c.Key("busy"), emptyIconSize/2, col(pl.Secondary))
		} else {
			Icon(c, glyph, IconProps{Tone: tone, Key: "g"})
		}
		Text(c, p.Title, TextProps{Role: RoleSubtitle, Strong: true, Key: "title"})
		if p.Description != "" {
			Text(c, p.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary, Key: "desc"}).TextAlign(ui.Center)
		}
		if p.Action != nil {
			p.Action()
		}
	})
}

// Skeleton is a placeholder bar; it is still, so reduced motion needs no special case.
func Skeleton(c *ui.Context, width, height float32) ui.Element {
	return ui.Box(c).Width(width).Height(height).Radius(theme.RadiusSmall).Background(col(pal(c).Subtle)).Shrink(0)
}

// SkeletonGroup announces one "加载中" for all the bars it holds, instead of every bar announcing itself.
func SkeletonGroup(c *ui.Context, key string, bars func()) ui.Element {
	return ui.Column(keyed(c, key)).Gap(theme.Space2).Role(ui.RoleStatus).Label("加载中").Children(bars)
}

// ProgressState is the design State axis of Progress; Running is the zero value.
type ProgressState uint8

const (
	ProgressRunning ProgressState = iota
	ProgressDone
	ProgressError
)

// ProgressProps configure Progress.
type ProgressProps struct {
	Title string
	// Percent is 0..100; values outside are clamped.
	Percent float32
	State   ProgressState
	Key     string
}

// Progress draws a title with its percent and a pill bar whose fill follows the state.
func Progress(c *ui.Context, p ProgressProps) ui.Element {
	pl := pal(c)
	pct := max(0, min(percentMax, p.Percent))
	fill, tone := col(pl.Accent), ToneInherit
	switch p.State {
	case ProgressDone:
		fill, tone = col(pl.Success), ToneSuccess
	case ProgressError:
		fill, tone = col(pl.Danger), ToneDanger
	}
	// Color is never the only signal: the state is also the description.
	state := map[ProgressState]string{ProgressRunning: "进行中", ProgressDone: "已完成", ProgressError: "失败"}[p.State]
	audit.Described("Progress", state)
	e := ui.Column(keyed(c, p.Key)).FillWidth().Gap(theme.Space1).Role(ui.RoleProgress).Label(p.Title).Value(strconv.Itoa(int(pct)) + "%").Description(state)
	return e.Children(func() {
		ui.Row(c.Key("head")).FillWidth().Justify(ui.SpaceBetween).Children(func() {
			Text(c, p.Title, TextProps{SingleLine: true, Key: "title"})
			Text(c, strconv.Itoa(int(pct))+"%", TextProps{Role: RoleCaption, Tone: tone, Key: "pct"})
			if p.State == ProgressError {
				Text(c, "失败", TextProps{Role: RoleCaption, Tone: tone, Key: "state"})
			}
		})
		ui.Row(c.Key("track")).FillWidth().Height(progressBar).Radius(theme.RadiusPill).Background(col(pl.Subtle)).Clip().Children(func() {
			ui.Box(c.Key("fill")).WidthPercent(pct).Height(progressBar).Background(fill)
		})
	})
}

// Divider is a 1px border-colored line; Vertical draws it across a row.
func Divider(c *ui.Context, vertical bool) ui.Element {
	e := ui.Box(c).Background(col(pal(c).Border)).Shrink(0)
	if vertical {
		return e.Width(1).FillHeight()
	}
	return e.Height(1).FillWidth()
}

// Card is the content container: surface, 1px border, card radius, space.4 padding and gap.
func Card(c *ui.Context, key string, content func()) ui.Element {
	pl := pal(c)
	return ui.Column(keyed(c, key)).FillWidth().Padding(theme.Space4).Gap(theme.Space4).Radius(theme.RadiusCard).Background(col(pl.Surface)).Border(1, col(pl.Border)).Children(content)
}

// StepState is the design State axis of Step; Current is the zero value.
type StepState uint8

const (
	StepCurrent StepState = iota
	StepDone
	StepNext
)

// StepProps configure Step.
type StepProps struct {
	// Index is the 1-based number shown until the step is done.
	Index int
	State StepState
	Key   string
}

// Step draws a numbered circle and its label; Done shows a check.
func Step(c *ui.Context, label string, p StepProps) ui.Element {
	pl := pal(c)
	fill, border, text := col(pl.Subtle), col(pl.Border), col(pl.Secondary)
	switch p.State {
	case StepCurrent:
		fill, border, text = col(pl.Accent), col(pl.Accent), col(pl.OnAccent)
	case StepDone:
		fill, border, text = col(pl.AccentSoft), col(pl.Accent), col(pl.Accent)
	}
	return ui.Row(keyed(c, p.Key)).AlignItems(ui.Center).Gap(theme.Space2).Label(label).Children(func() {
		ui.Box(c.Key("dot")).Size(stepSize, stepSize).Shrink(0).Center().Radius(theme.RadiusPill).Background(fill).Border(1, border).TextColor(text).Children(func() {
			if p.State == StepDone {
				Icon(c, icon.Check, IconProps{Small: true, Key: "check"})
			} else {
				Text(c, strconv.Itoa(p.Index), TextProps{Role: RoleCaption, Strong: true, Key: "n"})
			}
		})
		tone := ToneSecondary
		if p.State == StepCurrent {
			tone = ToneText
		}
		Text(c, label, TextProps{Tone: tone, Strong: p.State == StepCurrent, SingleLine: true, Key: "label"})
	})
}

// PaginationState is the design State axis of Pagination; Ready is the zero value.
type PaginationState uint8

const (
	PaginationReady PaginationState = iota
	PaginationLoading
	PaginationEnd
)

// PaginationProps configure Pagination.
type PaginationProps struct {
	State PaginationState
	// OnMore loads the next page; it is not called while loading.
	OnMore func()
	Key    string
}

// Pagination is the load-more footer of a list: an action, its busy form, or the end marker.
func Pagination(c *ui.Context, p PaginationProps) ui.Element {
	return ui.Row(keyed(c, p.Key)).FillWidth().Center().Padding(theme.Space2).Children(func() {
		if p.State == PaginationEnd {
			Text(c, "没有更多了", TextProps{Role: RoleCaption, Tone: ToneSecondary, Key: "end"})
			return
		}
		Button(c, "加载更多", ButtonProps{Kind: Ghost, Busy: p.State == PaginationLoading, OnClick: p.OnMore, Key: "more"})
	})
}

// DetailRow shows a label and a selectable value on one row.
func DetailRow(c *ui.Context, label, value string) ui.Element {
	return ui.Row(c).FillWidth().Gap(theme.Space3).AlignItems(ui.Start).Children(func() {
		Text(c, label, TextProps{Tone: ToneSecondary, Key: "label"}).Width(detailLabelWidth).Shrink(0)
		Text(c, value, TextProps{Key: "value"}).Grow(1).MinWidth(0).Selectable()
	})
}

// ToastTone is the design Tone axis of Toast; Success is the zero value.
type ToastTone uint8

const (
	ToastSuccess ToastTone = iota
	ToastError
	ToastInfo
)

var toastTypes = map[ToastTone]string{ToastSuccess: "success", ToastError: "error", ToastInfo: "info"}

func toastTone(kind string) ToastTone {
	for tone, name := range toastTypes {
		if name == kind {
			return tone
		}
	}
	return ToastInfo
}

// toastSurface styles a toast root: surface, 1px border, ctrl radius, padding and the popup shadow.
func toastSurface(e ui.Element, pl theme.Palette, message string) ui.Element {
	e.Row().AlignItems(ui.Center).Gap(theme.Space2).Padding(theme.Space2, theme.Space3).Radius(theme.RadiusCtrl).
		Background(col(pl.Surface)).Border(1, col(pl.Border)).TextColor(col(pl.Text)).Role(ui.RoleStatus).Label(message)
	return popupShadow(e, pl)
}

func toastContent(c *ui.Context, tone ToastTone, message string) {
	pl := pal(c)
	glyph, color := icon.Info, col(pl.Accent)
	switch tone {
	case ToastSuccess:
		glyph, color = icon.Check, col(pl.Success)
	case ToastError:
		glyph, color = icon.Warning, col(pl.Danger)
	}
	Icon(c, glyph, IconProps{Small: true, Key: "g"}).TextColor(color)
	Text(c, message, TextProps{Key: "m"})
}

// ToastView draws the design toast surface (tone icon, message, popup shadow) for galleries and previews.
// Real toasts are drawn by ToastHost with the same surface.
func ToastView(c *ui.Context, tone ToastTone, message string) ui.Element {
	return toastSurface(ui.Row(c), pal(c), message).Children(func() { toastContent(c, tone, message) })
}

// ShowToast queues a transient message. The message is the toast's Title, so there is no empty title row and no
// description dimming; ToastHost, built once at the app root, draws it.
func ShowToast(c *ui.Context, tone ToastTone, message string) {
	c.AddToast(ui.Toast{Title: message, Type: toastTypes[tone]})
}

// ToastHost draws the window's toasts with the kit appearance, bottom right: call it once in the root view.
// MyGo keeps stacking, time pausing on hover or focus, and the exit transition.
func ToastHost(c *ui.Context) ui.Element {
	return ui.ToastViewportBase(c, func(viewport ui.Element, toasts []ui.Toast) {
		viewport.Padding(theme.Space4).AlignItems(ui.End).Gap(theme.Space2)
		pl := pal(c)
		for _, t := range toasts {
			part := ui.ToastBase(c.Key(t.ID), t)
			toastSurface(part.Root, pl, t.Title)
			tone := toastTone(t.Type)
			part.Root.Children(func() { toastContent(c, tone, t.Title) })
		}
	})
}
