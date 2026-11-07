// INPUT: Bounded Hub transcript rows, typed native references and existing owned Inspector reads.
// OUTPUT: Canonical message identities, reply columns, copy actions and explicitly disclosed tool activity.
// POS: [L7 Hub UI] Keeps product presentation and tool details separate from shared scrolling and Markdown.
//go:build desktop

package hubdesktop

import (
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

type workspaceTranscriptBinding struct {
	session, branch string
	generation      uint64
}

func (p *NativePage) buildWorkspaceTranscript(c *ui.Context) {
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
		if _, exists := p.transcript.tools[expanded]; !exists {
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
		tool, exists := p.transcript.tools[row.ID]
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
					if p.transcriptBinding() != current || p.transcript.tools[row.ID] != tool {
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
					ui.Button(c, "查看完整记录").Label("Inspect tool result " + row.ID).Disabled(p.paneBusy).
						OnClick(func() { p.inspectTranscriptTool(current, row.ID, tool) })
				}
			})
		})
		return true
	}
}

func (p *NativePage) transcriptAuthor(row view.Message) (string, string) {
	origin, exists := p.transcript.origins[row.RunID]
	if !exists || origin.ambiguous || origin.sessionID != p.selected || origin.nativeID == "" {
		return "Alva", ""
	}
	var target hub.Target
	found := false
	for _, branch := range p.detail.Branches {
		if branch.SessionID != origin.sessionID || branch.NativeSessionID != origin.nativeID {
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
	for _, row := range p.transcript.Rows {
		if row.ID == id && row.Text != "" {
			c.WriteClipboard(row.Text)
			c.Announce("已复制消息")
			return
		}
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

func (p *NativePage) inspectTranscriptTool(binding workspaceTranscriptBinding, rowID string, tool transcriptTool) {
	if p.paneBusy || binding != p.transcriptBinding() || tool.sessionID != p.selected || p.transcript.tools[rowID] != tool {
		return
	}
	branch := p.transcriptToolBranch(tool)
	if branch == "" {
		p.problem = "这条工具记录没有可用的原始分支。"
		return
	}
	p.inspectBranch = branch
	p.inspectPath, p.inspectFrames, p.inspectAfter = nil, nil, nil
	p.inspectPage = product.InspectorPage{}
	p.inspectURI, p.inspectQuery, p.inspectLoadedQuery, p.inspectGroup = "", "", "", ""
	p.inspectText = agentcontext.TextPage{}
	if !tool.hasRef || tool.reference.URI == "" && tool.reference.Sequence == 0 {
		p.openInspector()
		return
	}
	p.setPane("inspector")
	if tool.reference.URI != "" {
		p.readTranscriptTool(binding, rowID, tool, branch, tool.reference.URI)
		return
	}
	// Native event references carry record identity, not a readable URI. Resolve
	// the exact committed metadata through the product's existing conversion.
	p.paneBusy = true
	go func() {
		page, err := p.Service.Inspector(p.ctx, binding.session, branch, "", tool.reference.Sequence-1, tool.reference.Sequence)
		p.dispatch(func() {
			p.paneBusy = false
			if binding != p.transcriptBinding() || p.pane != "inspector" || p.inspectBranch != branch || p.transcript.tools[rowID] != tool {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.inspectPage = page
			p.inspectCursor = tool.reference.Sequence - 1
			for _, entry := range page.Entries {
				if entry.ID == tool.reference.RecordID && entry.Sequence == tool.reference.Sequence && entry.Value.URI != "" {
					p.readTranscriptTool(binding, rowID, tool, branch, entry.Value.URI)
					return
				}
			}
			p.problem = "原始工具记录当前不可用，请在检查器中刷新。"
		})
	}()
}

func (p *NativePage) readTranscriptTool(binding workspaceTranscriptBinding, rowID string, tool transcriptTool, branch, uri string) {
	p.inspectURI = uri
	p.readInspectorBound(uri, 0, func() bool {
		return binding == p.transcriptBinding() && p.pane == "inspector" && p.inspectBranch == branch && p.inspectURI == uri && p.transcript.tools[rowID] == tool
	})
}
