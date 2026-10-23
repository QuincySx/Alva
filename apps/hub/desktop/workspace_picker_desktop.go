// INPUT: MyGo native editor events and shared product command/Skill/project-reference services
// OUTPUT: Scoped cancellable composer picker, typed draft selections and explicit bounded pagination
// POS: [L7 Hub UI] Selection prepares drafts only; Build renders cached results and never sends a Turn
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"path"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/egoist/mygo/ui"
)

type workspacePickerRow struct {
	command   *product.ComposerCatalogEntry
	reference *product.WorkspaceReferenceSelection
}

func (r workspacePickerRow) identity() string {
	if r.command != nil {
		return r.command.ID
	}
	return r.reference.ID
}

func (r workspacePickerRow) available() bool { return r.command == nil || r.command.Available }

type workspacePickerState struct {
	open, loading, preparing, explicit bool
	kind, query, scope, problem        string
	rows                               []workspacePickerRow
	active, offset, total              int
	next                               string
	hasMore, truncated                 bool
	sequence                           uint64
	cancel                             context.CancelFunc
	requestContext                     context.Context
	editor                             ui.Handle
	lastDraft                          string
	revealed                           string
	observed, composing, pasted        bool
	start, end, caret                  int
	argument                           string
	detail                             *product.ComposerEntryDetail
	detailLoading                      bool
	detailSequence                     uint64
}

func (p *NativePage) workspacePickerScope() string {
	return p.pane + "\x00" + p.workspace + "\x00" + p.selected + "\x00" + p.detail.Session.ActiveBranchID + "\x00" + hub.RouteKey(p.workspaceComposerTarget())
}

func (p *NativePage) workspacePickerBlocked() bool {
	return p.busy || p.workspaceComposerRunning() || p.inputBlocked() || p.selected != "" && (p.detail.Session.ID != p.selected || p.chat.Busy)
}

func (p *NativePage) closeWorkspacePicker() {
	if p.picker.cancel != nil {
		p.picker.cancel()
		p.picker.cancel = nil
	}
	p.picker.open, p.picker.loading, p.picker.preparing = false, false, false
	p.picker.sequence++
	p.picker.detailSequence++
}

func (p *NativePage) openWorkspacePicker(kind string) {
	if p.workspacePickerBlocked() || kind != "commands" && kind != "skills" && kind != "references" {
		return
	}
	p.closeWorkspacePicker()
	p.composer.closePopovers()
	p.picker.open, p.picker.explicit = true, true
	p.picker.kind, p.picker.query, p.picker.argument = kind, "", ""
	p.picker.scope = p.workspacePickerScope()
	p.picker.start, p.picker.end, p.picker.caret = -1, -1, -1
	p.picker.lastDraft, p.picker.observed = p.model.Draft, true
	p.picker.editor.RequestFocus()
	p.queryWorkspacePicker(false)
}

func (p *NativePage) queryWorkspacePicker(more bool) {
	s := &p.picker
	if !s.open || p.workspacePickerBlocked() {
		return
	}
	if s.cancel != nil {
		s.cancel()
	}
	s.sequence++
	s.detailSequence++
	s.loading, s.preparing, s.detailLoading = true, false, false
	s.problem, s.detail = "", nil
	if !more {
		s.offset, s.next = 0, ""
	} else {
		s.offset += len(s.rows)
	}
	s.active, s.revealed = 0, ""
	ctx, cancel := context.WithCancel(p.ctx)
	s.cancel = cancel
	s.requestContext = ctx
	sequence, scope, id, kind, query, offset, after := s.sequence, s.scope, p.selected, s.kind, s.query, s.offset, s.next
	if kind == "references" && id == "" {
		s.loading, s.rows, s.hasMore = false, nil, false
		s.problem = "请先选择已有项目会话，再搜索项目文件或目录；此操作不会自动创建会话。"
		return
	}
	go func() {
		var rows []workspacePickerRow
		var next string
		var total int
		var hasMore, truncated bool
		var err error
		if kind == "references" {
			var page product.WorkspaceReferencePage
			page, err = p.Service.SearchWorkspaceReferences(ctx, id, query, after, 20)
			for _, entry := range page.Entries {
				value := entry
				rows = append(rows, workspacePickerRow{reference: &value})
			}
			next, hasMore, truncated = page.Next, page.HasMore, page.Truncated
		} else {
			filter := product.ComposerEntryKind("")
			if kind == "skills" {
				filter = product.ComposerSkillEntry
			}
			var page product.ComposerCatalogPage
			page, err = p.Service.ComposerCatalog(ctx, id, product.ComposerCatalogQuery{Kind: filter, Search: query, Offset: offset, Limit: 20})
			for _, entry := range page.Entries {
				value := entry
				rows = append(rows, workspacePickerRow{command: &value})
			}
			total, hasMore = page.Total, offset+len(page.Entries) < page.Total
		}
		p.dispatchWorkspacePicker(ctx, func() {
			if !p.picker.open || p.picker.sequence != sequence || p.picker.scope != scope || p.workspacePickerScope() != scope || ctx.Err() != nil {
				return
			}
			p.picker.loading = false
			if err != nil {
				p.picker.problem, p.picker.rows = err.Error(), nil
				return
			}
			p.picker.rows, p.picker.next, p.picker.total = rows, next, total
			p.picker.hasMore, p.picker.truncated = hasMore, truncated
			p.moveWorkspacePicker(0)
		})
	}()
}

func (p *NativePage) moveWorkspacePicker(delta int) {
	s := &p.picker
	if len(s.rows) == 0 {
		return
	}
	index := min(max(s.active+delta, 0), len(s.rows)-1)
	step := 1
	if delta < 0 {
		step = -1
	}
	for index >= 0 && index < len(s.rows) {
		if s.rows[index].available() {
			s.active = index
			return
		}
		index += step
	}
	s.active = -1
}

func (p *NativePage) workspacePickerKey(k ui.TextKey) bool {
	if !p.picker.open || k.Mods != 0 {
		return false
	}
	switch k.Key {
	case ui.KeyEscape:
		p.closeWorkspacePicker()
		return true
	case ui.KeyUp:
		p.moveWorkspacePicker(-1)
		return true
	case ui.KeyDown:
		p.moveWorkspacePicker(1)
		return true
	case ui.KeyEnter:
		if k.Text() != p.model.Draft {
			return true
		}
		p.chooseWorkspacePicker()
		return true
	}
	return false
}

// workspacePickerInsertion accepts a newly typed single trigger at a word
// boundary. Paste, existing text, email/URL interiors and multi-rune inserts do
// not open a picker. Offsets use the native editor's rune coordinates.
func workspacePickerInsertion(before, after string, caret int) (int, string) {
	left, right := []rune(before), []rune(after)
	if len(right) != len(left)+1 || caret < 1 || caret > len(right) {
		return 0, ""
	}
	position := caret - 1
	if string(left[:min(position, len(left))]) != string(right[:position]) || string(left[min(position, len(left)):]) != string(right[caret:]) {
		return 0, ""
	}
	if position > 0 && !unicode.IsSpace(right[position-1]) && !strings.ContainsRune("([{", right[position-1]) {
		return 0, ""
	}
	switch right[position] {
	case '/':
		return position, "commands"
	case '@':
		return position, "references"
	}
	return 0, ""
}

func (p *NativePage) buildWorkspacePicker(c *ui.Context, editor, composer ui.Element) {
	s := &p.picker
	editor.Bind(&s.editor).OnTextKey(p.workspacePickerKey).OnPaste(func(text string) bool {
		s.pasted = true
		if len(text) <= view.MaxDraftBytes {
			return false
		}
		p.problem = "粘贴内容超过 32 KiB，请通过添加菜单附上文件。草稿未发送。"
		return true
	})
	changed, composing := editor.Changed(), editor.Composing()
	start, caret := editor.TextSelection()
	scope := p.workspacePickerScope()
	if s.open && (scope != s.scope || p.workspacePickerBlocked()) {
		p.closeWorkspacePicker()
	}
	if !s.observed {
		s.lastDraft, s.observed, s.caret = p.model.Draft, true, -1
	}
	if s.caret >= 0 {
		editor.SetTextSelection(s.caret, s.caret)
		s.caret = -1
	}
	if changed && !composing && !s.composing && !s.pasted && !p.workspacePickerBlocked() {
		if !s.open {
			if trigger, kind := workspacePickerInsertion(s.lastDraft, p.model.Draft, caret); kind != "" && start == caret {
				p.openWorkspacePicker(kind)
				s.explicit, s.start, s.end = false, trigger, caret
			}
		} else if !s.explicit {
			runes := []rune(p.model.Draft)
			if start != caret || s.start < 0 || caret <= s.start || caret > len(runes) || runes[s.start] != '/' && runes[s.start] != '@' {
				p.closeWorkspacePicker()
			} else {
				query := string(runes[s.start+1 : caret])
				if strings.ContainsAny(query, "\r\n\t ") || s.kind == "commands" && strings.ContainsAny(query, "/\\:") {
					p.closeWorkspacePicker()
				} else if query != s.query {
					s.query, s.end = query, caret
					p.queryWorkspacePicker(false)
				}
			}
		}
	}
	if s.open && !s.explicit && editor.Focused() && !changed && (start != caret || caret < s.start+1 || caret > s.end) {
		p.closeWorkspacePicker()
	}
	s.lastDraft, s.composing, s.pasted = p.model.Draft, composing, false
	if !s.open {
		return
	}
	// MyGo shares the placement anchor with keyboard/dismissal ownership.
	// Keep the input as owner and offset its popup to the outer card boundary.
	_, viewport := c.Size()
	bounds, inputBounds := composer.Bounds(), editor.Bounds()
	above, below := max(float32(0), bounds.Y-10), max(float32(0), viewport-bounds.Y-bounds.H-10)
	available := min(float32(360), above)
	belowComposer := above < 160 && below > above
	if belowComposer {
		available = min(360, below)
	}
	panel := ui.PopoverBase(c.Key("workspace-picker"), editor, &s.open, func(panel ui.Element) {
		panel.Label("Workspace picker").Width(600).MaxWidthPercent(95).MaxHeight(available).
			Padding(10).Gap(8).Radius(12).Background(c.Theme().Surface).Border(1, c.Theme().Border)
		if belowComposer {
			panel.AttachTo(editor, ui.AnchorBottomLeft, ui.AnchorTopLeft).
				Margin(6+bounds.Y+bounds.H-inputBounds.Y-inputBounds.H, 0, 0, bounds.X-inputBounds.X)
		} else {
			panel.AttachTo(editor, ui.AnchorTopLeft, ui.AnchorBottomLeft).
				Margin(0, 0, 6+inputBounds.Y-bounds.Y, bounds.X-inputBounds.X)
		}
		// Bounds read during Build belong to the previous layout. Request one
		// settling frame only when current layout changes these relative offsets.
		panel.DrawOver(func(_ *ui.Painter, _ ui.Rect) {
			if composer.Bounds() != bounds || editor.Bounds() != inputBounds {
				c.AnimationFrame()
			}
		})
		ui.Scroll(c.Key("picker-content")).Label("Composer picker contents").FillWidth().MaxHeight(max(0, available-22)).Gap(8).Children(func() {
			query := ui.TextInputBase(c.Key("picker-query"), &s.query).Label("Search composer items").Placeholder("搜索命令、Skills 或项目路径").
				FillWidth().Padding(8).Border(1, c.Theme().Border).Radius(6).OnTextKey(p.workspacePickerKey)
			if query.Changed() && !query.Composing() {
				p.queryWorkspacePicker(false)
			}
			if s.problem != "" {
				ui.Text(c, s.problem).FontSize(13).LineHeight(1.5).TextColor(c.Theme().TextMuted)
				ui.Button(c, "重试搜索").Label("Retry composer search").OnClick(func() { p.queryWorkspacePicker(false) })
			} else if s.loading {
				ui.Text(c, "正在搜索…").Role(ui.RoleStatus).FontSize(13)
			} else if len(s.rows) == 0 {
				ui.Text(c, "没有匹配项目").FontSize(13).TextColor(c.Theme().TextMuted)
			} else {
				p.buildWorkspacePickerRows(c, available-110)
			}
			if s.preparing {
				ui.Text(c, "正在准备引用…").Role(ui.RoleStatus).FontSize(13)
			}
			if s.hasMore && !s.loading {
				ui.Button(c, "更多结果").Label("More composer results").OnClick(func() { p.queryWorkspacePicker(true) })
			}
			if s.truncated {
				ui.Text(c, "已达到项目索引上限；请缩小查询范围。").FontSize(12).TextColor(c.Theme().TextMuted)
			}
		})
	})
	if panel.Dismissed() {
		p.closeWorkspacePicker()
	}
}

func (p *NativePage) buildWorkspacePickerRows(c *ui.Context, height float32) {
	s := &p.picker
	listHeight := max(float32(80), height-110)
	if s.detail != nil && s.detail.Document != nil {
		listHeight = max(80, listHeight-64)
	}
	ui.Scroll(c.Key("picker-results")).Label("Composer picker results").FillWidth().MaxHeight(listHeight).Children(func() {
		ui.Column(c).FillWidth().Gap(4).Children(func() {
			for index, row := range s.rows {
				name, description := row.referenceName(), ""
				if row.command != nil {
					name, description = "/"+row.command.Name, row.command.Description
					if !row.command.Available {
						description = row.command.UnavailableReason
					}
				}
				button := ui.ButtonBase(c.Key(row.identity())).Label("Select " + name).Role(ui.RoleListItem).Selected(index == s.active).
					KeepFocus().FillWidth().Padding(8).Radius(6).Disabled(!row.available() || s.preparing).
					OnClick(func() { s.active = index; p.chooseWorkspacePicker() }).HandleInput(func(ev ui.InputEvent) bool {
					if ev.Kind == ui.InputPointerMove && ev.Button == -1 && row.available() {
						s.active = index
					}
					return false
				})
				if row.reference != nil {
					button.Label("Select @" + row.reference.Path).Tooltip(row.reference.Path)
				}
				if index == s.active {
					button.Background(workspacePalette(c).selection)
					if s.revealed != row.identity() {
						button.ScrollIntoView()
						s.revealed = row.identity()
					}
				} else if button.Hovered() {
					button.Background(workspacePalette(c).hover)
				}
				button.Children(func() {
					ui.Column(c).FillWidth().Gap(2).Children(func() {
						ui.Text(c, name).FontSize(14).TextColor(c.Theme().Text).SingleLine()
						if description != "" {
							ui.Text(c, description).FontSize(12).TextColor(c.Theme().TextMuted).LineHeight(1.4)
						}
					})
				})
			}
		})
	})
	if s.active < 0 || s.active >= len(s.rows) {
		return
	}
	row := s.rows[s.active]
	ui.Column(c.Key("picker-details")).Label("Composer active item details").FillWidth().Gap(4).Children(func() {
		if row.command == nil {
			ui.Text(c, fmt.Sprintf("%s · %s · %d bytes", row.reference.Kind, row.reference.Path, row.reference.Bytes)).FontSize(12).TextColor(c.Theme().TextMuted).SingleLine().Tooltip(row.reference.Path)
			return
		}
		ui.Text(c, row.command.Origin+" · "+row.command.Scope+" · "+row.command.Source).FontSize(12).TextColor(c.Theme().TextMuted).SingleLine()
		ui.TextInput(c.Key("picker-argument"), &s.argument).Label("Composer request argument").Placeholder("可选：补充任务或审查范围").FillWidth()
		ui.Button(c, "查看详情").Label("Composer item details").Disabled(s.detailLoading).OnClick(p.loadWorkspacePickerDetails)
		if s.detail != nil && s.detail.Entry.ID == row.command.ID && s.detail.Document != nil {
			body := []rune(s.detail.Document.Body)
			ui.Scroll(c.Key("picker-skill-document")).Label("Selected Skill document preview").FillWidth().MaxHeight(64).Children(func() {
				ui.Text(c, string(body[:min(len(body), 2048)])).FontSize(12).LineHeight(1.5)
			})
			ui.Text(c, "SHA-256: "+s.detail.SHA256).FontSize(11).TextColor(c.Theme().TextMuted).SingleLine()
		}
	})
}

func (r workspacePickerRow) referenceName() string {
	if r.reference == nil {
		return ""
	}
	return "@" + path.Base(r.reference.Path)
}

func (p *NativePage) loadWorkspacePickerDetails() {
	s := &p.picker
	if !s.open || s.loading || s.active < 0 || s.active >= len(s.rows) || s.rows[s.active].command == nil {
		return
	}
	s.detailSequence++
	s.detailLoading = true
	sequence, detailSequence, scope, id, entryID, ctx := s.sequence, s.detailSequence, s.scope, p.selected, s.rows[s.active].command.ID, s.requestContext
	go func() {
		detail, err := p.Service.ComposerEntry(ctx, id, entryID)
		p.dispatchWorkspacePicker(ctx, func() {
			if !p.picker.open || p.picker.sequence != sequence || p.picker.detailSequence != detailSequence || p.workspacePickerScope() != scope {
				return
			}
			p.picker.detailLoading = false
			if err != nil {
				p.picker.problem = err.Error()
				return
			}
			p.picker.detail = &detail
		})
	}()
}

func (p *NativePage) chooseWorkspacePicker() {
	s := &p.picker
	if !s.open || s.loading || s.preparing || p.workspacePickerBlocked() || s.active < 0 || s.active >= len(s.rows) || !s.rows[s.active].available() {
		return
	}
	if len(s.argument) > 8192 || !utf8.ValidString(s.argument) {
		s.problem = "补充参数最多 8 KiB，请缩短后重试。"
		return
	}
	if s.rows[s.active].command != nil {
		for _, preparation := range p.composerSelections[p.selected] {
			if preparation.EntryID == s.rows[s.active].command.ID {
				s.problem = "该命令或 Skill 已在草稿中，请先移除后重新选择。"
				return
			}
		}
		if len(p.composerSelections[p.selected]) >= 8 {
			s.problem = "每份草稿最多选择 8 项命令或 Skills，请先移除已有选择。"
			return
		}
	}
	s.preparing = true
	sequence, scope, id, draft, start, end, explicit, row, argument := s.sequence, s.scope, p.selected, p.model.Draft, s.start, s.end, s.explicit, s.rows[s.active], s.argument
	ctx := s.requestContext
	go func() {
		var preparation product.ComposerPreparation
		var err error
		if row.command != nil {
			preparation, err = p.Service.PrepareComposerSelection(ctx, id, product.ComposerSelectionRequest{EntryID: row.command.ID, Arguments: map[string]string{"request": argument}})
		} else {
			_, err = p.Service.PrepareWorkspaceReference(ctx, id, *row.reference)
		}
		p.dispatchWorkspacePicker(ctx, func() {
			if !p.picker.open || p.picker.sequence != sequence || p.workspacePickerScope() != scope {
				return
			}
			p.picker.preparing = false
			if err != nil {
				p.picker.problem = err.Error()
				return
			}
			if p.model.Draft != draft || p.workspacePickerBlocked() {
				p.picker.problem = "草稿或运行状态已变化，请检查当前草稿后重新选择。"
				return
			}
			text := ""
			if row.command != nil {
				text = preparation.Text
				if p.composerSelections == nil {
					p.composerSelections = make(map[string][]product.ComposerPreparation)
				}
				p.composerSelections[id] = append(p.composerSelections[id], preparation)
			}
			runes := []rune(draft)
			if explicit {
				start, end = len(runes), len(runes)
				if len(runes) > 0 && text != "" {
					text = "\n" + text
				}
			}
			if start >= 0 && end >= start && end <= len(runes) {
				p.model.Draft = string(runes[:start]) + text + string(runes[end:])
				p.picker.caret = start + utf8.RuneCountInString(text)
			}
			p.closeWorkspacePicker()
			p.picker.editor.RequestFocus()
		})
	}()
}

// A superseded query must not occupy the bounded headless UI queue while a
// newer query is waiting. Native windows keep their normal dispatch owner.
func (p *NativePage) dispatchWorkspacePicker(ctx context.Context, apply func()) {
	if ctx.Err() != nil {
		return
	}
	if p.window != nil {
		p.dispatch(apply)
		return
	}
	select {
	case p.updates <- apply:
	case <-ctx.Done():
	}
}
