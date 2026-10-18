// INPUT: Owned Worker selection, trusted shared product worktree service and explicit MyGo actions
// OUTPUT: Isolated provisioning, cold hash/path evidence, bounded raw diff and two-step adoption confirmation
// POS: [L7 Hub product] Native controls never choose arbitrary host paths/native IDs, auto-run tasks, merge dirty files or claim verification
//go:build desktop

package hubdesktop

import (
	"errors"
	"fmt"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) worktreeAction(action string, w hub.Worker, offset int) {
	if p.paneBusy || p.Service.Worktrees == nil {
		return
	}
	lead, team := p.teamLead, p.team.Team.ID
	p.paneBusy = true
	p.worktreeConfirm = ""
	go func() {
		var err error
		var page product.WorktreePatchPage
		switch action {
		case "isolate":
			_, err = p.Service.ProvisionWorkerWorktree(p.ctx, lead, team, w.ID, true)
		case "preview":
			_, err = p.Service.PreviewWorkerChanges(p.ctx, lead, team, w.ID)
		case "review-removal":
			_, err = p.Service.ReviewWorkerRemoval(p.ctx, lead, team, w.ID)
		case "remove":
			if w.Worktree != nil && w.Worktree.Removal != nil {
				_, err = p.Service.RemoveWorkerWorktree(p.ctx, lead, team, w.ID, w.Worktree.Revision, w.Worktree.Removal.WorkerSHA256, true)
			} else {
				err = errors.New("saved removal review required")
			}
		case "raw":
			if w.Worktree != nil && w.Worktree.Evidence != nil {
				page, err = p.Service.WorkerPatch(p.ctx, lead, team, w.ID, w.Worktree.Evidence.ID, offset)
			} else {
				err = errors.New("saved evidence required")
			}
		case "adopt":
			if w.Worktree != nil && w.Worktree.Evidence != nil {
				_, err = p.Service.AdoptWorkerChanges(p.ctx, lead, team, w.ID, w.Worktree.Evidence.ID, w.Worktree.Revision, true)
			} else {
				err = errors.New("saved evidence required")
			}
		}
		updated, refreshErr := p.Service.Teams.Store.Team(p.ctx, lead, team)
		p.dispatch(func() {
			p.paneBusy = false
			if p.teamLead != lead || p.team.Team.ID != team || p.pane != "team" {
				return
			}
			if refreshErr == nil {
				p.team = updated
			}
			if err = errors.Join(err, refreshErr); err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			if p.teamWorker == w.ID {
				if action == "raw" {
					p.worktreePatch = page
					p.worktreePatchID = w.Worktree.Evidence.ID
					p.worktreeOffset = offset
				} else {
					p.worktreePatch = product.WorktreePatchPage{}
					p.worktreePatchID = ""
					p.worktreeOffset = 0
				}
			}
			p.refreshSessions()
		})
	}()
}
func (p *NativePage) buildWorkerWorktree(c *ui.Context, w hub.Worker) {
	ui.Text(c, "Worker workspace / adoption evidence").Bold()
	if p.Service.Worktrees == nil {
		ui.Text(c, "Managed isolation is off. Operator must explicitly configure --git-executable and --git-host-approved; no Git discovery, startup, clone, commits or resets.").FontSize(11)
		return
	}
	if w.Worktree == nil {
		ui.Text(c, "Before this Worker's FIRST Turn only: create a new product-private Git worktree at the saved repository HEAD. No checkout hooks/filters execute. It never adopts changes automatically. Shared/Worker must be idle.").FontSize(11)
		ui.Button(c, "Approve Git and isolate new Worker").Disabled(p.paneBusy).OnClick(func() { p.worktreeAction("isolate", w, 0) })
		return
	}
	state := w.Worktree
	ui.Text(c, fmt.Sprintf("%s · adoption=%s · revision=%d\n%s\nbase=%s · %s", state.State, state.Adoption, state.Revision, state.Binding.Path, state.Binding.BaseCommit, state.Problem)).FontSize(11)
	if state.State != "ready" {
		ui.Text(c, "No automatic retry/shared fallback. Partial private targets are retained for explicit human inspection; create a different Worker rather than retargeting existing native history.").FontSize(11)
		return
	}
	ui.Button(c, "Review managed worktree removal").Disabled(p.paneBusy || state.Adoption == "adopting" || state.Adoption == "unknown").OnClick(func() { p.worktreeAction("review-removal", w, 0) })
	if review := state.Removal; review != nil {
		ui.Text(c, fmt.Sprintf("Removal deletes ALL %d files (%d bytes) from %s, including unadopted changes. Worker will be retired; its branch, native history and saved adoption evidence remain.\nReviewed tree %s · HEAD %s", review.Files, review.Bytes, state.Binding.Path, review.WorkerSHA256, review.Head)).FontSize(11)
		confirmation := fmt.Sprintf("remove/%s/%d/%s", w.ID, state.Revision, review.WorkerSHA256)
		ui.Button(c, "Review deletion confirmation").Disabled(p.paneBusy).OnClick(func() { p.worktreeConfirm = confirmation })
		if p.worktreeConfirm == confirmation {
			ui.Button(c, "Confirm delete reviewed worktree and retire Worker").Disabled(p.paneBusy).OnClick(func() { p.worktreeAction("remove", w, 0) })
		}
	}
	ui.Button(c, "Generate exact Worker diff evidence").Disabled(p.paneBusy || state.Adoption != "").OnClick(func() { p.worktreeAction("preview", w, 0) })
	if state.Evidence == nil {
		return
	}
	e := state.Evidence
	ui.Text(c, fmt.Sprintf("%s · %d paths · %d patch bytes\nWorker snapshot %s\nShared snapshot %s\nPatch SHA %s", e.ID, len(e.Changes), e.PatchBytes, e.WorkerSHA256, e.SharedSHA256, e.PatchSHA256)).FontSize(11)
	p.worktreeList.Key = func(i int) any { return e.Changes[i].Path }
	ui.List(c, &p.worktreeList, len(e.Changes), func(i int) {
		change := e.Changes[i]
		ui.Text(c, fmt.Sprintf("%s\n%s %s → %s %s", change.Path, change.BeforeMode, change.BeforeSHA256, change.AfterMode, change.AfterSHA256)).FontSize(11)
	}).Height(130).Shrink(0)
	ui.Button(c, "Read raw diff page (may contain project secrets)").Disabled(p.paneBusy).OnClick(func() { p.worktreeAction("raw", w, 0) })
	if p.worktreePatchID == e.ID && p.worktreePatch.Text != "" {
		ui.Text(c, p.worktreePatch.Processing).FontSize(11)
		ui.Scroll(c).Height(170).Shrink(0).Children(func() { ui.Text(c, p.worktreePatch.Text).FontSize(11) })
		ui.Row(c).Gap(4).Children(func() {
			ui.Button(c, "Previous diff page").Disabled(p.paneBusy || p.worktreeOffset == 0).OnClick(func() { p.worktreeAction("raw", w, max(0, p.worktreeOffset-8<<10)) })
			ui.Button(c, "More diff bytes").Disabled(p.paneBusy || !p.worktreePatch.HasMore).OnClick(func() { p.worktreeAction("raw", w, p.worktreePatch.Next) })
		})
	}
	ui.Text(c, "Adoption changes ONLY these reviewed paths after both workspace-exclusive leases and a fresh SHA/mode check. Shared HEAD/index/files must still equal the base; dirty/conflicts are refused. No commit/stage/test success. Original/proposed bytes remain private for manual recovery. It is optimistic, not an OS multi-file transaction; failure is unknown and never automatically replayed/rolled back.").FontSize(11)
	if state.Adoption == "" {
		confirmation := fmt.Sprintf("%s/%s/%d", w.ID, e.ID, state.Revision)
		ui.Button(c, "Review shared-file adoption confirmation").Disabled(p.paneBusy).OnClick(func() { p.worktreeConfirm = confirmation })
		if p.worktreeConfirm == confirmation {
			ui.Text(c, "Confirm changing saved shared workspace: "+state.Binding.Workspace+" using exactly "+e.ID+". This is not permission to run Worker tasks.").FontSize(11)
			ui.Button(c, "Confirm apply reviewed paths to shared workspace").Disabled(p.paneBusy).OnClick(func() { p.worktreeAction("adopt", w, 0) })
		}
	}
}
