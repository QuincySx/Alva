// INPUT: Product TranscriptWindow content blocks, typed native references and the trace view's jump entry.
// OUTPUT: Block-to-row projection, scroll-driven paging, reply columns, copy actions and tool activity that opens its trace step.
// POS: [L7 Hub UI] Keeps product presentation and tool details separate from shared scrolling and Markdown; never model context.
//go:build desktop

package hubdesktop

import (
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

type workspaceTranscriptBinding struct {
	session, branch string
	generation      uint64
}

// Result references and native ownership remain separate from each other and
// row text. The value is comparable so stale UI actions can be rejected.
type transcriptTool struct {
	sessionID string
	nativeID  string
	name      string
	reference engine.Reference
	hasRef    bool
}

func transcriptToolOf(block product.Block) transcriptTool {
	tool := transcriptTool{sessionID: block.SessionID, nativeID: block.NativeSessionID, name: block.Name}
	if block.Ref != nil {
		tool.reference, tool.hasRef = *block.Ref, true
	}
	return tool
}

// transcriptRow keeps the established row roles; the shared view and this
// renderer still branch on them while blocks carry the typed facts.
func transcriptRow(block product.Block) view.Message {
	row := view.Message{ID: block.ID, RunID: block.AttemptID, Text: block.Text, Status: view.Status(block.Status)}
	switch block.Kind {
	case product.BlockKindUser:
		row.Role = "You"
	case product.BlockKindReply:
		row.Role = "Alva"
	case product.BlockKindReasoning:
		row.Role = "Thinking"
	case product.BlockKindContent:
		row.Role = "Content · " + block.Name
	case product.BlockKindTool:
		row.Role = "Tool · " + block.Name
	case product.BlockKindActivity:
		row.Role = "Execution activity · " + block.Name
	case product.BlockKindTimeline:
		row.Role, row.RunID = "Product · "+block.Name, ""
	default:
		row.Role = "Status"
		if block.Name == "retry" {
			row.Role = "Retry warning"
		}
	}
	return row
}

func (p *NativePage) resetTranscript() {
	p.transcript = product.NewTranscriptWindow(0)
	p.transcriptRows, p.transcriptBlocks, p.transcriptTools = nil, nil, nil
}

// projectTranscript turns the window's blocks into shared message rows.
func (p *NativePage) projectTranscript() {
	blocks := p.transcript.Blocks()
	rows := make([]view.Message, len(blocks))
	p.transcriptBlocks = make(map[string]product.Block, len(blocks))
	p.transcriptTools = make(map[string]transcriptTool)
	for i, block := range blocks {
		rows[i] = transcriptRow(block)
		p.transcriptBlocks[block.ID] = block
		if block.Kind == product.BlockKindTool {
			p.transcriptTools[block.ID] = transcriptToolOf(block)
		}
	}
	p.transcriptRows = rows
}

// followTranscriptScroll pages by what the last frame showed: near the top it
// loads earlier blocks, near the bottom of a trimmed window it reloads the latest.
func (p *NativePage) followTranscriptScroll() {
	first, last := p.chat.VisibleRows()
	if last < first || p.selected == "" || p.polling || p.busy {
		return
	}
	if first < 5 && p.transcript.MoreBefore() {
		p.older()
		return
	}
	if p.transcript.MoreAfter() && last >= len(p.model.Messages)-5 {
		p.loadTail()
	}
}

func (p *NativePage) buildWorkspaceTranscript(c *ui.Context) {
	p.followTranscriptScroll()
	p.chat.MessageGap = 24
	ui.Column(c).Label("Conversation transcript").Grow(1).MinHeight(0).FillWidth().Padding(16, 0, 0, 0).Children(func() {
		p.chat.BuildMessages(c)
	})
}

func (p *NativePage) transcriptBinding() workspaceTranscriptBinding {
	return workspaceTranscriptBinding{p.selected, p.detail.Session.ActiveBranchID, p.generation}
}

// One disclosure is retained only within the currently observed session/branch.
// All actual rows and reading anchors remain owned by the shared NativeView.
func newWorkspaceTranscriptRenderer(p *NativePage, options *markdown.Options) func(*ui.Context, view.Message) bool {
	var binding workspaceTranscriptBinding
	expanded := ""
	return func(c *ui.Context, row view.Message) bool {
		current := p.transcriptBinding()
		if current != binding {
			binding, expanded = current, ""
		}
		if _, exists := p.transcriptTools[expanded]; !exists {
			expanded = ""
		}
		if row.Role == "You" {
			ui.Row(c).Label("User turn " + row.ID).FillWidth().Justify(ui.End).Children(func() {
				ui.Column(c).Label("User message " + row.ID).FillWidth().MaxWidth(560).
					Padding(12).Radius(16).Background(workspacePalette(c).subtle).Children(func() {
					ui.Text(c, row.Text).FontSize(c.Theme().FontSize).FontWeight(400).LineHeight(1.75)
				})
			})
			return true
		}
		tool, exists := p.transcriptTools[row.ID]
		if !exists {
			p.buildWorkspaceTranscriptMessage(c, row, current, options)
			return true
		}
		open := expanded == row.ID
		label := "工具 · " + tool.name
		icon := "terminal"
		switch row.Status {
		case view.Running, view.Stopping:
			label += " · 进行中"
		case view.Failed:
			label += " · 失败"
			icon = "triangle-alert"
		case view.Completed:
			label += " · 已完成"
		case view.Cancelled:
			label += " · 已停止"
		}
		ui.Row(c).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
			ui.Box(c).Width(32).Shrink(0)
			ui.Column(c).Label("Tool content " + row.ID).FillWidth().MinWidth(0).Gap(8).Children(func() {
				button := ui.ButtonBase(c.Key("tool-disclosure")).Label("Tool activity "+row.ID).
					Expanded(open).
					FillWidth().Height(37).Padding(8, 0).Gap(8).Radius(0).
					AlignItems(ui.Center).Justify(ui.Start).Background(ui.Transparent).
					TextColor(c.Theme().TextMuted).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5)
				button.OnClick(func() {
					if p.transcriptBinding() != current || p.transcriptTools[row.ID] != tool {
						return
					}
					if expanded == row.ID {
						expanded = ""
					} else {
						expanded = row.ID
					}
				}).Children(func() {
					ui.Icon(c, workspaceIcon(icon)).Label("Tool activity icon "+row.ID).Size(16, 16).Shrink(0)
					ui.Text(c, label).MinWidth(0).SingleLine()
					chevron := "chevron-right"
					if open {
						chevron = "chevron-down"
					}
					ui.Icon(c, workspaceIcon(chevron)).Label("Tool disclosure icon "+row.ID).Size(16, 16).Shrink(0)
				})
				if open {
					if row.Text != "" {
						ui.Text(c, row.Text).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
					}
					ui.Button(c, "在轨迹中查看").Label("Inspect tool result " + row.ID).Disabled(p.trace.loading).
						OnClick(func() { p.inspectTranscriptTool(current, row.ID, tool) })
				}
			})
		})
		return true
	}
}

func (p *NativePage) transcriptAuthor(row view.Message) (string, string) {
	// The block carries only its attempt's unambiguous committed origin.
	block, exists := p.transcriptBlocks[row.ID]
	if !exists || block.SessionID != p.selected || block.NativeSessionID == "" {
		return "Alva", ""
	}
	var target hub.Target
	found := false
	for _, branch := range p.detail.Branches {
		if branch.SessionID != block.SessionID || branch.NativeSessionID != block.NativeSessionID {
			continue
		}
		if found && hub.RouteKey(target) != hub.RouteKey(branch.Target) {
			return "Alva", ""
		}
		target, found = branch.Target, true
	}
	if !found {
		return "Alva", ""
	}
	author := target.EngineID
	switch author {
	case "alva":
		author = "Alva"
	case "codex":
		author = "Codex"
	case "claude":
		author = "Claude"
	case "pi":
		author = "Pi"
	}
	if author == "" {
		author = "Alva"
	}
	model := target.ModelID
	if model == "engine-default" {
		model = ""
	}
	return author, model
}

func (p *NativePage) copyTranscriptMessage(c *ui.Context, binding workspaceTranscriptBinding, id string) {
	if binding != p.transcriptBinding() {
		return
	}
	if block, exists := p.transcriptBlocks[id]; exists && block.Text != "" {
		c.WriteClipboard(block.Text)
		c.Announce("已复制消息")
	}
}

func (p *NativePage) buildWorkspaceTranscriptMessage(c *ui.Context, row view.Message, binding workspaceTranscriptBinding, options *markdown.Options) {
	author, model := p.transcriptAuthor(row)
	assistant := row.Role == "Alva"
	ui.Row(c).Label("Message turn " + row.ID).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
		if assistant {
			mark := "a"
			if author != "Alva" {
				r, _ := utf8.DecodeRuneInString(author)
				mark = strings.ToUpper(string(r))
			}
			ui.Box(c).Label("Message avatar "+row.ID).Size(32, 32).Shrink(0).Radius(8).
				Background(workspacePalette(c).selection).Border(1, c.Theme().Border).
				AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				ui.Text(c, mark).Label("Message avatar glyph " + row.ID).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).TextColor(c.Theme().Accent)
			})
		} else {
			ui.Box(c).Width(32).Shrink(0)
		}
		ui.Column(c).Label("Message content " + row.ID).FillWidth().MinWidth(0).Gap(12).Children(func() {
			ui.Row(c).Label("Message header " + row.ID).FillWidth().Height(32).Gap(8).AlignItems(ui.Center).Children(func() {
				label := row.Role
				if assistant {
					label = author
				}
				name := ui.Text(c, label).Label("Message author " + row.ID).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).MinWidth(0).Shrink(1).SingleLine()
				if label == "Alva" || label == "Codex" || label == "Claude" || label == "Pi" {
					name.Shrink(0)
				}
				if assistant && model != "" {
					ui.Text(c, model).Label("Message model " + row.ID).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).MinWidth(0).Shrink(1).SingleLine()
				}
				ui.Box(c).Grow(1)
				if row.Text != "" {
					workspaceIconAction(c, "message-more", "Message actions "+row.ID, "ellipsis").Shrink(0).Menu(func(m *ui.Menu) {
						if m.Item("复制消息").Chosen() {
							p.copyTranscriptMessage(c, binding, row.ID)
						}
					})
				}
			})
			buildWorkspaceTranscriptMarkdown(c, row, options)
			if p.transcriptBlocks[row.ID].Truncated {
				ui.Text(c, "内容较长，这里只显示开头部分；完整内容保存在原始记录中。").Label("Message truncated " + row.ID).
					FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
			}
			if row.Text != "" {
				ui.Row(c).Label("Message content actions " + row.ID).FillWidth().Gap(8).Children(func() {
					workspaceIconAction(c, "message-copy", "Copy message "+row.ID, "copy").Tooltip("复制消息").OnClick(func() {
						p.copyTranscriptMessage(c, binding, row.ID)
					})
				})
			}
		})
	})
}

func buildWorkspaceTranscriptMarkdown(c *ui.Context, row view.Message, options *markdown.Options) {
	if options == nil {
		options = &markdown.Options{}
	}
	previous := options.Theme
	theme := markdown.Theme{}
	if previous != nil {
		theme = *previous
	}
	theme.FontSize, theme.LineHeight = c.Theme().FontSize, 1.75
	if row.Status == view.Failed {
		theme.Text = c.Theme().Danger
	}
	options.Theme = &theme
	defer func() { options.Theme = previous }()
	if row.Text == "" && (row.Status == view.Running || row.Status == view.Stopping) {
		ui.Text(c, "正在整理思路…").FontSize(c.Theme().FontSize).LineHeight(1.75).TextColor(c.Theme().TextMuted)
	} else if row.Status == view.Running || row.Status == view.Stopping {
		markdown.Stream(c, row.Text, options)
	} else {
		markdown.View(c, row.Text, options)
	}
	if row.Error != "" {
		ui.Text(c, row.Error).TextColor(c.Theme().Danger).FontSize(workspaceFontCaption)
	}
}

func (p *NativePage) transcriptToolBranch(tool transcriptTool) string {
	nativeID := tool.nativeID
	if tool.hasRef && tool.reference.SessionID != "" {
		nativeID = tool.reference.SessionID
	}
	for _, branch := range p.detail.Branches {
		if nativeID != "" && branch.SessionID == tool.sessionID && branch.NativeSessionID == nativeID {
			return branch.ID
		}
	}
	return ""
}

// inspectTranscriptTool switches the middle window to the trace of the tool's
// own native branch and selects its step. The block's Turn and history
// position bind the jump; a tool without a reference opens the branch tail.
func (p *NativePage) inspectTranscriptTool(binding workspaceTranscriptBinding, rowID string, tool transcriptTool) {
	if binding != p.transcriptBinding() || tool.sessionID != p.selected || p.transcriptTools[rowID] != tool {
		return
	}
	branch := p.transcriptToolBranch(tool)
	if branch == "" {
		p.problem = "这条工具记录没有可用的原始分支。"
		return
	}
	block := p.transcriptBlocks[rowID]
	target := product.TraceTarget{SessionID: binding.session, BranchID: branch}
	p.trace.active = true
	if !sameTraceTarget(p.trace.target, target) || len(p.trace.frames) > 0 {
		p.trace.frames = nil
	}
	if !tool.hasRef || tool.reference.Sequence == 0 {
		p.openTrace(target, 0, product.TraceHint{})
		return
	}
	p.openTrace(target, tool.reference.Sequence, product.TraceHint{Turn: block.Turn, History: block.History})
}
