// INPUT: Product ToolView values over engine ToolDisplay kinds, the trace reader's bounded Raw pages, workspace file tab pinning and MyGo editor.DiffView.
// OUTPUT: Typed tool rows (command/read/edit/search/other), sub-Agent cards and todo lists with per-block expansion, async bounded bodies and file/child/trace jumps.
// POS: [L7 Hub UI] Renders tool blocks by presentation type only; never names a tool or an engine and starts reads only from explicit actions or window updates.
//go:build desktop

package hubdesktop

import (
	"path/filepath"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
)

// workspaceToolBody is one bounded body read through the trace reader.
type workspaceToolBody struct {
	uri     string
	loading bool
	text    string
	more    bool
	problem string
}

// workspaceToolDiff keeps a diff viewer for the view value it was built from.
type workspaceToolDiff struct {
	source *product.ToolView
	state  *editor.DiffState
}

// workspaceToolScope is the session and branch tool-block state belongs to.
// Generation (Send, Stop, permission or plan changes) is deliberately not part
// of it: committed blocks and their bodies do not change with it.
type workspaceToolScope struct{ session, branch string }

// workspaceToolBlocks keeps the current observation's per-block UI state:
// expansion by block ID (kept while rows scroll out of the virtual list, across
// earlier-history loads and generation changes), bodies read on demand and diff
// viewers. A session or branch change resets it, so late reads land nowhere.
type workspaceToolBlocks struct {
	scope    workspaceToolScope
	epoch    uint64
	expanded map[string]bool
	bodies   map[string]*workspaceToolBody
	diffs    map[string]workspaceToolDiff
}

// workspacePendingResource is a tab requested while its view was loading,
// valid only for the session and active branch it was requested in.
type workspacePendingResource struct {
	session, branch string
	tab             hub.WorkspaceTab
}

const (
	toolBodyPages = 4 // 2 KiB trace Raw pages per expanded body
	toolTailLines = 40
	toolTailBytes = 8 << 10
)

func (p *NativePage) toolScope() workspaceToolScope {
	return workspaceToolScope{p.selected, p.detail.Session.ActiveBranchID}
}

func (p *NativePage) syncToolBlocks() *workspaceToolBlocks {
	current := p.toolScope()
	if p.toolBlocks.expanded == nil || p.toolBlocks.scope != current {
		p.toolBlocks = workspaceToolBlocks{scope: current, epoch: p.toolBlocks.epoch + 1, expanded: map[string]bool{}, bodies: map[string]*workspaceToolBody{}, diffs: map[string]workspaceToolDiff{}}
	}
	return &p.toolBlocks
}

// refreshToolBlocks follows a window update: bodies of blocks that left the
// window are released, and expanded blocks that gained a body source read it.
func (p *NativePage) refreshToolBlocks() {
	state := p.syncToolBlocks()
	for id := range state.bodies {
		if _, kept := p.transcriptBlocks[id]; !kept {
			delete(state.bodies, id)
		}
	}
	for id := range state.diffs {
		if _, kept := p.transcriptBlocks[id]; !kept {
			delete(state.diffs, id)
		}
	}
	for id := range state.expanded {
		p.loadToolBody(id)
	}
}

func (p *NativePage) toggleToolBlock(binding workspaceTranscriptBinding, rowID string, tool transcriptTool) {
	if p.transcriptBinding() != binding || p.transcriptTools[rowID] != tool {
		return
	}
	state := p.syncToolBlocks()
	if state.expanded[rowID] {
		delete(state.expanded, rowID)
		return
	}
	state.expanded[rowID] = true
	// A failed read is not cached: expanding again retries it. Window
	// updates never retry, so a failing reference cannot loop every poll.
	if body := state.bodies[rowID]; body != nil && body.problem != "" {
		delete(state.bodies, rowID)
	}
	p.loadToolBody(rowID)
}

// loadToolBody reads an expanded block's reference body (at most four 2 KiB
// pages) through its own native branch, off the UI thread.
func (p *NativePage) loadToolBody(rowID string) {
	state := p.syncToolBlocks()
	block, shown := p.transcriptBlocks[rowID]
	tool, known := p.transcriptTools[rowID]
	if !shown || !known || !state.expanded[rowID] || block.Tool == nil || block.Tool.Body != product.ToolBodyReference {
		return
	}
	uri := block.Tool.Output
	if body := state.bodies[rowID]; body != nil && body.uri == uri {
		return
	}
	body := &workspaceToolBody{uri: uri, loading: true}
	state.bodies[rowID] = body
	branch := p.transcriptToolBranch(tool)
	if branch == "" || p.Service.Traces == nil {
		body.loading, body.problem = false, "这条工具记录没有可用的原始分支。"
		return
	}
	scope, epoch := state.scope, state.epoch
	target := product.TraceTarget{SessionID: scope.session, BranchID: branch}
	go func() {
		var text strings.Builder
		more := false
		var err error
		offset := int64(0)
		for page := 0; page < toolBodyPages; page++ {
			result, readErr := p.Service.Traces.Raw(p.ctx, target, uri, offset)
			if readErr != nil {
				err = readErr
				break
			}
			text.WriteString(result.Text)
			more = result.Truncated && result.NextOffset > offset
			if !more {
				break
			}
			offset = result.NextOffset
		}
		p.dispatch(func() {
			if p.toolBlocks.epoch != epoch || p.toolScope() != scope || p.toolBlocks.bodies[rowID] != body {
				return
			}
			body.loading, body.text, body.more = false, text.String(), more
			if err != nil {
				body.problem = err.Error()
			}
		})
	}()
}

// openToolFile pins a tool's file in the resource panel. A path the product
// cannot pin (outside the workspace, gone) opens the project file search for
// its name instead.
func (p *NativePage) openToolFile(binding workspaceTranscriptBinding, path string) {
	if binding != p.transcriptBinding() || path == "" || binding.session != p.selected {
		return
	}
	if p.Service.WorkspaceViews == nil {
		p.problem = "工作区资料面板不可用。"
		return
	}
	session := binding.session
	go func() {
		tab, err := p.Service.WorkspaceFileTab(p.ctx, session, path)
		p.dispatch(func() {
			if binding != p.transcriptBinding() {
				return
			}
			if err != nil {
				branch := p.detail.Session.ActiveBranchID
				if p.detail.Session.ID != session || branch == "" {
					p.problem = "这个文件不在当前工作区中。"
					return
				}
				tab = hub.WorkspaceTab{Kind: "files", Title: "项目文件", BranchID: branch, Query: toolSearchQuery(filepath.Base(path))}
			}
			p.openToolResource(tab)
		})
	}()
}

func toolSearchQuery(name string) string {
	for len(name) > 256 || !utf8.ValidString(name) {
		name = name[:len(name)-1]
	}
	return name
}

func (p *NativePage) openToolResource(tab hub.WorkspaceTab) {
	p.openWorkspaceResources()
	if s := p.workspaceResourceSession(); s != nil && s.loaded {
		p.openWorkspaceResource(tab)
		return
	}
	p.resources.pending = &workspacePendingResource{session: p.selected, branch: p.detail.Session.ActiveBranchID, tab: tab}
}

// openPendingWorkspaceResource opens a tab requested before the view loaded,
// unless the session or active branch changed since.
func (p *NativePage) openPendingWorkspaceResource(id string) {
	pending := p.resources.pending
	p.resources.pending = nil
	if pending != nil && pending.session == id && p.selected == id && pending.branch == p.detail.Session.ActiveBranchID {
		p.openWorkspaceResource(pending.tab)
	}
}

func toolStatusText(status view.Status) string {
	switch status {
	case view.Running, view.Stopping:
		return "进行中"
	case view.Failed:
		return "失败"
	case view.Cancelled, view.Status(product.BlockStatusInterrupted):
		return "已停止"
	case view.Status(product.BlockStatusWaiting):
		return "等待中"
	}
	return ""
}

// toolOutcomeText is an engine outcome word in the interface language.
func toolOutcomeText(status string) string {
	switch status {
	case "succeeded", "completed":
		return "成功"
	case "failed":
		return "失败"
	case "running":
		return "运行中"
	case "cancelled", "canceled":
		return "已停止"
	}
	return status
}

// toolReceiptText is a command whose output only the trace holds.
func toolReceiptText(v product.ToolView) string {
	text := "命令输出保存在原始记录中，完整输出请在轨迹中查看。"
	status := ""
	if v.Status != "" {
		status = "状态：" + toolOutcomeText(v.Status)
	}
	if v.ExitCode != nil {
		if status != "" {
			status += " · "
		}
		status += "退出码 " + strconv.Itoa(*v.ExitCode)
	}
	if status != "" {
		return status + "\n" + text
	}
	return text
}

func firstLine(text string) string {
	line, _, _ := strings.Cut(strings.TrimSpace(text), "\n")
	return line
}

// toolBlockSummary is a collapsed row: icon and one line per type.
func toolBlockSummary(block product.Block, v product.ToolView) (string, string) {
	status := view.Status(block.Status)
	suffix := ""
	if text := toolStatusText(status); text != "" {
		suffix = " · " + text
	}
	icon, label := "terminal", ""
	switch v.Kind {
	case engine.ToolCommand:
		label = firstLine(v.Command)
		if label == "" {
			label = "运行命令"
		}
		if v.ExitCode != nil {
			label += " · 退出码 " + strconv.Itoa(*v.ExitCode)
			if status == view.Failed {
				suffix = ""
			}
		} else if v.Status != "" && status == view.Completed {
			label += " · " + toolOutcomeText(v.Status)
		}
	case engine.ToolRead:
		icon, label = "file-text", "读取文件"
		if v.Path != "" {
			label = "读取 " + v.Path
		}
	case engine.ToolEdit:
		icon, label = "pencil", "修改文件"
		if v.Path != "" {
			label = "修改 " + v.Path
		}
		if v.Files > 1 {
			label += " 等 " + strconv.Itoa(v.Files) + " 个文件"
		}
		if lines := product.ToolLineSummary(v); lines != "" {
			label += " · " + lines
		}
	case engine.ToolSearch:
		icon, label = "search", "搜索"
		if v.Query != "" {
			label = "搜索 " + firstLine(v.Query)
		}
		if v.Matches != nil {
			label += " · " + strconv.Itoa(*v.Matches) + " 处"
			if v.MoreMatches {
				label += "以上"
			}
		}
	default:
		label = "工具 · " + v.Name
		if status == view.Completed {
			suffix = " · 已完成"
		}
	}
	if status == view.Failed {
		icon = "triangle-alert"
	}
	return icon, label + suffix
}

// buildWorkspaceToolBlock renders one tool block by its presentation type.
func (p *NativePage) buildWorkspaceToolBlock(c *ui.Context, row view.Message, tool transcriptTool, binding workspaceTranscriptBinding) {
	state := p.syncToolBlocks()
	block := p.transcriptBlocks[row.ID]
	v := product.ToolView{ToolDisplay: engine.ToolDisplay{Kind: engine.ToolOther}, Name: tool.name, Body: product.ToolBodyText, Jump: product.ToolJumpTrace}
	if block.Tool != nil {
		v = *block.Tool
	}
	open := state.expanded[row.ID]
	ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
		ui.Box(c).Width(32).Shrink(0)
		ui.Column(c).Label("Tool content " + row.ID).FillWidth().MinWidth(0).Gap(8).Children(func() {
			switch v.Kind {
			case engine.ToolSubagent:
				p.buildToolAgentCard(c, row, block, v, tool, binding, open)
			case engine.ToolTodo:
				p.buildToolTodo(c, row, block, v, tool, binding, open)
			default:
				icon, label := toolBlockSummary(block, v)
				p.buildToolDisclosure(c, row.ID, icon, label, tool, binding, open)
				if open {
					p.buildToolDetail(c, row, block, v, tool, binding)
				}
			}
		})
	})
}

func (p *NativePage) buildToolDisclosure(c *ui.Context, rowID, icon, label string, tool transcriptTool, binding workspaceTranscriptBinding, open bool) {
	button := ui.ButtonBase(c.Key("tool-disclosure")).Label("Tool activity "+rowID).
		Expanded(open).
		FillWidth().Height(37).Padding(8, 0).Gap(8).Radius(0).
		AlignItems(ui.Center).Justify(ui.Start).Background(ui.Transparent).
		TextColor(c.Theme().TextMuted).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5)
	button.OnClick(func() { p.toggleToolBlock(binding, rowID, tool) }).Children(func() {
		ui.Icon(c, workspaceIcon(icon)).Label("Tool activity icon "+rowID).Size(16, 16).Shrink(0)
		ui.Text(c, label).MinWidth(0).Shrink(1).SingleLine()
		chevron := "chevron-right"
		if open {
			chevron = "chevron-down"
		}
		ui.Icon(c, workspaceIcon(chevron)).Label("Tool disclosure icon "+rowID).Size(16, 16).Shrink(0)
	})
}

func (p *NativePage) buildToolText(c *ui.Context, text string, failed bool) {
	if text == "" {
		return
	}
	color := c.Theme().TextMuted
	if failed {
		color = c.Theme().Danger
	}
	ui.Text(c, text).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(color)
}

func buildToolMono(c *ui.Context, rowID, text string) {
	if text == "" {
		return
	}
	ui.Column(c).Label("Tool output " + rowID).FillWidth().MinWidth(0).Padding(8).Radius(8).Background(workspacePalette(c).subtle).Children(func() {
		ui.Text(c, text).Font("monospace").FontSize(12).LineHeight(1.5).TextColor(c.Theme().Text)
	})
}

func (p *NativePage) buildToolReferenceBody(c *ui.Context, rowID string) {
	body := p.toolBlocks.bodies[rowID]
	switch {
	case body == nil || body.loading:
		ui.Text(c, "正在读取…").FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	case body.problem != "":
		ui.Text(c, body.problem).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().Danger)
	default:
		buildToolMono(c, rowID, body.text)
		if body.more {
			ui.Text(c, "这里只显示开头部分；完整内容可在轨迹中查看。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		}
	}
}

func (p *NativePage) buildToolDiff(c *ui.Context, rowID string, block product.Block) {
	cached, exists := p.toolBlocks.diffs[rowID]
	if !exists || cached.source != block.Tool {
		diff := block.Tool.Diff
		cached = workspaceToolDiff{source: block.Tool, state: &editor.DiffState{Old: editor.NewBuffer(diff.Old), New: editor.NewBuffer(diff.New), Layout: editor.DiffUnified, Indicators: editor.DiffClassic, FitContent: true, Wrap: true, NoLineNumbers: diff.Partial, Font: ui.Font{Family: "monospace", Size: 12}}}
		p.toolBlocks.diffs[rowID] = cached
	}
	ui.Column(c).Label("Tool diff "+rowID).FillWidth().MinWidth(0).Radius(8).Border(1, c.Theme().Border).Clip().Children(func() {
		editor.DiffView(c, cached.state).FillWidth()
	})
}

// buildToolDetail is the expanded content of a command, read, edit, search
// or other block, then its jumps.
func (p *NativePage) buildToolDetail(c *ui.Context, row view.Message, block product.Block, v product.ToolView, tool transcriptTool, binding workspaceTranscriptBinding) {
	failed := block.Status == product.BlockStatusFailed
	switch v.Kind {
	case engine.ToolCommand:
		if v.OutputInTrace {
			// The result is a job receipt; show its status, not the raw JSON.
			p.buildToolText(c, toolReceiptText(v), failed)
			break
		}
		buildToolMono(c, row.ID, product.ToolTail(block.Text, toolTailLines, toolTailBytes))
	case engine.ToolRead:
		if v.Body == product.ToolBodyReference {
			p.buildToolReferenceBody(c, row.ID)
		} else {
			p.buildToolText(c, block.Text, failed)
		}
	case engine.ToolEdit:
		if failed {
			p.buildToolText(c, block.Text, true)
		}
		if v.Diff != nil {
			p.buildToolDiff(c, row.ID, block)
			if v.Diff.Partial {
				ui.Text(c, "只显示改动片段；完整文件可在右栏打开。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
			}
		} else if !failed {
			p.buildToolText(c, block.Text, false)
		}
	case engine.ToolSearch:
		if failed || len(v.Results) == 0 {
			p.buildToolText(c, block.Text, failed)
			break
		}
		ui.Column(c).Label("Tool results " + row.ID).FillWidth().MinWidth(0).Gap(2).Children(func() {
			for i, match := range v.Results {
				label := match.Path
				if match.Line > 0 {
					label += ":" + strconv.Itoa(match.Line)
				}
				if text := strings.TrimSpace(match.Text); text != "" {
					label += " · " + firstLine(text)
				}
				path := match.Path
				ui.ButtonBase(c.Key("tool-result-"+strconv.Itoa(i))).Label("Open search result "+row.ID+" "+strconv.Itoa(i)).
					FillWidth().Height(28).Padding(4, 8).Radius(6).AlignItems(ui.Center).Justify(ui.Start).Background(ui.Transparent).
					TextColor(c.Theme().Text).FontSize(workspaceFontBody).Disabled(path == "").
					OnClick(func() { p.openToolFile(binding, path) }).Children(func() {
					ui.Text(c, label).MinWidth(0).Shrink(1).SingleLine()
				})
			}
		})
	default:
		p.buildToolText(c, block.Text, false)
	}
	if block.Truncated {
		ui.Text(c, "结果较长，这里只显示开头部分；完整内容可在轨迹中查看。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	}
	ui.Row(c).Label("Tool actions " + row.ID).FillWidth().Gap(8).Children(func() {
		if v.Jump == product.ToolJumpFile {
			path := v.Path
			ui.Button(c, "在右栏打开").Label("Open tool file " + row.ID).OnClick(func() { p.openToolFile(binding, path) })
		}
		ui.Button(c, "在轨迹中查看").Label("Inspect tool result " + row.ID).Disabled(p.trace.loading).
			OnClick(func() { p.inspectTranscriptTool(binding, row.ID, tool) })
	})
}

func agentStatusText(status view.Status) string {
	switch status {
	case view.Running, view.Stopping:
		return "运行中"
	case view.Completed:
		return "完成"
	case view.Failed:
		return "失败"
	}
	if text := toolStatusText(status); text != "" {
		return text
	}
	return string(status)
}

// buildToolAgentCard renders a sub-Agent call as a card: task and status,
// its result summary when expanded, and entry into the child session record.
func (p *NativePage) buildToolAgentCard(c *ui.Context, row view.Message, block product.Block, v product.ToolView, tool transcriptTool, binding workspaceTranscriptBinding, open bool) {
	status := view.Status(block.Status)
	ui.Column(c).Label("Agent card "+row.ID).FillWidth().MinWidth(0).Padding(12).Gap(8).Radius(12).
		Border(1, c.Theme().Border).Background(workspacePalette(c).subtle).Children(func() {
		header := ui.ButtonBase(c.Key("agent-card")).Label("Tool activity " + row.ID).Expanded(open).
			FillWidth().Height(28).Gap(8).Radius(0).AlignItems(ui.Center).Justify(ui.Start).Background(ui.Transparent).
			TextColor(c.Theme().Text).FontSize(workspaceFontBody).LineHeight(1.5)
		header.OnClick(func() { p.toggleToolBlock(binding, row.ID, tool) }).Children(func() {
			icon := "users-round"
			if status == view.Failed {
				icon = "triangle-alert"
			}
			ui.Icon(c, workspaceIcon(icon)).Label("Tool activity icon "+row.ID).Size(16, 16).Shrink(0)
			task := firstLine(v.Task)
			if task == "" {
				task = "子 Agent"
			}
			ui.Text(c, task).FontWeight(600).MinWidth(0).Shrink(1).SingleLine()
			ui.Box(c).Grow(1)
			color := c.Theme().TextMuted
			if status == view.Failed {
				color = c.Theme().Danger
			}
			ui.Text(c, agentStatusText(status)).FontSize(workspaceFontCaption).TextColor(color).Shrink(0)
		})
		if open {
			if v.Body == product.ToolBodyReference {
				p.buildToolReferenceBody(c, row.ID)
			} else {
				p.buildToolText(c, block.Text, status == view.Failed)
			}
		}
		ui.Row(c).Label("Tool actions " + row.ID).FillWidth().Gap(8).Children(func() {
			ui.Button(c, "进入子会话").Label("Enter agent session " + row.ID).Disabled(v.Jump != product.ToolJumpChild || p.trace.loading).
				OnClick(func() { p.enterTranscriptToolChild(binding, row.ID, tool) })
			ui.Button(c, "在轨迹中查看").Label("Inspect tool result " + row.ID).Disabled(p.trace.loading).
				OnClick(func() { p.inspectTranscriptTool(binding, row.ID, tool) })
		})
	})
}

// buildToolTodo renders a plan update: the latest of a Turn in full, earlier
// ones as one "updated" line that can still be opened.
func (p *NativePage) buildToolTodo(c *ui.Context, row view.Message, block product.Block, v product.ToolView, tool transcriptTool, binding workspaceTranscriptBinding, open bool) {
	if v.Superseded && !open {
		p.buildToolDisclosure(c, row.ID, "list", "待办已更新", tool, binding, false)
		return
	}
	ui.Column(c).Label("Todo list "+row.ID).FillWidth().MinWidth(0).Padding(12).Gap(6).Radius(12).Border(1, c.Theme().Border).Children(func() {
		title := "待办清单"
		if text := toolStatusText(view.Status(block.Status)); text != "" {
			title += " · " + text
		}
		if v.Superseded {
			p.buildToolDisclosure(c, row.ID, "list", title+" · 已更新", tool, binding, true)
		} else {
			ui.Row(c).Label("Tool activity " + row.ID).FillWidth().Height(28).Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, workspaceIcon("list")).Label("Tool activity icon "+row.ID).Size(16, 16).Shrink(0)
				ui.Text(c, title).FontSize(workspaceFontBody).FontWeight(600).MinWidth(0).SingleLine()
			})
		}
		for i, todo := range v.Todos {
			icon, color := "square", c.Theme().Text
			switch todo.Status {
			case engine.TodoCompleted:
				icon, color = "check", c.Theme().TextMuted
			case engine.TodoInProgress:
				icon = "loader-circle"
			}
			ui.Row(c).Label("Todo item " + row.ID + " " + strconv.Itoa(i)).FillWidth().Gap(8).AlignItems(ui.Start).Children(func() {
				ui.Icon(c, workspaceIcon(icon)).Label("Todo status "+row.ID+" "+strconv.Itoa(i)+" "+string(todo.Status)).Size(14, 14).Shrink(0).Margin(3, 0, 0, 0)
				ui.Text(c, todo.Text).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(color).MinWidth(0)
			})
		}
		if len(v.Todos) == 0 {
			text := v.Plan
			if text == "" {
				text = block.Text
			}
			p.buildToolText(c, text, block.Status == product.BlockStatusFailed)
		}
	})
}
