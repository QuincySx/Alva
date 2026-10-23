// INPUT: Explicit native file selection, page-local pending paths and shared HubService reference drafts.
// OUTPUT: Deferred home-file import, truthful pending/saved chips and branch-compatible attachment admission.
// POS: [L7 Hub UI] Keeps selected originals untouched until Send and removes only page selections or draft references.
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
	if p.busy || p.window == nil || p.inputBlocked() || p.workspaceComposerTarget().EngineID != "alva" {
		return
	}
	if p.selected != "" {
		p.upload()
		return
	}
	p.busy = true
	go func() {
		paths, err := mygo.Dialog.Open(mygo.OpenDialogOptions{Parent: p.window, Title: "选择待导入文件 — 明确发送时保存原件", Multiple: true})
		p.dispatch(func() {
			p.busy = false
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.retainWorkspaceFiles("", paths)
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
	if p.workspacePendingFiles() && p.workspaceComposerTarget().EngineID != "alva" {
		return "待导入文件仅支持 Alva 引擎，请选择 Alva 或移除待导入文件。"
	}
	files := p.Service.DraftAttachments(p.selected)
	if len(files) == 0 || p.detail.Session.ID != p.selected {
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
	return ""
}

func (p *NativePage) buildWorkspaceAttachments(c *ui.Context) {
	pending, saved := p.workspaceFiles[p.selected], p.Service.DraftAttachments(p.selected)
	if len(pending) == 0 && len(saved) == 0 {
		return
	}
	t, colors := c.Theme(), workspacePalette(c)
	blocked := p.busy || p.inputBlocked()
	ui.Row(c).FillWidth().Wrap().Gap(6).Children(func() {
		for i, file := range pending {
			name := filepath.Base(file.path)
			button := ui.ButtonBase(c.Key(fmt.Sprintf("pending-%d", i))).Label("Remove pending file "+name).
				Padding(6, 8).Gap(6).Radius(8).Border(1, t.Border).Background(colors.subtle).
				Disabled(blocked).Tooltip(name + " · 待导入，发送时才读取并保存原件；点击仅移除此选择").
				OnClick(func() {
					files := p.workspaceFiles[p.selected]
					p.workspaceFiles[p.selected] = append(files[:i], files[i+1:]...)
				})
			button.Children(func() {
				ui.Icon(c, workspaceIcon("paperclip")).Size(14, 14)
				ui.Text(c, name).FontSize(12).SingleLine().MaxWidth(180)
				ui.Text(c, "待导入").FontSize(12).TextColor(t.TextMuted)
				ui.Icon(c, workspaceIcon("x")).Size(14, 14)
			})
		}
		for _, attachment := range saved {
			button := ui.ButtonBase(c.Key(attachment.Info.ID)).Label("Remove draft reference "+attachment.Info.Name).
				Padding(6, 8).Gap(6).Radius(8).Border(1, t.Border).Background(colors.subtle).
				Disabled(blocked).Tooltip(fmt.Sprintf("%s · %d bytes · 原件已保存，发送时仅附上引用；点击移除此草稿引用", attachment.Info.Name, attachment.Info.Bytes)).
				OnClick(func() { p.Service.RemoveDraftAttachment(p.selected, attachment.Info.ID) })
			button.Children(func() {
				ui.Icon(c, workspaceIcon("paperclip")).Size(14, 14)
				ui.Text(c, attachment.Info.Name).FontSize(12).SingleLine().MaxWidth(180)
				ui.Icon(c, workspaceIcon("x")).Size(14, 14)
			})
		}
	})
}

func (p *NativePage) importWorkspaceFiles(id string, files []workspaceSelectedFile) (int, error) {
	for i, file := range files {
		if _, err := p.Service.Upload(p.ctx, id, file.path); err != nil {
			return i, err
		}
	}
	return len(files), nil
}
