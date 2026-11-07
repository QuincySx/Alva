// INPUT: Explicit native file selection, durable captured files and legacy page-local reference receipts.
// OUTPUT: Immediate pre-session file capture, truthful pending/saved Attachment cards and exact captured selection admission.
// POS: [L7 Hub UI] Retains selected originals immediately for every engine and removes only draft associations.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"path/filepath"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type workspaceSelectedFile struct {
	path string
}

func (p *NativePage) chooseWorkspaceFiles() {
	if p.busy || p.window == nil || p.inputBlocked() {
		return
	}
	id, generation := p.selected, p.generation
	p.busy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Title: "选择文件 — 立即保存私有原件，最多 256 MiB/文件", Multiple: true})
		p.dispatch(func() {
			p.busy = false
			if err != nil {
				p.problem = err.Error()
				return
			}
			if p.selected != id || p.generation != generation {
				p.problem = "选择文件期间会话已变化，请在当前草稿重新选择。"
				return
			}
			p.captureWorkspaceFilePaths(paths)
		})
	}()
}

// Selection stores paths only. Upload remains the sole authority for opening,
// validating and saving original bytes and enforcing the per-Turn budgets.
func (p *NativePage) retainWorkspaceFiles(id string, paths []string) {
	if p.workspaceFiles == nil {
		p.workspaceFiles = make(map[string][]workspaceSelectedFile)
	}
	for _, path := range paths {
		p.workspaceFiles[id] = append(p.workspaceFiles[id], workspaceSelectedFile{path: path})
	}
}

func (p *NativePage) workspacePendingFiles() bool {
	return len(p.workspaceFiles[p.selected]) != 0
}

func (p *NativePage) workspaceAttachmentProblem() string {
	if problem := p.workspaceCapturedFileProblem(); problem != "" {
		return problem
	}
	if p.workspacePlanEnabled() && p.workspaceComposerTarget().EngineID != "alva" {
		return "只读计划仅支持 Alva 引擎，请切回 Alva 或退出计划模式。"
	}
	if p.workspacePendingFiles() && p.workspaceComposerTarget().EngineID != "alva" {
		return "待导入文件仅支持 Alva 引擎，请选择 Alva 或移除待导入文件。"
	}
	files := p.workspaceDraftAttachments(p.selected)
	refs := p.workspaceDraftReferences(p.selected)
	if len(p.workspaceReferences[p.selected]) != len(files)+len(refs) {
		return "草稿引用已失效，请移除失效引用后重新选择；已保存原件保留。"
	}
	if len(files)+len(refs) == 0 || p.detail.Session.ID != p.selected {
		return ""
	}
	branch, err := product.ActiveBranch(p.detail)
	if err != nil {
		return "附件所属分支尚未加载，请重新加载会话。"
	}
	for _, file := range files {
		if file.BranchID != branch.ID || file.Info.SessionID != branch.NativeSessionID {
			return "附件引用属于其他分支，请切回原分支或移除草稿引用后再发送。"
		}
	}
	for _, ref := range refs {
		if ref.Selection.BranchID != branch.ID || ref.Info.SessionID != branch.NativeSessionID {
			return "项目引用属于其他分支，请切回原分支或移除引用。"
		}
	}
	return ""
}

func (p *NativePage) buildWorkspaceAttachments(c *ui.Context) {
	pending, saved := p.workspaceFiles[p.selected], p.workspaceDraftAttachments(p.selected)
	refs := p.workspaceDraftReferences(p.selected)
	known := make(map[product.DraftReferenceSelection]bool, len(saved)+len(refs))
	for _, file := range saved {
		known[file.DraftSelection()] = true
	}
	for _, reference := range refs {
		known[reference.DraftSelection()] = true
	}
	var missing []product.DraftReferenceSelection
	for _, reference := range p.workspaceReferences[p.selected] {
		if !known[reference] {
			missing = append(missing, reference)
		}
	}
	if len(pending)+len(saved)+len(refs)+len(missing)+len(p.composerSelections[p.selected])+len(p.composerFiles[p.selected])+len(p.composerFilePending[p.selected]) == 0 && p.voice.pending == "" && p.voice.problem == "" && p.voice.recorder == nil {
		return
	}
	blocked := p.busy || p.inputBlocked()
	// References and dictation share one viewport budget. Independently capped
	// sections could still crowd the message and toolbar out of the composer.
	ui.Scroll(c.Key("composer-references")).Label("Composer attachments").FillWidth().MinWidth(0).Gap(8).MaxHeight(59*2 + 8).Children(func() {
		p.buildWorkspaceVoiceNotice(c)
		if len(p.composerSelections[p.selected]) != 0 {
			ui.Row(c.Key("prepared-references")).FillWidth().MinWidth(0).Wrap().Gap(8).AlignItems(ui.Center).Children(func() {
				p.buildWorkspaceSelections(c)
			})
		}
		if len(pending)+len(saved)+len(refs)+len(missing)+len(p.composerFiles[p.selected])+len(p.composerFilePending[p.selected]) == 0 {
			return
		}
		ui.Row(c.Key("attachment-cards")).FillWidth().MinWidth(0).Wrap().Gap(8).Children(func() {
			p.buildWorkspaceCapturedFileCards(c)
			for _, reference := range missing {
				workspaceAttachmentCard(c, reference.ID, "失效引用", "请移除后重新选择", "file-text",
					"Remove unavailable reference "+reference.ID, "该草稿引用已移除或版本已失效；移除提示保留已保存原件", blocked,
					func() { p.consumeWorkspaceReferences(p.selected, []product.DraftReferenceSelection{reference}) })
			}
			for _, ref := range refs {
				icon := "file-text"
				if ref.Selection.Kind == "directory" {
					icon = "folder"
				}
				workspaceAttachmentCard(c, ref.DraftID, "@"+ref.Selection.Path, "项目引用 · 已保存", icon,
					"Remove project reference "+ref.Selection.Path, ref.Selection.Path+" · "+ref.Info.Processing, blocked,
					func() {
						p.Service.RemoveDraftWorkspaceReference(p.selected, ref.DraftID)
						p.consumeWorkspaceReferences(p.selected, []product.DraftReferenceSelection{ref.DraftSelection()})
					})
			}
			for i, file := range pending {
				name := filepath.Base(file.path)
				workspaceAttachmentCard(c, fmt.Sprintf("pending-%d", i), name, "待导入", "file-text",
					"Remove pending file "+name, file.path+" · 待导入，发送时才读取并保存原件；移除仅取消此选择", blocked, func() {
						files := p.workspaceFiles[p.selected]
						p.workspaceFiles[p.selected] = append(files[:i], files[i+1:]...)
					})
			}
			for _, attachment := range saved {
				workspaceAttachmentCard(c, attachment.DraftID, attachment.Info.Name,
					fmt.Sprintf("已保存 · %d bytes", attachment.Info.Bytes), "file-text",
					"Remove draft reference "+attachment.Info.Name,
					attachment.Info.Name+" · "+attachment.Info.Processing+" · 发送时仅附上引用；移除保留已保存原件", blocked,
					func() {
						p.Service.RemoveDraftAttachment(p.selected, attachment.DraftID)
						p.consumeWorkspaceReferences(p.selected, []product.DraftReferenceSelection{attachment.DraftSelection()})
					})
			}
		})
	})
}

// cmp_alva_attachment.Ready: a 16px File slot, a natural two-line Copy
// (13/1.5 + 4 + 11/1.5), and a separate 28px Remove action inside 8px insets.
// Pending paths share the resource geometry without claiming upload progress.
func workspaceAttachmentCard(c *ui.Context, key, name, meta, icon, label, hint string, blocked bool, remove func()) {
	t := c.Theme()
	ui.Row(c.Key("attachment-" + key)).Label("Attachment " + name).Width(320).MaxWidthPercent(100).MinWidth(0).MinHeight(56).
		Padding(8).Gap(8).AlignItems(ui.Center).Radius(8).Background(t.Surface).Tooltip(hint).
		DrawOver(func(painter *ui.Painter, bounds ui.Rect) { painter.Stroke(bounds, t.Border, 8, 1) }).Children(func() {
		ui.Icon(c, workspaceIcon(icon)).Label("Attachment source icon "+key).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(t.Accent)
		ui.Column(c).Label("Attachment copy " + key).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, name).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).TextColor(t.Text).SingleLine().FillWidth().MinWidth(0)
			ui.Text(c, meta).FontSize(workspaceFontCaption).FontWeight(400).LineHeight(1.5).TextColor(t.TextMuted).SingleLine().FillWidth().MinWidth(0)
		})
		workspaceIconAction(c, "remove-"+key, label, "x").Shrink(0).Disabled(blocked).
			Tooltip("移除此草稿引用；保留原件").OnClick(remove)
	})
}

func (p *NativePage) importWorkspaceFiles(id string, files []workspaceSelectedFile) ([]product.DraftReferenceSelection, error) {
	var references []product.DraftReferenceSelection
	for _, file := range files {
		attachment, err := p.Service.Upload(p.ctx, id, file.path)
		if err != nil {
			return references, err
		}
		references = append(references, attachment.DraftSelection())
		p.referenceOwner.retain(id, []product.DraftReferenceSelection{attachment.DraftSelection()})
	}
	return references, nil
}

func (p *NativePage) workspaceDraftAttachments(id string) []product.DraftAttachment {
	var files []product.DraftAttachment
	for _, draft := range p.Service.DraftAttachments(id) {
		for _, selection := range p.workspaceReferences[id] {
			if draft.DraftSelection() == selection {
				files = append(files, draft)
				break
			}
		}
	}
	return files
}

func (p *NativePage) workspaceDraftReferences(id string) []product.DraftWorkspaceReference {
	var references []product.DraftWorkspaceReference
	for _, draft := range p.Service.DraftWorkspaceReferences(id) {
		for _, selection := range p.workspaceReferences[id] {
			if draft.DraftSelection() == selection {
				references = append(references, draft)
				break
			}
		}
	}
	return references
}
