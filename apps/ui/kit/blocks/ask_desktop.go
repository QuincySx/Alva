// INPUT: Alva Approval, Question and QuestionOption definitions (Pending/Allowed/Denied/Expired, Pending/Submitted/Expired, six option states).
// OUTPUT: Approval and Question cards with a closed status enum, gated callbacks, busy exposure and keyboard-reachable named buttons.
// POS: [L7 Kit blocks] One-time consent, late-click rejection and branch ownership are the product layer's; the cards only refuse to fire twice and show busy.
//go:build desktop

package blocks

import (
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// AskStatus is the status of an approval or question card; Pending is the zero value.
type AskStatus uint8

const (
	AskPending AskStatus = iota
	// AskBusy means a decision was sent and its result is awaited: the buttons stay visible, named and focusable,
	// show the loader on the chosen one and do not fire again.
	AskBusy
	AskAllowed
	AskDenied
	AskAnswered
	AskExpired
	// AskError means the last attempt failed; the buttons are live again and the message is shown.
	AskError
)

// AskChoice says which decision is in flight while the card is busy.
type AskChoice uint8

const (
	ChoiceNone AskChoice = iota
	ChoiceAllow
	ChoiceDeny
)

// ApprovalView is the data of Approval.
type ApprovalView struct {
	// Title says what is asked ("run this command"); Detail is the exact thing in the mono font.
	Title, Detail string
	Status        AskStatus
	// Choice is the decision in flight while Status is AskBusy.
	Choice AskChoice
	// Message explains AskError.
	Message string
}

// ApprovalActions are the callbacks of Approval; they fire only while the card is pending or after an error.
type ApprovalActions struct {
	OnAllow, OnDeny func()
}

func (s AskStatus) live() bool { return s == AskPending || s == AskError }

func statusBadge(c *ui.Context, s AskStatus) {
	switch s {
	case AskAllowed:
		kit.Badge(c, "已允许", kit.BadgeProps{Tone: kit.BadgeSuccess, Key: "status"})
	case AskDenied:
		kit.Badge(c, "已拒绝", kit.BadgeProps{Tone: kit.BadgeNeutral, Key: "status"})
	case AskAnswered:
		kit.Badge(c, "已回答", kit.BadgeProps{Tone: kit.BadgeSuccess, Key: "status"})
	case AskExpired:
		kit.Badge(c, "已过期", kit.BadgeProps{Tone: kit.BadgeWarning, Key: "status"})
	case AskBusy:
		kit.Badge(c, "处理中", kit.BadgeProps{Tone: kit.BadgeAccent, Key: "status"})
	case AskError:
		kit.Badge(c, "出错", kit.BadgeProps{Tone: kit.BadgeDanger, Key: "status"})
	}
}

// approvalHandlers are the callbacks handed to the buttons: nil unless the card is live (pending, or after an error)
// and the caller supplied one. This is the business gate; kit.Button adds its own Busy and Disabled gate on top.
func approvalHandlers(v ApprovalView, a ApprovalActions) (allow, deny func()) {
	if !v.Status.live() {
		return nil, nil
	}
	return a.OnAllow, a.OnDeny
}

// Approval is the permission request card.
func Approval(c *ui.Context, v ApprovalView, a ApprovalActions, key string) ui.Element {
	audit.Named("Approval", v.Title)
	pl := pal(c)
	busy := v.Status == AskBusy
	return kit.Card(c, key, func() {
		ui.Row(c.Key("head")).FillWidth().AlignItems(ui.Center).Gap(theme.Space2).Children(func() {
			kit.Icon(c, icon.Shield, kit.IconProps{Tone: kit.ToneAccent, Key: "g"})
			kit.Text(c, v.Title, kit.TextProps{Strong: true, SingleLine: true, Key: "title"}).Grow(1).MinWidth(0)
			if v.Status != AskPending {
				statusBadge(c, v.Status)
			}
		})
		if v.Detail != "" {
			ui.Box(c.Key("detail")).FillWidth().Padding(theme.Space2, theme.Space3).Radius(theme.RadiusCtrl).Background(col(pl.Subtle)).Children(func() {
				kit.Text(c, v.Detail, kit.TextProps{Role: kit.RoleMono, Key: "text"}).Selectable()
			})
		}
		if v.Status == AskError && v.Message != "" {
			kit.Text(c, v.Message, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneDanger, Key: "message"})
		}
		if v.Status.live() || busy {
			ui.Row(c.Key("actions")).FillWidth().Justify(ui.End).Gap(theme.Space2).Children(func() {
				allow, deny := approvalHandlers(v, a)
				kit.Button(c, "拒绝", kit.ButtonProps{Busy: busy && v.Choice == ChoiceDeny, Disabled: busy && v.Choice != ChoiceDeny, OnClick: deny, Key: "deny"})
				kit.Button(c, "允许", kit.ButtonProps{Kind: kit.Primary, Busy: busy && v.Choice == ChoiceAllow, Disabled: busy && v.Choice != ChoiceAllow, OnClick: allow, Key: "allow"})
			})
		}
	}).MaxWidth(cardMaxWidth)
}

// OptionState is the interaction look of QuestionOption; the zero value is an enabled, unselected option.
type OptionView struct {
	Label, Description string
	Selected, Disabled bool
	// Multi draws a checkbox mark instead of a radio mark.
	Multi bool
}

// QuestionOption is one answer choice: a bordered row with a mark, a label and an optional description.
func QuestionOption(c *ui.Context, v OptionView, onToggle func(), key string) ui.Element {
	audit.Named("QuestionOption", v.Label)
	pl, fs := pal(c), force.Take()
	b := ui.ButtonBase(keyed(c, key)).Label(v.Label).Selected(v.Selected).FillWidth().MinHeight(theme.ControlHeight).Padding(theme.Space2, theme.Space3).Gap(theme.Space3).
		Radius(theme.RadiusCtrl).AlignItems(ui.Center).FocusRing(false).Disabled(v.Disabled)
	fill, border, text := col(pl.Surface), col(pl.Border), col(pl.Text)
	switch {
	case v.Selected:
		fill, border = col(pl.AccentSoft), col(pl.Accent)
	case !v.Disabled && fs.Hovered(b.Hovered()):
		fill = col(pl.Subtle)
	}
	if v.Disabled {
		text = col(pl.Secondary)
	}
	b.Background(fill).TextColor(text)
	focused, accent := !v.Disabled && fs.Focused(b.FocusVisible()), col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			kit.InlineFocus(pt, r, theme.RadiusCtrl, accent)
		} else {
			pt.Stroke(r, border, theme.RadiusCtrl, 1)
		}
	})
	if onToggle != nil {
		off := v.Disabled
		b.OnClick(func() {
			if !off {
				onToggle()
			}
		})
	}
	return b.Children(func() {
		mark := ui.Box(c.Key("mark")).Size(theme.IconSize, theme.IconSize).Shrink(0).Center().Radius(theme.RadiusPill).Background(col(pl.Surface)).Border(1, col(pl.ControlBorder))
		if v.Multi {
			mark.Radius(theme.RadiusSmall)
		}
		if v.Selected {
			mark.Background(col(pl.Accent)).Border(1, col(pl.Accent))
		}
		mark.Children(func() {
			if v.Selected {
				kit.Icon(c, icon.Check, kit.IconProps{Small: true, Key: "check"}).TextColor(col(pl.OnAccent))
			}
		})
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Children(func() {
			kit.Text(c, v.Label, kit.TextProps{Key: "label"})
			if v.Description != "" {
				kit.Text(c, v.Description, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "desc"})
			}
		})
	})
}

// QuestionView is the data of Question. Selected is parallel to Options, plus one trailing entry for the free-text
// option when AllowOther is set; the caller owns it and receives the next value through OnChange.
type QuestionView struct {
	Prompt  string
	Options []OptionView
	Multi   bool
	// AllowOther adds an "其他" option with a text field. Other is then required: the field edits *Other in place.
	AllowOther bool
	Other      *string
	Selected   []bool
	Status     AskStatus
	Message    string
}

// QuestionActions are the callbacks of Question.
type QuestionActions struct {
	// OnChange receives the next selection after a click. Without Multi, blocks makes it exclusive (the clicked option
	// is the only one on); with Multi it flips the clicked option. Disabled options never change.
	OnChange func(next []bool)
	// OnSubmit sends the current selection; it only fires when the selection is valid.
	OnSubmit func()
}

func selectedAt(s []bool, i int) bool { return i < len(s) && s[i] }

// nextSelection is the selection after clicking option index (len(Options) is the free-text option).
func nextSelection(v QuestionView, index int) []bool {
	n := len(v.Options)
	if v.AllowOther {
		n++
	}
	next := make([]bool, n)
	for i := range next {
		next[i] = selectedAt(v.Selected, i)
	}
	if index < 0 || index >= n {
		return next
	}
	if v.Multi {
		next[index] = !next[index]
		return next
	}
	for i := range next {
		next[i] = i == index
	}
	return next
}

// questionValid reports whether the selection can be submitted: at least one option, and a non-blank text for the free-text option.
func questionValid(v QuestionView) bool {
	for i := range v.Options {
		if selectedAt(v.Selected, i) {
			return true
		}
	}
	return v.AllowOther && selectedAt(v.Selected, len(v.Options)) && v.Other != nil && strings.TrimSpace(*v.Other) != ""
}

// questionSubmit is the callback handed to the submit button: nil unless the card is live and the selection valid.
func questionSubmit(v QuestionView, a QuestionActions) func() {
	if !v.Status.live() || !questionValid(v) {
		return nil
	}
	return a.OnSubmit
}

// Question is the choice card: a prompt, its options (and an optional free-text one) and a submit action. Options
// lock once the card is not live; submitting needs a valid selection and does not fire while busy.
func Question(c *ui.Context, v QuestionView, a QuestionActions, key string) ui.Element {
	audit.Named("Question", v.Prompt)
	if v.AllowOther && v.Other == nil {
		audit.Named("Question: QuestionView.Other is required with AllowOther", "")
		local := ""
		v.Other = &local
	}
	busy := v.Status == AskBusy
	return kit.Card(c, key, func() {
		ui.Row(c.Key("head")).FillWidth().AlignItems(ui.Center).Gap(theme.Space2).Children(func() {
			kit.Icon(c, icon.Help, kit.IconProps{Tone: kit.ToneAccent, Key: "g"})
			kit.Text(c, v.Prompt, kit.TextProps{Strong: true, Key: "prompt"}).Grow(1).MinWidth(0)
			if v.Status != AskPending {
				statusBadge(c, v.Status)
			}
		})
		ui.Column(c.Key("options")).FillWidth().Gap(theme.Space2).Children(func() {
			options := v.Options
			if v.AllowOther {
				options = append(append([]OptionView(nil), options...), OptionView{Label: "其他"})
			}
			for i, o := range options {
				index := i
				o.Multi = v.Multi
				o.Selected = selectedAt(v.Selected, i)
				o.Disabled = o.Disabled || !v.Status.live()
				var toggle func()
				if a.OnChange != nil {
					toggle = func() { a.OnChange(nextSelection(v, index)) }
				}
				QuestionOption(c, o, toggle, "option-"+itoa(i))
			}
			if v.AllowOther && selectedAt(v.Selected, len(v.Options)) {
				kit.Input(c, v.Other, kit.InputProps{Label: "其他答案", Placeholder: "请输入你的答案", ReadOnly: !v.Status.live(), Key: "other"})
			}
		})
		if v.Status == AskError && v.Message != "" {
			kit.Text(c, v.Message, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneDanger, Key: "message"})
		}
		if v.Status.live() || busy {
			ui.Row(c.Key("actions")).FillWidth().Justify(ui.End).Children(func() {
				kit.Button(c, "提交", kit.ButtonProps{Kind: kit.Primary, Busy: busy, Disabled: v.Status.live() && !questionValid(v), OnClick: questionSubmit(v, a), Key: "submit"})
			})
		}
	}).MaxWidth(cardMaxWidth)
}
