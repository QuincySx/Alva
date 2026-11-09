// INPUT: The design axes of the 19 K3 components and the blocks in this package.
// OUTPUT: Pages(): one gallery page per design component, cells equal to the design's variants, with live demos for the stateful cards and menus.
// POS: [L7 Kit blocks] Same rules as kit's example pages: forced states through internal/force, per-cell state through internal/cells, Chinese sample copy, no product names.
//go:build desktop

package blocks

import (
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/cells"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// Gallery sample widths: layout of the sample cells, not design tokens.
const (
	sampleCard  = 480
	sampleRow   = 360
	sampleWide  = 560
	liveBusy    = 800 * time.Millisecond
	sampleModel = "Sample"
)

func axis(name string, values ...string) kit.Axis { return kit.Axis{Name: name, Values: values} }

// Pages lists the blocks pages in gallery order.
func Pages() []kit.Page {
	return []kit.Page{
		composerPage(), voicePage(), composerMenuPage(), permissionPage(), runStatusPage(),
		messageHeaderPage(), toolActivityPage(), codeBlockPage(), citationPage(), attachmentPage(),
		approvalPage(), questionPage(), questionOptionPage(),
		modelRowPage(), connectionRowPage(), taskRowPage(),
		providerFormPage(), modelFormPage(), providerModelsPage(),
	}
}

func forcedFor(state string) force.State {
	switch {
	case strings.HasSuffix(state, "Focus"):
		return force.Focus
	case strings.HasSuffix(state, "Hover"):
		return force.Hover
	}
	return force.None
}

func composerPage() kit.Page {
	states := []string{"Idle", "Focus", "Ready", "Attaching", "Submitting", "Running", "Waiting", "Stopping", "Stopped", "Complete", "Failed", "Recording", "Transcribing", "Disabled"}
	phases := map[string]ComposerPhase{"Idle": PhaseIdle, "Focus": PhaseIdle, "Ready": PhaseReady, "Attaching": PhaseAttaching, "Submitting": PhaseSubmitting, "Running": PhaseRunning, "Waiting": PhaseWaiting,
		"Stopping": PhaseStopping, "Stopped": PhaseStopped, "Complete": PhaseComplete, "Failed": PhaseFailed, "Recording": PhaseRecording, "Transcribing": PhaseTranscribing, "Disabled": PhaseDisabled}
	return kit.Page{Name: "Composer", Design: "Composer", Category: "对话", Axes: []kit.Axis{axis("State", states...)},
		Cell: func(c *ui.Context, sel []string) {
			state := sel[0]
			initial := ""
			switch state {
			case "Idle", "Focus", "Recording", "Transcribing", "Disabled":
			default:
				initial = "帮我整理本周的进展"
			}
			draft := cells.Slot("Composer", sel, "draft", initial)
			v := ComposerView{Draft: draft, Phase: phases[state]}
			a := ComposerActions{OnSend: func() {}, OnStop: func() {}, OnAttach: func() {}, OnVoice: func() {}}
			switch state {
			case "Attaching":
				v.Attachments = []AttachmentView{{Name: "报告.pdf", Detail: "上传中", State: AttachmentUploading}}
			case "Running":
				v.Queued = 2
			case "Waiting":
				a.Queue = func() { RunStatus(c, RunStatusView{State: RunWaiting}, "run") }
			case "Stopped":
				v.Notice = "已停止，草稿已保留"
			case "Complete":
				a.Queue = func() { RunStatus(c, RunStatusView{State: RunDone}, "run") }
			case "Failed":
				v.Notice = "发送失败，草稿已保留"
			case "Recording":
				v.Elapsed = "00:12"
			}
			open := cells.Slot("Composer", sel, "perm-open", false)
			mode := cells.Slot("Composer", sel, "perm", PermissionAsk)
			a.Toolbar = func() { PermissionMenu(c, *mode, open, func(m PermissionMode) { *mode = m }, "permission") }
			force.With(forcedFor(state), func() { Composer(c, v, a, "composer").Width(sampleWide) })
		}}
}

func voicePage() kit.Page {
	states := []string{"Idle", "Hover", "Focus", "Recording", "RecordingHover", "Transcribing", "Error", "Disabled"}
	return kit.Page{Name: "VoiceInput", Design: "VoiceInput", Category: "对话", Axes: []kit.Axis{axis("State", states...)},
		Cell: func(c *ui.Context, sel []string) {
			state := map[string]VoiceState{"Recording": VoiceRecording, "RecordingHover": VoiceRecording, "Transcribing": VoiceTranscribing, "Error": VoiceError}[sel[0]]
			force.With(forcedFor(sel[0]), func() { VoiceInput(c, state, "00:12", sel[0] == "Disabled", func() {}, "voice") })
		}}
}

func composerMenuPage() kit.Page {
	items := map[string][]ComposerMenuItem{
		"Add":        {{Label: "添加文件", Description: "选择本地文件", Icon: icon.Paperclip}, {Label: "添加目录", Description: "选择当前任务可用的资料", Icon: icon.Folder}},
		"References": {{Label: "设计说明", Description: "docs/design", Icon: icon.File}, {Label: "路线图", Description: "docs/ROADMAP", Icon: icon.File}},
		"Commands":   {{Label: "总结", Description: "总结当前会话", Icon: icon.Sparkles, Shortcut: "/summarize"}, {Label: "压缩", Description: "压缩历史", Icon: icon.Database, Shortcut: "/compact"}},
	}
	kinds := map[string]ComposerMenuKind{"Add": MenuAdd, "References": MenuReferences, "Commands": MenuCommands, "Empty": MenuEmpty, "Loading": MenuLoading, "Error": MenuError}
	return kit.Page{Name: "ComposerMenu", Design: "ComposerMenu", Category: "对话", Axes: []kit.Axis{axis("View", "Add", "References", "Commands", "Empty", "Loading", "Error")},
		Cell: func(c *ui.Context, sel []string) {
			v := ComposerMenuView{Kind: kinds[sel[0]], Items: items[sel[0]]}
			switch sel[0] {
			case "Empty":
				v.Message = "没有匹配的内容"
			case "Error":
				v.Message = "加载失败"
			}
			ComposerMenu(c, v, func(int) {}, func() {}, "menu")
		}}
}

func permissionPage() kit.Page {
	return kit.Page{Name: "PermissionMenu", Design: "PermissionMenu", Category: "对话", Axes: []kit.Axis{axis("Mode", "Ask", "Auto", "Full")},
		Cell: func(c *ui.Context, sel []string) {
			mode := cells.Slot("PermissionMenu", sel, "mode", map[string]PermissionMode{"Ask": PermissionAsk, "Auto": PermissionAuto, "Full": PermissionFull}[sel[0]])
			open := cells.Slot("PermissionMenu", sel, "open", false)
			PermissionMenu(c, *mode, open, func(m PermissionMode) { *mode = m }, "perm")
		}}
}

func runStatusPage() kit.Page {
	return kit.Page{Name: "RunStatus", Design: "RunStatus", Category: "对话", Axes: []kit.Axis{axis("State", "Working", "Waiting", "Stopping", "Stopped", "Done", "Error")},
		Cell: func(c *ui.Context, sel []string) {
			state := map[string]RunState{"Working": RunWorking, "Waiting": RunWaiting, "Stopping": RunStopping, "Stopped": RunStopped, "Done": RunDone, "Error": RunError}[sel[0]]
			RunStatus(c, RunStatusView{State: state, Detail: "已用 12 秒"}, "run")
		}}
}

func messageHeaderPage() kit.Page {
	return kit.Page{Name: "MessageHeader", Design: "MessageHeader", Category: "对话", Axes: []kit.Axis{axis("Author", "Assistant", "User", "Partner")},
		Cell: func(c *ui.Context, sel []string) {
			author := map[string]AuthorKind{"Assistant": AuthorAssistant, "User": AuthorUser, "Partner": AuthorPartner}[sel[0]]
			name := map[string]string{"Assistant": "助手", "User": "我", "Partner": "研究伙伴"}[sel[0]]
			MessageHeader(c, MessageHeaderView{Author: author, Name: name, Meta: "10:30"}, "header")
		}}
}

func toolActivityPage() kit.Page {
	type spec struct {
		status   ToolStatus
		expanded bool
	}
	specs := map[string]spec{"Collapsed": {ToolDone, false}, "Running": {ToolRunning, false}, "Error": {ToolError, false}, "Expanded": {ToolDone, true}, "Focus": {ToolDone, false},
		"ExpandedFocus": {ToolDone, true}, "ErrorExpanded": {ToolError, true}}
	return kit.Page{Name: "ToolActivity", Design: "ToolActivity", Category: "对话", Axes: []kit.Axis{axis("State", "Collapsed", "Running", "Error", "Expanded", "Focus", "ExpandedFocus", "ErrorExpanded")},
		Cell: func(c *ui.Context, sel []string) {
			s := specs[sel[0]]
			expanded := cells.Slot("ToolActivity", sel, "expanded", s.expanded)
			output := "已完成 3 项检查"
			if s.status == ToolError {
				output = "退出码 1：找不到目标文件"
			}
			v := ToolActivityView{Kind: ToolCommand, Title: "运行检查", Detail: "go test ./...", Status: s.status, Output: output}
			force.With(forcedFor(sel[0]), func() { ToolActivity(c, v, *expanded, func(on bool) { *expanded = on }, "tool").Width(sampleRow) })
		}}
}

func codeBlockPage() kit.Page {
	return kit.Page{Name: "CodeBlock", Design: "CodeBlock", Category: "对话", Axes: []kit.Axis{axis("State", "Default", "Copied", "CopyError")},
		Cell: func(c *ui.Context, sel []string) {
			state := cells.Slot("CodeBlock", sel, "copy", map[string]CopyState{"Default": CopyDefault, "Copied": CopyCopied, "CopyError": CopyFailed}[sel[0]])
			v := CodeBlockView{Language: "go", Code: "func main() {\n\tfmt.Println(\"你好\")\n}", Copy: *state}
			CodeBlock(c, v, func() { *state = CopyCopied }, "code").Width(sampleRow)
		}}
}

func citationPage() kit.Page {
	return kit.Page{Name: "Citation", Design: "Citation", Category: "对话", Axes: []kit.Axis{axis("State", "Default", "Focus", "Unavailable")},
		Cell: func(c *ui.Context, sel []string) {
			v := CitationView{Index: 1, Title: "设计说明", Source: "docs/design", Unavailable: sel[0] == "Unavailable"}
			force.With(forcedFor(sel[0]), func() { Citation(c, v, func() {}, "cite") })
		}}
}

func attachmentPage() kit.Page {
	return kit.Page{Name: "Attachment", Design: "Attachment", Category: "对话", Axes: []kit.Axis{axis("State", "Ready", "Uploading", "Error", "Unavailable")},
		Cell: func(c *ui.Context, sel []string) {
			state := map[string]AttachmentState{"Ready": AttachmentReady, "Uploading": AttachmentUploading, "Error": AttachmentError, "Unavailable": AttachmentUnavailable}[sel[0]]
			detail := map[string]string{"Ready": "128 KB", "Uploading": "上传中", "Error": "上传失败", "Unavailable": "文件已移动"}[sel[0]]
			Attachment(c, AttachmentView{Name: "报告.pdf", Detail: detail, State: state}, func() {}, "attachment")
		}}
}

func approvalPage() kit.Page {
	statuses := map[string]AskStatus{"Pending": AskPending, "Allowed": AskAllowed, "Denied": AskDenied, "Expired": AskExpired}
	view := func(s AskStatus) ApprovalView {
		return ApprovalView{Title: "需要运行一条命令", Detail: "go test ./apps/ui/...", Status: s}
	}
	return kit.Page{Name: "Approval", Design: "Approval", Category: "对话", Axes: []kit.Axis{axis("State", "Pending", "Allowed", "Denied", "Expired")},
		Cell: func(c *ui.Context, sel []string) {
			Approval(c, view(statuses[sel[0]]), ApprovalActions{OnAllow: func() {}, OnDeny: func() {}}, "approval").Width(sampleCard)
		},
		Live: func(c *ui.Context) {
			status := cells.Slot("Approval", nil, "status", AskPending)
			choice := cells.Slot("Approval", nil, "choice", ChoiceNone)
			since := cells.Slot("Approval", nil, "since", time.Time{})
			if *status == AskBusy {
				if wait := liveBusy - c.Now().Sub(*since); wait <= 0 {
					*status = map[AskChoice]AskStatus{ChoiceAllow: AskAllowed, ChoiceDeny: AskDenied}[*choice]
				} else {
					c.After(wait)
				}
			}
			decide := func(ch AskChoice) func() {
				return func() { *status, *choice, *since = AskBusy, ch, c.Now() }
			}
			v := view(*status)
			v.Choice = *choice
			Approval(c, v, ApprovalActions{OnAllow: decide(ChoiceAllow), OnDeny: decide(ChoiceDeny)}, "live").Width(sampleCard)
			kit.Button(c, "重置演示", kit.ButtonProps{Kind: kit.Ghost, Key: "reset", OnClick: func() { *status, *choice = AskPending, ChoiceNone }})
		}}
}

func questionPage() kit.Page {
	options := []OptionView{{Label: "只处理最近 7 天", Description: "更快"}, {Label: "处理全部", Description: "更完整"}, {Label: "稍后再说"}}
	return kit.Page{Name: "Question", Design: "Question", Category: "对话", Axes: []kit.Axis{axis("State", "Pending", "Submitted", "Expired")},
		Cell: func(c *ui.Context, sel []string) {
			status := map[string]AskStatus{"Pending": AskPending, "Submitted": AskAnswered, "Expired": AskExpired}[sel[0]]
			selected := cells.Slot("Question", sel, "selected", []bool{false, true, false, false})
			other := cells.Slot("Question", sel, "other", "")
			Question(c, QuestionView{Prompt: "整理哪一段时间的内容？", Options: options, AllowOther: true, Other: other, Selected: *selected, Status: status},
				QuestionActions{OnChange: func(next []bool) { *selected = next }, OnSubmit: func() {}}, "question").Width(sampleCard)
		}}
}

func questionOptionPage() kit.Page {
	return kit.Page{Name: "QuestionOption", Design: "QuestionOption", Category: "对话", Axes: []kit.Axis{axis("State", "Default", "Selected", "Disabled", "Focus", "SelectedFocus", "SelectedDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			state := sel[0]
			on := cells.Slot("QuestionOption", sel, "on", strings.HasPrefix(state, "Selected"))
			v := OptionView{Label: "只处理最近 7 天", Description: "更快", Selected: *on, Disabled: strings.HasSuffix(state, "Disabled")}
			force.With(forcedFor(state), func() { QuestionOption(c, v, func() { *on = !*on }, "option").Width(sampleRow) })
		}}
}

func modelRowPage() kit.Page {
	return kit.Page{Name: "ModelRow", Design: "ModelRow", Category: "数据", Axes: []kit.Axis{axis("Mode", "Picker", "Manage"), axis("State", "Default", "Hover", "Selected", "Unavailable")},
		Cell: func(c *ui.Context, sel []string) {
			mode := map[string]ModelRowMode{"Picker": ModelPicker, "Manage": ModelManage}[sel[0]]
			fav := cells.Slot("ModelRow", sel, "fav", false)
			v := ModelRowView{Name: sampleModel, Description: "上下文 128K", Selected: sel[1] == "Selected", Unavailable: sel[1] == "Unavailable", Favorite: *fav}
			a := ModelRowActions{OnSelect: func() {}, OnToggleFavorite: func() { *fav = !*fav }, OnEdit: func() {}}
			force.With(forcedFor(sel[1]), func() { ModelRow(c, v, mode, a, "model").Width(sampleRow) })
		}}
}

func connectionRowPage() kit.Page {
	return kit.Page{Name: "ConnectionRow", Design: "ConnectionRow", Category: "数据", Axes: []kit.Axis{axis("State", "Default", "Selected", "Disconnected", "Error")},
		Cell: func(c *ui.Context, sel []string) {
			state := map[string]ConnectionState{"Default": ConnectionDefault, "Selected": ConnectionSelected, "Disconnected": ConnectionDisconnected, "Error": ConnectionError}[sel[0]]
			ConnectionRow(c, ConnectionRowView{Name: "示例供应商", Detail: "api.example.invalid", State: state}, func() {}, "connection").Width(sampleRow)
		}}
}

func taskRowPage() kit.Page {
	return kit.Page{Name: "TaskRow", Design: "TaskRow", Category: "数据", Axes: []kit.Axis{axis("State", "Running", "Waiting", "Done", "Failed", "Cancelled", "Queued")},
		Cell: func(c *ui.Context, sel []string) {
			state := map[string]TaskState{"Running": TaskRunning, "Waiting": TaskWaiting, "Done": TaskDone, "Failed": TaskFailed, "Cancelled": TaskCancelled, "Queued": TaskQueued}[sel[0]]
			TaskRow(c, TaskRowView{Title: "整理周报", Detail: "10:30 开始", State: state}, func() {}, "task").Width(sampleRow)
		}}
}

func providerFormPage() kit.Page {
	return kit.Page{Name: "ProviderForm", Design: "ProviderForm", Category: "输入", Axes: []kit.Axis{axis("Source", "Preset", "Custom")},
		Cell: func(c *ui.Context, sel []string) {
			source := map[string]ProviderSource{"Preset": SourcePreset, "Custom": SourceCustom}[sel[0]]
			name, url := cells.Slot("ProviderForm", sel, "name", ""), cells.Slot("ProviderForm", sel, "url", "")
			ProviderForm(c, ProviderFormView{Source: source}, ProviderFormActions{OnSource: func(ProviderSource) {}, OnSave: func() {}, OnCancel: func() {}}, func() {
				kit.Field(c, kit.FieldProps{Title: "名称", Key: "f-name"}, func() { kit.Input(c, name, kit.InputProps{Placeholder: "例如 示例供应商", Key: "name"}) })
				if source == SourceCustom {
					kit.Field(c, kit.FieldProps{Title: "服务地址", Key: "f-url"}, func() { kit.Input(c, url, kit.InputProps{Placeholder: "https://", Key: "url"}) })
				}
			}, "form")
		}}
}

func modelFormPage() kit.Page {
	return kit.Page{Name: "ModelForm", Design: "ModelForm", Category: "输入", Axes: []kit.Axis{axis("Mode", "Add", "Edit")},
		Cell: func(c *ui.Context, sel []string) {
			mode := map[string]ModelFormMode{"Add": ModelAdd, "Edit": ModelEdit}[sel[0]]
			id, name := cells.Slot("ModelForm", sel, "id", map[string]string{"Add": "", "Edit": "sample-model"}[sel[0]]), cells.Slot("ModelForm", sel, "name", map[string]string{"Add": "", "Edit": "示例模型"}[sel[0]])
			ModelForm(c, ModelFormView{Mode: mode, ID: id, Name: name}, ModelFormActions{OnSave: func() {}, OnCancel: func() {}}, nil, "form")
		}}
}

func providerModelsPage() kit.Page {
	models := []ModelRowView{{Name: "示例模型 A", Description: "上下文 128K", Favorite: true}, {Name: "示例模型 B", Description: "上下文 32K"}}
	states := map[string]CatalogState{"Idle": CatalogIdle, "Loading": CatalogLoading, "Ready": CatalogReady, "Empty": CatalogEmpty, "Unavailable": CatalogUnavailable, "Error": CatalogError}
	return kit.Page{Name: "ProviderModels", Design: "ProviderModels", Category: "数据", Axes: []kit.Axis{axis("State", "Idle", "Loading", "Ready", "Empty", "Unavailable", "Error")},
		Cell: func(c *ui.Context, sel []string) {
			v := ProviderModelsView{State: states[sel[0]], Models: models, Message: "稍后再试"}
			ProviderModels(c, v, ProviderModelsActions{OnFetch: func() {}, Row: func(int) ModelRowActions { return ModelRowActions{OnToggleFavorite: func() {}, OnEdit: func() {}} }}, "catalog").Width(sampleRow)
		}}
}

var _ = theme.Space1
