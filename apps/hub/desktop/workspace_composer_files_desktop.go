// INPUT: Explicit native/candidate file choices, shared draft scope and immutable product file receipts.
// OUTPUT: Immediate streamed capture, truthful attachment cards and exact captured-file snapshot/consumption helpers.
// POS: [L7 Hub UI] File selection preserves manual text and originals; no picker or attachment action sends a Turn.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"path/filepath"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

type workspaceComposerPendingFile struct{ path, problem string }

func (p *NativePage) captureWorkspaceComposerFiles() []product.ComposerFileSelection {
	return append([]product.ComposerFileSelection(nil), p.composerFiles[p.selected]...)
}
func (p *NativePage) consumeWorkspaceComposerFiles(id string, sent []product.ComposerFileSelection) {
	seen := make(map[product.ComposerFileSelection]bool, len(sent))
	for _, selection := range sent {
		seen[selection] = true
	}
	current := p.composerFiles[id][:0]
	for _, selection := range p.composerFiles[id] {
		if !seen[selection] {
			current = append(current, selection)
		}
	}
	if p.composerFiles != nil {
		p.composerFiles[id] = current
	}
	for _, removed := range sent {
		retained := false
		for _, items := range p.composerFiles {
			for _, selection := range items {
				if selection.ID == removed.ID {
					retained = true
					break
				}
			}
			if retained {
				break
			}
		}
		if !retained {
			delete(p.composerFileRecords, removed.ID)
		}
	}
}
func (p *NativePage) retainWorkspaceComposerFiles(id string, files []hub.ComposerFile) {
	if p.composerFiles == nil {
		p.composerFiles = make(map[string][]product.ComposerFileSelection)
	}
	if p.composerFileRecords == nil {
		p.composerFileRecords = make(map[string]hub.ComposerFile)
	}
	for _, f := range files {
		selection := product.SelectedComposerFile(f)
		duplicate := false
		for _, existing := range p.composerFiles[id] {
			if existing == selection {
				duplicate = true
				break
			}
		}
		if !duplicate {
			p.composerFiles[id] = append(p.composerFiles[id], selection)
		}
		p.composerFileRecords[f.ID] = f
	}
}
func (p *NativePage) workspaceFileCandidateDirectory() string {
	if p.selected != "" && p.detail.Session.ID == p.selected {
		return p.detail.Session.Workspace
	}
	if p.workspace == "" {
		return ""
	}
	canonical, err := filepath.EvalSymlinks(p.workspace)
	if err != nil {
		return p.workspace
	}
	return canonical
}

// captureWorkspaceFilePaths is called after an explicit native selection.
// Busy remains visible until actual captures return; failures preserve earlier
// captures and the entire message, and never delete a selected original.
func (p *NativePage) captureWorkspaceFilePaths(paths []string) {
	if p.busy || p.inputBlocked() || len(paths) == 0 {
		return
	}
	id, scope, generation := p.selected, p.workspaceComposerDraftScope(), p.generation
	pendingPaths := make(map[string]bool)
	for _, pending := range p.composerFilePending[id] {
		pendingPaths[pending.path] = true
	}
	for _, path := range paths {
		pendingPaths[path] = true
	}
	if len(p.composerFiles[id])+len(pendingPaths) > 8 {
		p.problem = "每份草稿最多选择 8 个文件，请先移除部分引用。草稿文字已保留。"
		return
	}
	finish := p.referenceOwner.begin()
	if finish == nil {
		return
	}
	if p.composerFilePending == nil {
		p.composerFilePending = make(map[string][]workspaceComposerPendingFile)
	}
	for _, path := range paths {
		exists := false
		for _, pending := range p.composerFilePending[id] {
			if pending.path == path {
				exists = true
				break
			}
		}
		if !exists {
			p.composerFilePending[id] = append(p.composerFilePending[id], workspaceComposerPendingFile{path: path})
		}
	}
	p.busy = true
	p.problem = ""
	go func() {
		var files []hub.ComposerFile
		var capturedPaths []string
		var err error
		for _, path := range paths {
			var f hub.ComposerFile
			f, err = p.Service.CaptureComposerFile(p.ctx, scope, path)
			if err != nil {
				break
			}
			files = append(files, f)
			capturedPaths = append(capturedPaths, path)
		}
		finish()
		p.dispatch(func() {
			p.busy = false
			p.retainWorkspaceComposerFiles(id, files)
			kept := p.composerFilePending[id][:0]
			for _, pending := range p.composerFilePending[id] {
				captured := false
				for _, path := range capturedPaths {
					if pending.path == path {
						captured = true
						break
					}
				}
				if !captured {
					if err != nil {
						pending.problem = err.Error()
					}
					kept = append(kept, pending)
				}
			}
			p.composerFilePending[id] = kept
			if err != nil {
				p.problem = err.Error() + "；草稿文字和已保存的文件引用保留。"
			}
			if p.selected != id || p.generation != generation {
				return
			}
		})
	}()
}
func (p *NativePage) workspaceCapturedFileProblem() string {
	if len(p.composerFilePending[p.selected]) != 0 {
		return "所选文件尚未保存，请重试或移除待保存的选择；草稿文字保留。"
	}
	if len(p.composerFiles[p.selected])+len(p.workspaceReferences[p.selected]) > 8 {
		return "每份草稿最多选择 8 个文件，请移除部分引用。"
	}
	for _, selection := range p.composerFiles[p.selected] {
		f, ok := p.composerFileRecords[selection.ID]
		if !ok || f.SHA256 != selection.SHA256 || f.Scope != selection.Scope {
			return "文件引用尚未恢复或已变化，请重新载入草稿；原件保留。"
		}
	}
	return ""
}
func (p *NativePage) buildWorkspaceCapturedFileCards(c *ui.Context) {
	for i, pending := range p.composerFilePending[p.selected] {
		name := filepath.Base(pending.path)
		meta := "待保存 · 可重试"
		if p.busy {
			meta = "正在保存原件…"
		}
		workspaceAttachmentCard(c, fmt.Sprintf("capture-pending-%d", i), name, meta, "file-text", "Remove pending capture "+name, pending.path+" · "+pending.problem, p.busy || p.inputBlocked(), func() {
			items := p.composerFilePending[p.selected]
			p.composerFilePending[p.selected] = append(items[:i], items[i+1:]...)
		})
	}
	if len(p.composerFilePending[p.selected]) != 0 {
		workspaceControlButton(c, "retry-file-capture", "重试保存文件", "secondary", p.busy || p.inputBlocked()).Label("Retry captured files").OnClick(func() {
			var paths []string
			for _, pending := range p.composerFilePending[p.selected] {
				paths = append(paths, pending.path)
			}
			p.captureWorkspaceFilePaths(paths)
		})
	}

	for _, selection := range p.composerFiles[p.selected] {
		f, ok := p.composerFileRecords[selection.ID]
		name, meta, hint := selection.ID, "引用恢复中", "已保存原件保留"
		if ok {
			name = f.Name
			meta = fmt.Sprintf("已保存 · %d bytes", f.Bytes)
			hint = f.SourcePath + " · " + f.Processing
		}
		workspaceAttachmentCard(c, selection.ID, name, meta, "file-text", "Remove captured file "+name, hint, p.busy || p.inputBlocked(), func() { p.consumeWorkspaceComposerFiles(p.selected, []product.ComposerFileSelection{selection}) })
	}
}

func (p *NativePage) chooseWorkspaceFileCandidate(candidate product.ComposerFileCandidate) {
	if candidate.Kind == "directory" {
		p.picker.explicit = true
		p.picker.query = candidate.Path + string(filepath.Separator)
		p.queryWorkspacePicker(false)
		return
	}
	s := &p.picker
	s.preparing = true
	sequence, scope, id, draft, start, end, explicit := s.sequence, s.scope, p.selected, p.model.Draft, s.start, s.end, s.explicit
	logicalScope := p.workspaceComposerDraftScope()
	ctx := s.requestContext
	if ctx == nil {
		ctx = p.ctx
	}
	finish := p.referenceOwner.begin()
	if finish == nil {
		s.preparing = false
		return
	}
	go func() {
		var f hub.ComposerFile
		var err error
		if candidate.Captured != nil {
			f, err = p.Service.ComposerFileRecord(ctx, *candidate.Captured)
		} else {
			f, err = p.Service.CaptureComposerFile(ctx, logicalScope, candidate.Path)
		}
		finish()
		p.dispatchWorkspacePicker(ctx, func() {
			if ctx.Err() != nil || !p.picker.open || p.picker.sequence != sequence || p.workspacePickerScope() != scope {
				return
			}
			p.picker.preparing = false
			if err != nil {
				p.picker.problem = err.Error()
				return
			}
			if p.model.Draft != draft || p.workspacePickerBlocked() {
				p.picker.problem = "草稿已变化，文件原件已保存；请检查当前草稿后重新选择。"
				return
			}
			if len(p.composerFiles[id]) >= 8 {
				p.picker.problem = "每份草稿最多选择 8 个文件。"
				return
			}
			p.retainWorkspaceComposerFiles(id, []hub.ComposerFile{f})
			runes := []rune(draft)
			if !explicit && start >= 0 && end >= start && end <= len(runes) {
				p.model.Draft = string(runes[:start]) + string(runes[end:])
				p.picker.caret = start
			}
			p.closeWorkspacePicker()
			p.picker.editor.RequestFocus()
		})
	}()
}
