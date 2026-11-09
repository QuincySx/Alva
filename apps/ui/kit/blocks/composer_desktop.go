// INPUT: Alva Composer (14 states), VoiceInput (8), ComposerMenu (6) and PermissionMenu (3) definitions, the composer recipe.
// OUTPUT: Composer shell with its slots, VoiceInput, ComposerMenu and PermissionMenu: appearance and interaction slots, no send, queue or voice logic.
// POS: [L7 Kit blocks] Queueing and interjecting (M2) fill the Queue slot and the OnSend meaning later; the shell only gives them a place and a look.
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

// VoiceState is the state of the voice input control; Idle is the zero value.
type VoiceState uint8

const (
	VoiceIdle VoiceState = iota
	VoiceRecording
	VoiceTranscribing
	VoiceError
)

func voiceLabel(s VoiceState) string {
	switch s {
	case VoiceRecording:
		return "停止录音"
	case VoiceTranscribing:
		return "正在转写"
	case VoiceError:
		return "语音输入出错，重试"
	}
	return "开始语音输入"
}

// VoiceInput is the 32px microphone control. Recording shows a stop mark and elapsed; Transcribing is busy and
// keeps its name and focus; hover and focus come from the runtime.
func VoiceInput(c *ui.Context, state VoiceState, elapsed string, disabled bool, onToggle func(), key string) ui.Element {
	label := voiceLabel(state)
	audit.Named("VoiceInput", label)
	pl, fs := pal(c), force.Take()
	busy := state == VoiceTranscribing
	b := ui.ButtonBase(keyed(c, key)).Label(label).Height(roundAction).MinWidth(roundAction).Padding(0, theme.Space2).Gap(theme.Space1).Radius(theme.RadiusPill).
		AlignItems(ui.Center).Justify(ui.Center).FocusRing(false).Disabled(disabled && !busy)
	fill, text := col(pl.Transparent), col(pl.Secondary)
	switch {
	case disabled:
	case state == VoiceRecording:
		fill, text = col(pl.DangerBg), col(pl.Danger)
	case state == VoiceError:
		text = col(pl.Danger)
	case fs.Hovered(b.Hovered()):
		fill, text = col(pl.Subtle), col(pl.Text)
	}
	b.Background(fill).TextColor(text)
	if busy {
		b.Description("正在转写，请等待完成。")
	}
	focused, accent := !disabled && fs.Focused(b.FocusVisible()), col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			kit.InlineFocus(pt, r, theme.RadiusPill, accent)
		}
	})
	if onToggle != nil {
		off := disabled || busy
		b.OnClick(func() {
			if !off {
				onToggle()
			}
		})
	}
	return b.Children(func() {
		switch state {
		case VoiceRecording:
			kit.Icon(c, icon.Stop, kit.IconProps{Small: true, Key: "g"})
			if elapsed != "" {
				kit.Text(c, elapsed, kit.TextProps{Role: kit.RoleCaption, Key: "elapsed"})
			}
		case VoiceTranscribing:
			kit.Loader(c.Key("busy"), theme.IconSmall, kit.ToneSecondary)
		case VoiceError:
			kit.Icon(c, icon.Warning, kit.IconProps{Small: true, Key: "g"})
		default:
			kit.Icon(c, icon.Microphone, kit.IconProps{Small: true, Key: "g"})
		}
	})
}

// PermissionMode is how much the assistant may do without asking; Ask is the zero value.
type PermissionMode uint8

const (
	PermissionAsk PermissionMode = iota
	PermissionAuto
	PermissionFull
)

func permissionLabel(m PermissionMode) string {
	switch m {
	case PermissionAuto:
		return "自动"
	case PermissionFull:
		return "完全访问"
	}
	return "询问"
}

// PermissionMenu is the mode picker: a ghost button naming the mode that opens a menu of the three; onChange gets the choice.
func PermissionMenu(c *ui.Context, mode PermissionMode, open *bool, onChange func(PermissionMode), key string) {
	audit.Named("PermissionMenu", "权限模式")
	if open == nil {
		// open is required; a local value keeps the frame alive and the audit reports the mistake.
		audit.Named("PermissionMenu: open is required", "")
		local := false
		open = &local
	}
	trigger := kit.Button(c, permissionLabel(mode), kit.ButtonProps{Kind: kit.Ghost, Icon: icon.Shield, TrailingIcon: icon.ChevronDown, Key: key, OnClick: func() { *open = !*open }})
	items := []kit.MenuItem{
		{Key: "ask", Label: "询问", Description: "每次操作前先问我", Selected: mode == PermissionAsk, Icon: icon.Shield},
		{Key: "auto", Label: "自动", Description: "安全操作自动执行，其余询问", Selected: mode == PermissionAuto, Icon: icon.Sparkles},
		{Key: "full", Label: "完全访问", Description: "不再询问，直接执行", Selected: mode == PermissionFull, Danger: true, Icon: icon.Warning},
	}
	if chosen := kit.Menu(c, trigger, open, items, kit.MenuProps{Label: "权限模式"}); chosen != "" && onChange != nil {
		onChange(map[string]PermissionMode{"ask": PermissionAsk, "auto": PermissionAuto, "full": PermissionFull}[chosen])
	}
}

// ComposerMenuKind is the view of the composer's popup menu; Add is the zero value.
type ComposerMenuKind uint8

const (
	MenuAdd ComposerMenuKind = iota
	MenuReferences
	MenuCommands
	MenuEmpty
	MenuLoading
	MenuError
)

// ComposerMenuItem is one entry of ComposerMenu.
type ComposerMenuItem struct {
	Label, Description, Shortcut string
	Icon                         icon.Name
}

// ComposerMenuView is the data of ComposerMenu.
type ComposerMenuView struct {
	Kind  ComposerMenuKind
	Items []ComposerMenuItem
	// Message is the text of the Empty and Error views.
	Message string
}

const composerMenuWidth = 320

// ComposerMenu is the panel opened by the add button, @ and /: a list of entries, or an empty, loading or error view.
// onPick gets the entry index; onRetry reruns a failed load.
func ComposerMenu(c *ui.Context, v ComposerMenuView, onPick func(int), onRetry func(), key string) ui.Element {
	audit.Named("ComposerMenu", "撰写菜单")
	panel := kit.PanelSurface(c, ui.Column(keyed(c, key)).Width(composerMenuWidth).Label("撰写菜单"))
	return panel.Children(func() {
		switch v.Kind {
		case MenuLoading:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyLoading, Title: "正在加载", Key: "state"})
		case MenuEmpty:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyNone, Title: v.Message, Key: "state"})
		case MenuError:
			kit.EmptyState(c, kit.EmptyStateProps{Kind: kit.EmptyError, Title: v.Message, Key: "state", Action: func() {
				kit.Button(c, "重试", kit.ButtonProps{OnClick: onRetry, Key: "retry"})
			}})
		default:
			for i, it := range v.Items {
				index := i
				kit.ListRow(c, it.Label, kit.ListRowProps{Description: it.Description, Key: "item-" + itoa(i), OnClick: func() {
					if onPick != nil {
						onPick(index)
					}
				}, Leading: func() { kit.Icon(c, it.Icon, kit.IconProps{Small: true, Key: "g"}) }, Trailing: func() {
					if it.Shortcut != "" {
						kit.Text(c, it.Shortcut, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "shortcut"})
					}
				}})
			}
		}
	})
}

// ComposerPhase is the lifecycle phase the Composer shell draws; Idle is the zero value.
type ComposerPhase uint8

const (
	PhaseIdle ComposerPhase = iota
	PhaseReady
	PhaseAttaching
	PhaseSubmitting
	PhaseRunning
	PhaseWaiting
	PhaseStopping
	PhaseStopped
	PhaseComplete
	PhaseFailed
	PhaseRecording
	PhaseTranscribing
	PhaseDisabled
)

// ComposerView is the data of Composer. Draft is required: it is the caller's text and the editor edits it in place.
// A nil Draft does not panic: a local value is used (typing is lost) and the kit audit records the mistake.
type ComposerView struct {
	Draft       *string
	Phase       ComposerPhase
	Placeholder string
	Attachments []AttachmentView
	// Queued counts messages waiting behind the running one; the shell shows it in the queue slot. How queueing and
	// interjecting behave is decided elsewhere (M2).
	Queued int
	// Notice is a one-line message under the editor (a failure, a stop reason).
	Notice string
	// Elapsed is the recording time shown by VoiceInput.
	Elapsed string
	// MenuOpen says a completion menu is open: Enter then belongs to the menu and never submits.
	MenuOpen bool
}

// ComposerActions are the callbacks and slots of Composer. Every callback is optional.
type ComposerActions struct {
	OnSend, OnStop, OnAttach, OnVoice func()
	// OnQueue and OnInterject are what Enter and Cmd+Enter do while a run is active; without OnInterject,
	// Cmd+Enter falls back to OnQueue. How queueing and interjecting work is the product's (M2).
	OnQueue, OnInterject func()
	OnRemoveAttachment   func(index int)
	// Toolbar builds the leading toolbar controls (permission and model pickers); Queue builds the queue and
	// interjection area above the editor.
	Toolbar, Queue func()
}

const (
	composerLines    = 2
	composerMaxLines = 8
)

// composerEnter decides what an Enter or Cmd+Enter in the editor does. It returns the callback to run (nil for none)
// and whether the key is consumed. Shift+Enter never reaches it: it stays a line break. Enter does not submit while an
// input method is composing (it picks a candidate), while a completion menu is open (it picks an entry), on a blank
// draft (it neither sends nor adds a line) or while the composer cannot submit (a blocked Enter stays a line break,
// a blocked Cmd+Enter is taken). Idle: Enter sends. Running: Enter queues, Cmd+Enter interjects.
func composerEnter(phase ComposerPhase, blank, menuOpen, composing, cmd, uploading bool, a ComposerActions) (run func(), consume bool) {
	if menuOpen || composing || (blank && !cmd) {
		return nil, true
	}
	switch {
	case blank, phase.locked(), phase == PhaseStopping:
		return nil, cmd
	case phase.running():
		run = a.OnQueue
		if cmd && a.OnInterject != nil {
			run = a.OnInterject
		}
	case uploading:
		return nil, cmd
	default:
		run = a.OnSend
	}
	return run, run != nil || cmd
}

func (p ComposerPhase) locked() bool {
	switch p {
	case PhaseSubmitting, PhaseRecording, PhaseTranscribing, PhaseDisabled:
		return true
	}
	return false
}

// running reports phases in which a run is active and Stop is offered instead of Send.
func (p ComposerPhase) running() bool {
	return p == PhaseRunning || p == PhaseWaiting || p == PhaseStopping
}

// Composer is the input card: attachments, queue slot, editor, notice and toolbar with voice and Send or Stop.
func Composer(c *ui.Context, v ComposerView, a ComposerActions, key string) ui.Element {
	audit.Named("Composer", "输入框")
	if v.Draft == nil {
		audit.Named("Composer: ComposerView.Draft is required", "")
		local := ""
		v.Draft = &local
	}
	pl, fs := pal(c), force.Take()
	uploading := false
	for _, at := range v.Attachments {
		uploading = uploading || at.State == AttachmentUploading
	}
	card := ui.Column(keyed(c, key)).FillWidth().Padding(theme.Space2).Gap(theme.Space2).Radius(theme.RadiusCard).Background(col(pl.Surface)).Label("输入框")
	var editor ui.Element
	card.Children(func() {
		if len(v.Attachments) > 0 {
			ui.Row(c.Key("attachments")).FillWidth().Wrap().Gap(theme.Space2).Children(func() {
				for i, at := range v.Attachments {
					index := i
					var remove func()
					if a.OnRemoveAttachment != nil && !v.Phase.locked() {
						remove = func() { a.OnRemoveAttachment(index) }
					}
					Attachment(c, at, remove, "attachment-"+itoa(i))
				}
			})
		}
		if a.Queue != nil || v.Queued > 0 {
			ui.Row(c.Key("queue")).FillWidth().AlignItems(ui.Center).Gap(theme.Space2).Children(func() {
				if v.Queued > 0 {
					kit.Badge(c, "已排队 "+itoa(v.Queued)+" 条", kit.BadgeProps{Tone: kit.BadgeNeutral, Key: "queued"})
				}
				if a.Queue != nil {
					a.Queue()
				}
			})
		}
		placeholder := v.Placeholder
		if placeholder == "" {
			placeholder = "输入消息"
		}
		if v.Phase.running() {
			placeholder = "输入后可插话或排队"
		}
		editor = ui.TextAreaBase(c.Key("editor"), v.Draft).FillWidth().Padding(theme.Space2).FontSize(theme.FontSizeBody).LineHeight(theme.LineNormal).
			TextColor(col(pl.Text)).FocusRing(false).Label("消息输入").Placeholder(placeholder).PlaceholderColor(col(pl.Secondary)).Lines(composerLines, composerMaxLines).
			ReadOnly(v.Phase.locked()).Disabled(v.Phase == PhaseDisabled)
		composing, draftText := editor.Composing(), v.Draft
		editor.OnTextKey(func(k ui.TextKey) bool {
			if k.Key != ui.KeyEnter || (k.Mods != 0 && k.Mods != ui.Cmd) {
				return false
			}
			text := k.Text()
			run, consume := composerEnter(v.Phase, strings.TrimSpace(text) == "", v.MenuOpen, composing, k.Mods == ui.Cmd, uploading, a)
			if run != nil {
				*draftText = text // the key carries input queued before it: submit what the user sees
				run()
			}
			return consume
		})
		if v.Notice != "" {
			tone := kit.ToneSecondary
			if v.Phase == PhaseFailed {
				tone = kit.ToneDanger
			}
			kit.Text(c, v.Notice, kit.TextProps{Role: kit.RoleCaption, Tone: tone, Key: "notice"})
		}
		ui.Row(c.Key("toolbar")).FillWidth().AlignItems(ui.Center).Gap(theme.Space1).Children(func() {
			kit.IconButton(c, icon.Plus, "添加", kit.IconButtonProps{Disabled: v.Phase.locked(), OnClick: a.OnAttach, Key: "add"})
			if a.Toolbar != nil {
				a.Toolbar()
			}
			ui.Spacer(c).Grow(1)
			voice := VoiceIdle
			switch v.Phase {
			case PhaseRecording:
				voice = VoiceRecording
			case PhaseTranscribing:
				voice = VoiceTranscribing
			}
			VoiceInput(c, voice, v.Elapsed, v.Phase == PhaseDisabled || v.Phase.running() || v.Phase == PhaseSubmitting, a.OnVoice, "voice")
			sendOrStop(c, v, a, uploading)
		})
	})
	focused := fs.Focused(editor.Focused())
	border, width := col(pl.Border), float32(1)
	switch {
	case v.Phase == PhaseFailed:
		border = col(pl.Danger)
	case focused:
		border, width = col(pl.Accent), theme.BorderFocus
	}
	card.DrawOver(func(pt *ui.Painter, r ui.Rect) { pt.Stroke(r, border, theme.RadiusCard, width) })
	return card
}

// sendOrStop draws the 32px round primary action: Stop while a run is active, otherwise Send.
func sendOrStop(c *ui.Context, v ComposerView, a ComposerActions, uploading bool) {
	pl := pal(c)
	stop := v.Phase.running()
	label, glyph := "发送", icon.ArrowUp
	if stop {
		label, glyph = "停止", icon.Stop
	}
	audit.Named("ComposerAction", label)
	empty := v.Draft == nil || *v.Draft == ""
	busy := v.Phase == PhaseSubmitting || v.Phase == PhaseStopping
	disabled := v.Phase == PhaseDisabled || v.Phase == PhaseRecording || v.Phase == PhaseTranscribing || (!stop && (empty || uploading))
	b := ui.ButtonBase(c.Key("action")).Label(label).Size(roundAction, roundAction).Shrink(0).Radius(theme.RadiusPill).AlignItems(ui.Center).Justify(ui.Center).FocusRing(false).Disabled(disabled && !busy)
	fill, text := col(pl.Text), col(pl.Surface)
	if disabled && !busy {
		fill, text = col(pl.Subtle), col(pl.Secondary)
	}
	b.Background(fill).TextColor(text)
	if busy {
		b.Description("正在处理，请等待完成。")
	}
	focused, surface, accent := b.FocusVisible(), col(pl.Surface), col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			pt.Stroke(r, surface, theme.RadiusPill, theme.Space1)
			pt.Stroke(r, accent, theme.RadiusPill, theme.BorderFocus)
		}
	})
	action := a.OnSend
	if stop {
		action = a.OnStop
	}
	if action != nil {
		off := disabled || busy
		b.OnClick(func() {
			if !off {
				action()
			}
		})
	}
	b.Children(func() {
		if busy {
			kit.LoaderColor(c.Key("busy"), theme.IconSmall, text)
		} else {
			kit.Icon(c, glyph, kit.IconProps{Small: true, Key: "g"})
		}
	})
}
